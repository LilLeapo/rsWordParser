#!/usr/bin/env python3
"""Targeted mutation tests for tools/independent-ooxml-audit.py.

The harness deliberately mutates packages and checks that the independent audit
rejects each non-equivalent mutation.  It does not use rsword.
"""
from __future__ import annotations

import io
import json
import subprocess
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
AUDIT = HERE / "independent-ooxml-audit.py"
W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
REL = "http://schemas.openxmlformats.org/package/2006/relationships"
CT = "http://schemas.openxmlformats.org/package/2006/content-types"


def build_docx(document: str, styles: str = "<w:styles xmlns:w=\"%s\"/>" % W) -> bytes:
    entries = {
        "[Content_Types].xml": (
            f'<Types xmlns="{CT}">'
            '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
            '<Default Extension="xml" ContentType="application/xml"/>'
            '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
            "</Types>"
        ),
        "_rels/.rels": (
            f'<Relationships xmlns="{REL}">'
            '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>'
            "</Relationships>"
        ),
        "word/document.xml": document,
        "word/styles.xml": styles,
    }
    out = io.BytesIO()
    with zipfile.ZipFile(out, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        for name, text in entries.items():
            zf.writestr(name, text.encode("utf-8"))
    return out.getvalue()


def rewrite(original: bytes, replacements: dict[str, bytes]) -> bytes:
    out = io.BytesIO()
    with zipfile.ZipFile(io.BytesIO(original), "r") as zin, zipfile.ZipFile(
        out, "w", compression=zipfile.ZIP_DEFLATED
    ) as zout:
        for info in zin.infolist():
            zout.writestr(info, replacements.get(info.filename, zin.read(info.filename)))
    return out.getvalue()


class AuditMutationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        self.document = (
            f'<w:document xmlns:w="{W}"><w:body>'
            '<w:p><w:r><w:t>alpha</w:t></w:r></w:p>'
            '<w:p><w:r><w:t>beta</w:t></w:r></w:p>'
            '</w:body></w:document>'
        )
        self.before = self.dir / "before.docx"
        self.after = self.dir / "after.docx"
        self.before.write_bytes(build_docx(self.document))

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def audit(self, changed_document: str, extra_args: list[str] | None = None) -> tuple[int, dict]:
        self.after.write_bytes(
            rewrite(self.before.read_bytes(), {"word/document.xml": changed_document.encode("utf-8")})
        )
        args = [
            sys.executable,
            str(AUDIT),
            "--before",
            str(self.before),
            "--after",
            str(self.after),
            "--changed-part",
            "word/document.xml",
        ]
        args.extend(extra_args or [])
        proc = subprocess.run(args, text=True, capture_output=True, check=False)
        return proc.returncode, json.loads(proc.stdout)

    def test_untouched_adjacent_paragraph_deletion_is_rejected(self) -> None:
        changed = (
            f'<w:document xmlns:w="{W}"><w:body>'
            '<w:p><w:r><w:t>alpha</w:t></w:r></w:p>'
            '</w:body></w:document>'
        )
        rc, result = self.audit(changed, ["--changed-body-paragraph", "0"])
        self.assertNotEqual(rc, 0)
        self.assertTrue(any("paragraph count changed" in f for f in result["failures"]), result)

    def test_reordered_duplicate_paragraphs_are_rejected(self) -> None:
        changed = (
            f'<w:document xmlns:w="{W}"><w:body>'
            '<w:p><w:r><w:t>beta</w:t></w:r></w:p>'
            '<w:p><w:r><w:t>alpha</w:t></w:r></w:p>'
            '</w:body></w:document>'
        )
        rc, result = self.audit(changed, ["--changed-body-paragraph", "0"])
        self.assertNotEqual(rc, 0)
        self.assertTrue(any("untouched body paragraph 1 changed" in f for f in result["failures"]), result)

    def test_malformed_changed_xml_is_rejected(self) -> None:
        rc, result = self.audit("<w:document>", ["--changed-body-paragraph", "0"])
        self.assertNotEqual(rc, 0)
        self.assertTrue(any("XML parse failed" in f for f in result["failures"]), result)

    def test_untouched_styles_rewrite_is_rejected(self) -> None:
        # styles.xml changes, but only word/document.xml is declared changed.
        self.after.write_bytes(
            rewrite(
                self.before.read_bytes(),
                {"word/styles.xml": b'<w:styles xmlns:w="%s"><w:style/></w:styles>' % W.encode()},
            )
        )
        proc = subprocess.run(
            [
                sys.executable,
                str(AUDIT),
                "--before",
                str(self.before),
                "--after",
                str(self.after),
                "--changed-part",
                "word/document.xml",
            ],
            text=True,
            capture_output=True,
            check=False,
        )
        result = json.loads(proc.stdout)
        self.assertNotEqual(proc.returncode, 0)
        self.assertTrue(any("word/styles.xml: untouched" in f for f in result["failures"]), result)


if __name__ == "__main__":
    unittest.main(verbosity=2)
