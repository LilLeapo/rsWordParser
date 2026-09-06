"""Inspect copied Word packages only. This script never starts Office or edits DOCX."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import posixpath
import re
import shutil
import sys
import xml.etree.ElementTree as ET
from zipfile import ZipFile


CASES = (
    "text/text-basic", "text/text-custom-styles", "table/table-styled",
    "hf/hf-variants", "sections/sections-three", "fields/fields-toc",
    "revisions/revisions-comments", "shapes/textbox-shapes",
    "sdt/content-controls", "links/hyperlinks-bookmarks",
    "strict/strict-basic", "misc/large-report",
)
W_TRANSITIONAL = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
W_STRICT = "http://purl.oclc.org/ooxml/wordprocessingml/main"
R_TRANSITIONAL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
R_STRICT = "http://purl.oclc.org/ooxml/officeDocument/relationships"
FALSE_VALUES = {"0", "false", "off"}


def local_name(value: str) -> str:
    return value.rsplit("}", 1)[-1]


def sha256(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


class Package:
    def __init__(self, snapshot: Path):
        self.archive = ZipFile(snapshot, "r")
        self.names = set(self.archive.namelist())
        self.document = self.xml("word/document.xml")
        self.w = self.document.tag.partition("}")[0].lstrip("{")
        self.r = R_STRICT if self.w == W_STRICT else R_TRANSITIONAL
        self.ns = {"w": self.w, "r": self.r}
        self.styles = self.xml("word/styles.xml", optional=True)
        self.style_map = {
            node.get(self.attr("styleId")): node
            for node in self.find_all(self.styles, "w:style")
        }
        self.checks: list[dict] = []
        self.warnings: list[str] = []

    def close(self):
        self.archive.close()

    def xml(self, name: str, optional: bool = False):
        if name not in self.names:
            if optional:
                return None
            raise ValueError(f"Missing package part: {name}")
        return ET.fromstring(self.archive.read(name))

    def attr(self, name: str) -> str:
        return "{" + self.w + "}" + name

    def find(self, node, path: str):
        return None if node is None else node.find(path, self.ns)

    def find_all(self, node, path: str):
        return [] if node is None else node.findall(path, self.ns)

    def value(self, node, default=None):
        return default if node is None else node.get(self.attr("val"), default)

    def on(self, node) -> bool:
        return node is not None and str(self.value(node, "1")).lower() not in FALSE_VALUES

    def check(self, name: str, passed: bool, evidence=None):
        self.checks.append({"name": name, "passed": bool(passed), "evidence": evidence})

    def text(self, node=None) -> str:
        return "".join(part.text or "" for part in self.find_all(self.document if node is None else node, ".//w:t"))

    def fields(self, node=None) -> str:
        node = self.document if node is None else node
        fragments = [part.text or "" for part in self.find_all(node, ".//w:instrText")]
        fragments += [part.get(self.attr("instr"), "") for part in self.find_all(node, ".//w:fldSimple")]
        return " ".join(fragments)

    def heading_level(self, style_id: str | None, seen=None):
        seen = set() if seen is None else seen
        if not style_id or style_id in seen:
            return None
        seen.add(style_id)
        style = self.style_map.get(style_id)
        level = self.value(self.find(style, "w:pPr/w:outlineLvl"))
        if level is not None:
            return int(level)
        name = self.value(self.find(style, "w:name"), "").replace(" ", "").lower()
        match = re.fullmatch(r"(?:heading|\u6807\u9898)([1-9])", name)
        if match:
            return int(match.group(1)) - 1
        return self.heading_level(self.value(self.find(style, "w:basedOn")), seen)

    def document_headings(self):
        levels = []
        for paragraph in self.find_all(self.document, ".//w:p"):
            level = self.value(self.find(paragraph, "w:pPr/w:outlineLvl"))
            if level is None:
                level = self.heading_level(self.value(self.find(paragraph, "w:pPr/w:pStyle")))
            if level is not None:
                levels.append(int(level))
        return levels

    def relationships(self):
        relationships = self.xml("word/_rels/document.xml.rels", optional=True)
        if relationships is None:
            return {}
        return {node.get("Id"): dict(node.attrib) for node in relationships}

    def related_part(self, reference, relationships):
        relationship = relationships.get(reference.get("{" + self.r + "}id"), {})
        target = relationship.get("Target", "")
        if target.startswith("/"):
            return target.lstrip("/")
        return posixpath.normpath(posixpath.join("word", target))


def check_basic(package: Package):
    p = package
    p.check("heading levels 1 and 2", {0, 1}.issubset(set(p.document_headings())), p.document_headings())
    numbering = p.xml("word/numbering.xml", optional=True)
    abstract = {n.get(p.attr("abstractNumId")): n for n in p.find_all(numbering, "w:abstractNum")}
    numbers = {n.get(p.attr("numId")): p.value(p.find(n, "w:abstractNumId")) for n in p.find_all(numbering, "w:num")}
    used = []
    for paragraph in p.find_all(p.document, ".//w:p"):
        properties = p.find(paragraph, "w:pPr/w:numPr")
        if properties is None:
            continue
        number = p.value(p.find(properties, "w:numId"))
        level = p.value(p.find(properties, "w:ilvl"), "0")
        definition = abstract.get(numbers.get(number))
        level_node = next((n for n in p.find_all(definition, "w:lvl") if n.get(p.attr("ilvl")) == level), None)
        used.append({"level": int(level), "format": p.value(p.find(level_node, "w:numFmt")), "text": p.value(p.find(level_node, "w:lvlText")), "paragraph": p.text(paragraph)})
    p.check("a bullet paragraph", any(n["format"] == "bullet" for n in used), used)
    numbered = [n for n in used if n["format"] != "bullet"]
    p.check("three numbering levels used", {0, 1, 2}.issubset({n["level"] for n in numbered}), numbered)
    for level, number_format in enumerate(("%1.", "%1.%2.", "%1.%2.%3.")):
        p.check(f"number format level {level + 1}", any(n["level"] == level and n["text"] == number_format for n in numbered), number_format)
    for tag in ("b", "i", "u", "strike"):
        p.check(f"run formatting {tag}", any(p.on(n) for n in p.find_all(p.document, ".//w:rPr/w:" + tag)))
    p.check("superscript", any(p.value(n) == "superscript" for n in p.find_all(p.document, ".//w:vertAlign")))
    p.check("colored run", any(p.value(n, "auto").lower() not in {"auto", "000000"} for n in p.find_all(p.document, ".//w:rPr/w:color")))
    p.check("18 point run", any(p.value(n) == "36" for n in p.find_all(p.document, ".//w:rPr/w:sz")))
    p.check("mixed Chinese and English body", "Mixed English" in p.text())


def check_custom_styles(p: Package):
    custom_name = "\u6211\u7684\u6807\u9898"
    matches = [(identifier, node) for identifier, node in p.style_map.items() if p.value(p.find(node, "w:name")) == custom_name]
    p.check("custom style exists", len(matches) == 1, [identifier for identifier, _ in matches])
    if not matches:
        return
    identifier, style = matches[0]
    parent = p.value(p.find(style, "w:basedOn"))
    p.check("custom style based on Heading 1", p.heading_level(parent) == 0, parent)
    fonts = p.find(style, "w:rPr/w:rFonts")
    p.check("custom font is stored", fonts is not None and bool(fonts.attrib), dict(fonts.attrib) if fonts is not None else None)
    p.check("custom style applied", any(p.value(n) == identifier for n in p.find_all(p.document, ".//w:pStyle")))


def check_table(p: Package):
    tables = p.find_all(p.document, ".//w:tbl")
    p.check("outer and nested tables", len(tables) >= 2, len(tables))
    if not tables:
        return
    outer = tables[0]
    p.check("outer table has three rows and grid columns", len(p.find_all(outer, "w:tr")) == 3 and len(p.find_all(outer, "w:tblGrid/w:gridCol")) == 3)
    style_id = p.value(p.find(outer, "w:tblPr/w:tblStyle"), "")
    style_name = p.value(p.find(p.style_map.get(style_id), "w:name"), "")
    normalized = (style_id + " " + style_name).lower().replace(" ", "").replace("-", "")
    p.check("Grid Table 4 Accent 1 style", "gridtable4accent1" in normalized or "\u7f51\u683c\u88684\u7740\u82721" in normalized, {"id": style_id, "name": style_name})
    look = p.find(outer, "w:tblPr/w:tblLook")
    mask = int(p.value(look, "0"), 16)
    first_row = look.get(p.attr("firstRow")) if look is not None else None
    no_horizontal_bands = look.get(p.attr("noHBand")) if look is not None else None
    p.check("header row table styling", first_row not in FALSE_VALUES if first_row is not None else bool(mask & 0x20))
    p.check("banded row table styling", no_horizontal_bands in FALSE_VALUES if no_horizontal_bands is not None else not bool(mask & 0x200))
    p.check("merged cell", any(int(p.value(n, "1")) >= 2 for n in p.find_all(outer, ".//w:gridSpan")))
    nested = p.find_all(outer, "w:tr/w:tc/w:tbl")
    p.check("nested 2 by 2 table", any(len(p.find_all(t, "w:tr")) == 2 and all(len(p.find_all(r, "w:tc")) == 2 for r in p.find_all(t, "w:tr")) for t in nested))


def check_headers(p: Package):
    section = p.find(p.document, ".//w:sectPr")
    settings = p.xml("word/settings.xml", optional=True)
    p.check("different first page", p.on(p.find(section, "w:titlePg")))
    p.check("different odd and even pages", p.on(p.find(settings, "w:evenAndOddHeaders")))
    relationships = p.relationships()
    headers = []
    for role in ("header", "footer"):
        refs = p.find_all(section, "w:" + role + "Reference")
        kinds = {n.get(p.attr("type")) for n in refs}
        p.check(role + " has all three variants", {"default", "first", "even"}.issubset(kinds), sorted(kinds))
        texts = []
        for reference in refs:
            part = p.related_part(reference, relationships)
            p.check(role + " relationship target exists", part in p.names, part)
            if part not in p.names:
                continue
            xml = p.xml(part)
            texts.append(p.text(xml))
            if role == "footer":
                p.check(part + " has PAGE field", re.search(r"\bPAGE\b", p.fields(xml), re.I) is not None)
            else:
                headers.append(xml)
        p.check(role + " variants have distinct text", len(set(texts)) == 3, texts)
    watermark = any(local_name(n.tag) == "textpath" and "CORPUS" in n.get("string", "") for header in headers for n in header.iter())
    p.check("VML text watermark", watermark)


def check_sections(p: Package):
    sections = p.find_all(p.document, ".//w:sectPr")
    p.check("exactly three sections", len(sections) == 3, len(sections))
    if len(sections) != 3:
        return
    page = p.find(sections[1], "w:pgSz")
    p.check("second section landscape", page is not None and (page.get(p.attr("orient")) == "landscape" or int(page.get(p.attr("w"), "0")) > int(page.get(p.attr("h"), "0"))))
    columns = p.find(sections[2], "w:cols")
    count = int(columns.get(p.attr("num"), "1")) if columns is not None else 1
    p.check("third section has two columns", count == 2, count)
    margins = [tuple(p.find(section, "w:pgMar").get(p.attr(side)) for side in ("top", "bottom", "left", "right")) for section in sections]
    p.check("three distinct margin settings", len(set(margins)) == 3, margins)
    header_ids = [{ref.get("{" + p.r + "}id") for ref in p.find_all(section, "w:headerReference")} for section in sections]
    p.check("second section has an independent header reference", bool(header_ids[1]) and header_ids[1].isdisjoint(header_ids[0]), [sorted(ids) for ids in header_ids])


def check_fields(p: Package):
    codes = p.fields()
    for code in ("TOC", "REF", "DATE"):
        p.check(code + " field", re.search(r"\b" + code + r"\b", codes, re.I) is not None, codes)
    p.check("three heading levels", {0, 1, 2}.issubset(set(p.document_headings())))
    bookmarks = {n.get(p.attr("name")) for n in p.find_all(p.document, ".//w:bookmarkStart")}
    p.check("REF target bookmark", "ChapterOne" in bookmarks and re.search(r"\bREF\s+ChapterOne\b", codes, re.I) is not None)
    for note in ("footnote", "endnote"):
        refs = p.find_all(p.document, ".//w:" + note + "Reference")
        part = p.xml("word/" + note + "s.xml", optional=True)
        ids = {n.get(p.attr("id")) for n in p.find_all(part, "w:" + note)}
        usable = [n.get(p.attr("id")) for n in refs if int(n.get(p.attr("id"), "0")) > 0]
        p.check(note + " has a real note and valid reference", bool(usable) and set(usable).issubset(ids), usable)


def check_revisions(p: Package):
    for tag in ("ins", "del", "rPrChange"):
        p.check("tracked " + tag, bool(p.find_all(p.document, ".//w:" + tag)))
    comments = p.xml("word/comments.xml", optional=True)
    count = len(p.find_all(comments, "w:comment"))
    p.check("two comments and a reply are stored", count >= 3, count)
    extended = p.xml("word/commentsExtended.xml", optional=True)
    nodes = [] if extended is None else list(extended.iter())
    replies = sum(any(local_name(key) == "paraIdParent" and bool(value) for key, value in node.attrib.items()) for node in nodes)
    done = sum(any(local_name(key) == "done" and value not in FALSE_VALUES for key, value in node.attrib.items()) for node in nodes)
    p.check("threaded reply metadata", replies >= 1, replies)
    p.check("resolved comment metadata", done >= 1, done)


def check_shapes(p: Package):
    textboxes = p.find_all(p.document, ".//w:txbxContent")
    p.check("multi-paragraph textbox", any(len(p.find_all(n, "w:p")) >= 2 for n in textboxes))
    p.check("rounded rectangle geometry", any((local_name(n.tag) == "prstGeom" and n.get("prst") == "roundRect") or local_name(n.tag) == "roundrect" for n in p.document.iter()))
    groups = [n for n in p.document.iter() if local_name(n.tag) in {"wgp", "group"}]
    p.check("group of at least two shapes", any(sum(local_name(child.tag) in {"wsp", "shape", "rect", "oval"} for child in group.iter()) >= 2 for group in groups))
    art = any(local_name(n.tag) == "textpath" and bool(n.get("string")) for n in p.document.iter())
    art = art or any(local_name(n.tag) == "prstTxWarp" and n.get("prst") != "textNoShape" for n in p.document.iter())
    p.check("WordArt markup", art)


def check_controls(p: Package):
    properties = p.find_all(p.document, ".//w:sdtPr")
    known_types = {"text", "comboBox", "dropDownList", "date", "picture", "citation", "bibliography", "docPartObj", "docPartList", "equation", "group", "checkbox", "repeatingSection", "repeatingSectionItem"}
    rich = [n for n in properties if p.find(n, "w:richText") is not None or not any(local_name(child.tag) in known_types for child in n)]
    p.check("rich text content control", bool(rich))
    lists = [n for n in properties if p.find(n, "w:dropDownList") is not None]
    p.check("dropdown with two entries", any(len(p.find_all(n, "w:dropDownList/w:listItem")) >= 2 for n in lists))
    p.check("date picker and display format", any(p.find(n, "w:date/w:dateFormat") is not None for n in properties))
    boxes = [child for n in properties for child in n if local_name(child.tag) == "checkbox"]
    checked = any(local_name(child.tag) == "checked" and any(local_name(k) == "val" and v not in FALSE_VALUES for k, v in child.attrib.items()) for box in boxes for child in box)
    p.check("checked checkbox control", checked)
    locks = [p.value(p.find(n, "w:lock")) for n in properties]
    p.check("content and deletion lock", "sdtContentLocked" in locks, locks)


def check_links(p: Package):
    relationships = p.relationships()
    links = p.find_all(p.document, ".//w:hyperlink")
    external = [relationships.get(n.get("{" + p.r + "}id"), {}) for n in links]
    p.check("external HTTP hyperlink", any(n.get("TargetMode") == "External" and n.get("Target", "").startswith(("https://", "http://")) for n in external))
    p.check("mailto hyperlink", any(n.get("TargetMode") == "External" and n.get("Target", "").startswith("mailto:") for n in external))
    bookmarks = {n.get(p.attr("name")) for n in p.find_all(p.document, ".//w:bookmarkStart")}
    anchors = [n.get(p.attr("anchor")) for n in links if n.get(p.attr("anchor"))]
    p.check("internal hyperlink to existing bookmark", bool(anchors) and all(n in bookmarks for n in anchors), anchors)


def check_report(p: Package):
    app = p.xml("docProps/app.xml", optional=True)
    pages = next((int(n.text) for n in app.iter() if local_name(n.tag) == "Pages" and n.text and n.text.isdigit()), None) if app is not None else None
    p.check("stored page count is at least 21", pages is not None and pages >= 21, pages)
    p.check("report has tables", bool(p.find_all(p.document, ".//w:tbl")))
    charts = sorted(n for n in p.names if re.fullmatch(r"word/charts/chart(?:Ex)?\d+\.xml", n))
    p.check("report has charts", bool(charts), charts)
    p.check("report has TOC", re.search(r"\bTOC\b", p.fields(), re.I) is not None)
    p.check("report has header reference", bool(p.find_all(p.document, ".//w:headerReference")))
    p.warnings.append("Package page-count metadata is not a layout oracle; confirm the Word-exported PDF page count and report content separately.")


CHECKERS = {
    "text-basic": check_basic, "text-custom-styles": check_custom_styles,
    "table-styled": check_table, "hf-variants": check_headers,
    "sections-three": check_sections, "fields-toc": check_fields,
    "revisions-comments": check_revisions, "textbox-shapes": check_shapes,
    "content-controls": check_controls, "hyperlinks-bookmarks": check_links,
    "strict-basic": check_basic, "large-report": check_report,
}


def verify(case: str, root: Path, snapshots: Path, suffix: str = "") -> dict:
    source = root / (case + suffix + ".docx")
    record = {"case": case, "variant": suffix, "file": str(source), "status": "missing", "checks": [], "warnings": []}
    if not source.is_file():
        record["warnings"].append("Sample has not been generated. No Office operation was attempted.")
        return record
    snapshot = snapshots / (case + suffix + ".docx")
    snapshot.parent.mkdir(parents=True, exist_ok=True)
    package = None
    try:
        original_hash = sha256(source)
        shutil.copy2(source, snapshot)
        record.update({"snapshot": str(snapshot), "sha256": original_hash})
        package = Package(snapshot)
        package.check("snapshot matches original bytes", sha256(snapshot) == original_hash)
        package.check("document main namespace", package.w == (W_STRICT if case.startswith("strict/") else W_TRANSITIONAL), package.w)
        body = package.text()
        package.check("before and after markers", "before \u524d\u6587" in body and "after \u540e\u6587" in body)
        CHECKERS[case.rsplit("/", 1)[-1]](package)
        package.check("original remains unchanged", sha256(source) == original_hash)
        record["checks"] = package.checks
        record["warnings"] = package.warnings
        record["status"] = "passed" if all(check["passed"] for check in package.checks) else "failed"
    except Exception as error:
        record["status"] = "error"
        record["error"] = f"{type(error).__name__}: {error}"
        if package is not None:
            record["checks"] = package.checks
    finally:
        if package is not None:
            package.close()
    return record


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument("--only", nargs="+", help="Case basenames or domain/basename values")
    parser.add_argument("--suffix", default="", help="Variant suffix, for example 2 selects basename-2.docx")
    parser.add_argument("--require-all", action="store_true", help="Exit 2 if requested samples are missing")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    suffix = args.suffix
    if suffix:
        if not re.fullmatch(r"-?\d+", suffix):
            parser.error("--suffix must be an integer such as 2")
        suffix = "-" + suffix.lstrip("-")
    selected = [case for case in CASES if not args.only or case in args.only or case.rsplit("/", 1)[-1] in args.only]
    if args.only:
        unknown = set(args.only) - set(CASES) - {case.rsplit("/", 1)[-1] for case in CASES}
        if unknown:
            parser.error("Unknown cases: " + ", ".join(sorted(unknown)))
    root = args.root.resolve()
    stamp = datetime.now(timezone.utc).strftime("%Y%m%d-%H%M%S-%f")
    snapshots = root / "_checks" / "p1" / stamp
    records = [verify(case, root, snapshots, suffix) for case in selected]
    counts = {status: sum(r["status"] == status for r in records) for status in ("passed", "failed", "error", "missing")}
    output = args.output or root / "_scripts" / ("p1-package-check-" + stamp + ".json")
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps({"method": "Copy originals, open copied ZIPs read-only, parse XML; no Office automation", "createdUtc": datetime.now(timezone.utc).isoformat(), "counts": counts, "results": records}, ensure_ascii=False, indent=2), encoding="utf-8")
    for record in records:
        failed = [c["name"] for c in record["checks"] if not c["passed"]]
        print(f"{record['case']}: {record['status']}" + ("; " + "; ".join(failed) if failed else ""))
    print("Report:", output)
    if counts["failed"] or counts["error"]:
        return 1
    return 2 if args.require_all and counts["missing"] else 0


if __name__ == "__main__":
    sys.exit(main())
