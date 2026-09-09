#!/usr/bin/env python3
"""Independent OOXML/ZIP audit using only the Python standard library.

This tool intentionally does not import or invoke rsword.  It compares:
  * no-edit: the complete package bytes;
  * edited: every ZIP local record that is not explicitly allowed to change;
  * XML/relationship well-formedness and relationship target containment;
  * body paragraph structure, comparing every paragraph except one allowed index.

The paragraph check is structural, not a substring check: duplicate paragraphs
are compared in order and multiplicity.
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import platform
import struct
import sys
import zipfile
import xml.etree.ElementTree as ET
from pathlib import Path, PurePosixPath
from typing import Any

W_NS = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
REL_NS = "http://schemas.openxmlformats.org/package/2006/relationships"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical_xml(data: bytes) -> str:
    return ET.canonicalize(data.decode("utf-8"), strip_text=False)


def local_records(data: bytes) -> list[dict[str, Any]]:
    """Return ZIP local records (header + name + extra + compressed payload)."""
    records: list[dict[str, Any]] = []
    with zipfile.ZipFile(io.BytesIO(data), "r") as zf:
        for info in zf.infolist():
            off = info.header_offset
            header = data[off : off + 30]
            if len(header) != 30 or header[:4] != b"PK\x03\x04":
                raise ValueError(f"{info.filename}: invalid local header")
            (_, _, flags, method, mtime, mdate, crc, csize, usize, nlen, xlen) = struct.unpack(
                "<IHHHHHIIIHH", header
            )
            name = data[off + 30 : off + 30 + nlen]
            extra = data[off + 30 + nlen : off + 30 + nlen + xlen]
            payload_start = off + 30 + nlen + xlen
            payload_end = payload_start + csize
            if payload_end > len(data):
                raise ValueError(f"{info.filename}: truncated local record")
            local = data[off:payload_end]
            records.append(
                {
                    "name": info.filename,
                    "name_bytes_hex": name.hex(),
                    "header_offset": off,
                    "flags": flags,
                    "method": method,
                    "date_time": (mdate, mtime),
                    "crc32": crc,
                    "compressed_size": csize,
                    "uncompressed_size": usize,
                    "local_record_bytes": len(local),
                    "local_record_sha256": sha256(local),
                    "payload_sha256": sha256(data[payload_start:payload_end]),
                    "external_attr": info.external_attr,
                    "internal_attr": info.internal_attr,
                    "create_system": info.create_system,
                    "comment": info.comment.hex(),
                }
            )
    return records


def parse_xml_entries(data: bytes) -> tuple[dict[str, Any], list[str]]:
    """Parse all XML and relationship parts; return per-entry info and failures."""
    info: dict[str, Any] = {}
    failures: list[str] = []
    with zipfile.ZipFile(io.BytesIO(data), "r") as zf:
        names = set(zf.namelist())
        for name in sorted(names):
            if not (name.endswith(".xml") or name.endswith(".rels")):
                continue
            try:
                raw = zf.read(name)
                root = ET.fromstring(raw)
            except Exception as exc:  # noqa: BLE001 - report exact parser failure
                failures.append(f"{name}: XML parse failed: {exc}")
                continue
            elements = list(root.iter())
            info[name] = {
                "sha256": sha256(raw),
                "root": root.tag,
                "element_count": len(elements),
                "canonical_sha256": sha256(canonical_xml(raw).encode("utf-8")),
            }
            if name.endswith(".rels"):
                base = PurePosixPath(".") if name == "_rels/.rels" else PurePosixPath(name).parent.parent
                for rel in root.findall(f"{{{REL_NS}}}Relationship"):
                    if rel.get("TargetMode") == "External":
                        continue
                    target = rel.get("Target")
                    if not target:
                        failures.append(f"{name}: relationship without Target")
                        continue
                    if target.startswith("/"):
                        resolved = PurePosixPath(target.lstrip("/"))
                    else:
                        resolved = PurePosixPath(os.path.normpath(str(base / target)))
                    if str(resolved).startswith("..") or not str(resolved):
                        failures.append(f"{name}: relationship target escapes package: {target}")
                    elif str(resolved) not in names:
                        failures.append(f"{name}: relationship target missing: {target}")
    return info, failures


def body_paragraphs(data: bytes) -> list[str] | None:
    try:
        with zipfile.ZipFile(io.BytesIO(data), "r") as zf:
            raw = zf.read("word/document.xml")
        root = ET.fromstring(raw)
    except Exception:
        return None
    body = root.find(f"{{{W_NS}}}body")
    if body is None:
        return None
    return [canonical_xml(ET.tostring(p)) for p in body.findall(f"{{{W_NS}}}p")]


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--before", required=True, type=Path)
    ap.add_argument("--after", required=True, type=Path)
    ap.add_argument("--changed-part", action="append", default=[], help="ZIP entry allowed to change")
    ap.add_argument(
        "--changed-body-paragraph",
        type=int,
        help="0-based body paragraph index allowed to change; all other paragraphs must be structurally equal",
    )
    ap.add_argument("--expect-text", help="text expected somewhere in the allowed changed paragraph")
    ap.add_argument("--out", type=Path)
    args = ap.parse_args()

    before = args.before.read_bytes()
    after = args.after.read_bytes()
    allowed = set(args.changed_part)
    failures: list[str] = []
    checks = 0

    if before == after:
        checks += 1
        if allowed or args.changed_body_paragraph is not None:
            failures.append("bytes are identical but the invocation declared changed content")
    else:
        if not allowed:
            failures.append("bytes differ but no --changed-part was declared")

    try:
        before_records = local_records(before)
        after_records = local_records(after)
    except Exception as exc:  # noqa: BLE001
        print(json.dumps({"status": "FAIL", "failures": [str(exc)]}, ensure_ascii=False, indent=2))
        return 1

    checks += 1
    if [r["name"] for r in before_records] != [r["name"] for r in after_records]:
        failures.append("ZIP entry names/order differ")

    before_by_name = {r["name"]: r for r in before_records}
    after_by_name = {r["name"]: r for r in after_records}
    changed_entries = sorted(set(before_by_name) & set(after_by_name))
    for name in changed_entries:
        b = before_by_name[name]
        a = after_by_name[name]
        if name in allowed:
            continue
        checks += 1
        for key in (
            "method",
            "crc32",
            "compressed_size",
            "uncompressed_size",
            "payload_sha256",
        ):
            if b[key] != a[key]:
                failures.append(f"{name}: untouched local-record field changed: {key}")

    before_xml, xml_failures = parse_xml_entries(before)
    after_xml, after_xml_failures = parse_xml_entries(after)
    failures.extend(f"before {x}" for x in xml_failures)
    failures.extend(f"after {x}" for x in after_xml_failures)
    checks += len(before_xml) + len(after_xml)

    if args.changed_body_paragraph is not None:
        checks += 1
        before_paras = body_paragraphs(before)
        after_paras = body_paragraphs(after)
        if before_paras is None or after_paras is None:
            failures.append("word/document.xml body paragraphs unavailable")
        elif len(before_paras) != len(after_paras):
            failures.append(
                f"body paragraph count changed: {len(before_paras)} -> {len(after_paras)}"
            )
        else:
            idx = args.changed_body_paragraph
            if idx < 0 or idx >= len(before_paras):
                failures.append(f"changed paragraph index out of range: {idx}")
            else:
                for i, (b, a) in enumerate(zip(before_paras, after_paras)):
                    if i == idx:
                        if b == a:
                            failures.append(f"allowed changed paragraph {idx} is structurally unchanged")
                        if args.expect_text and args.expect_text not in after_paras[i]:
                            failures.append(f"expected text missing from paragraph {idx}: {args.expect_text}")
                    elif b != a:
                        failures.append(f"untouched body paragraph {i} changed structurally")

    result = {
        "status": "PASS" if not failures else "FAIL",
        "python": platform.python_version(),
        "zipfile": zipfile.__file__,
        "before": {"path": str(args.before), "bytes": len(before), "sha256": sha256(before)},
        "after": {"path": str(args.after), "bytes": len(after), "sha256": sha256(after)},
        "zip_entries": len(before_records),
        "xml_entries": len(before_xml),
        "changed_parts_allowed": sorted(allowed),
        "checks": checks,
        "failures": failures,
    }
    text = json.dumps(result, ensure_ascii=False, indent=2, sort_keys=True)
    if args.out:
        args.out.write_text(text + "\n", encoding="utf-8")
    print(text)
    return 0 if not failures else 1


if __name__ == "__main__":
    sys.exit(main())
