"""Audit copies of Word-authored DOCX packages without modifying originals."""
from pathlib import Path
from collections import Counter
import argparse
import hashlib
import json
import posixpath
import shutil
from urllib.parse import unquote
from zipfile import ZipFile
import xml.etree.ElementTree as ET

NAMES = {
    "chart": "chart-column chart-bar chart-stacked chart-percent-stacked chart-line chart-line-plain chart-pie chart-doughnut chart-area chart-scatter chart-scatter-lines chart-bubble chart-combo chart-3d chart-dates chart-style chart-style-gray chart-point-color chart-legend chart-no-legend chart-no-title chart-floating chart-in-table chartex-sunburst chartex-treemap chartex-waterfall chartex-histogram chartex-boxwhisker chartex-funnel chart-pasted-embedded chart-pasted-linked chart-pasted-picture",
    "smartart": "smartart-list smartart-hierarchy smartart-process smartart-cycle smartart-picture smartart-styled smartart-floating smartart-in-table smartart-edited-text",
    "canvas": "canvas-shapes canvas-picture canvas-resized canvas-floating canvas-textbox",
    "math": "math-fraction math-integral math-matrix math-inline math-display-two math-builtin math-latex math-linear math-styled math-in-table",
    "ole": "ole-excel-embedded ole-excel-linked ole-icon ole-ppt ole-with-text ole-in-table",
    "ink": "ink-pen ink-highlighter ink-to-shape ink-math",
    "image": "image-inline image-wrap-square image-wrap-tight image-behind image-front image-top-bottom image-cropped image-svg image-linked image-insert-and-link image-emf image-rotated image-alt-decorative image-two-in-run",
    "text": "text-basic text-custom-styles", "table": "table-styled", "hf": "hf-variants",
    "sections": "sections-three", "fields": "fields-toc", "revisions": "revisions-comments",
    "shapes": "textbox-shapes", "sdt": "content-controls", "links": "hyperlinks-bookmarks",
    "strict": "strict-basic", "misc": "large-report",
}
NS = {
    "w": "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
    "m": "http://schemas.openxmlformats.org/officeDocument/2006/math",
    "o": "urn:schemas-microsoft-com:office:office",
    "v": "urn:schemas-microsoft-com:vml",
    "a": "http://schemas.openxmlformats.org/drawingml/2006/main",
    "wp": "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
}


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def audit(source, root):
    relative = source.relative_to(root).as_posix()
    copy = root / "_checks" / "final" / relative
    copy.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, copy)
    result = {"file": relative, "bytes": source.stat().st_size, "sha256": sha256(source)}
    result["copyHashMatches"] = sha256(copy) == result["sha256"]
    issues = []
    with ZipFile(copy) as package:
        names = package.namelist()
        result["partCount"] = len(names)
        result["crcError"] = package.testzip()
        result["duplicateEntries"] = [name for name, count in Counter(names).items() if count > 1]
        xml = {}
        for name in names:
            if name.endswith((".xml", ".rels")):
                try:
                    xml[name] = ET.fromstring(package.read(name))
                except ET.ParseError as error:
                    issues.append(f"Invalid XML {name}: {error}")
        content_types = xml.get("[Content_Types].xml")
        if content_types is not None:
            keys = [(element.tag.rsplit("}", 1)[-1], element.get("Extension", element.get("PartName"))) for element in content_types]
            result["duplicateContentTypes"] = [list(key) for key, count in Counter(keys).items() if count > 1]
        external = []
        broken = []
        for name, part in xml.items():
            if not name.endswith(".rels"):
                continue
            base = "" if name == "_rels/.rels" else posixpath.dirname(posixpath.dirname(name))
            for relationship in part:
                target = relationship.get("Target", "")
                record = {"part": name, "id": relationship.get("Id"), "type": relationship.get("Type"), "target": target}
                if relationship.get("TargetMode") == "External":
                    external.append(record)
                    continue
                path = unquote(target).split("#", 1)[0]
                resolved = posixpath.normpath(path.lstrip("/") if path.startswith("/") else posixpath.join(base, path))
                if resolved not in names:
                    broken.append({**record, "resolved": resolved})
        result["brokenInternalRelationships"] = broken
        result["externalRelationships"] = external
        doc = xml.get("word/document.xml")
        if doc is not None:
            strict = doc.tag.startswith("{http://purl.oclc.org/ooxml/wordprocessingml/main}")
            ns = dict(NS)
            if strict:
                ns["w"] = "http://purl.oclc.org/ooxml/wordprocessingml/main"
            result["flavor"] = "Strict" if strict else "Transitional"
            result["elementCounts"] = dict(Counter(element.tag for element in doc.iter()))
            texts = [element.text or "" for element in doc.findall(".//w:t", ns)]
            text = "".join(texts)
            result["beforeAfterMarkers"] = {"before": "before" in text, "after": "after" in text}
            result["documentText"] = text[:12000]
            result["sections"] = len(doc.findall(".//w:sectPr", ns))
            result["tables"] = len(doc.findall(".//w:tbl", ns))
            result["inlineObjects"] = len(doc.findall(".//wp:inline", ns))
            result["floatingObjects"] = len(doc.findall(".//wp:anchor", ns))
            result["equationsPerMathParagraph"] = [len(p.findall("m:oMath", ns)) for p in doc.findall(".//m:oMathPara", ns)]
            result["oleObjects"] = [dict(element.attrib) for element in doc.findall(".//o:OLEObject", ns)]
            result["contentControls"] = [[node.tag.rsplit("}", 1)[-1] for node in element] for element in doc.findall(".//w:sdtPr", ns)]
            result["headerFooterReferences"] = [dict(element.attrib) for element in doc.findall(".//w:headerReference", ns) + doc.findall(".//w:footerReference", ns)]
            result["fieldInstructions"] = [element.text for element in doc.findall(".//w:instrText", ns)]
            result["revisions"] = {tag: len(doc.findall(f".//w:{tag}", ns)) for tag in ["ins", "del", "rPrChange"]}
        result["featureParts"] = [name for name in names if name.startswith(("word/charts/", "word/diagrams/", "word/embeddings/", "word/ink/", "word/media/", "word/comments", "word/header", "word/footer", "word/footnotes", "word/endnotes"))]
    result["xmlIssues"] = issues
    result["sourceUnchanged"] = result["sha256"] == sha256(source)
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--input-zip", type=Path, default=Path("C:/word/_roundtrip.zip"))
    args = parser.parse_args()
    root = args.root
    files = []
    for domain in [*NAMES, "_roundtrip"]:
        files.extend(path for path in (root / domain).glob("*.docx") if not path.name.startswith("~$"))
    audits = [audit(path, root) for path in sorted(files)]
    inventory = []
    for domain, names in NAMES.items():
        for name in names.split():
            candidates = [item["file"] for item in audits if Path(item["file"]).parent.as_posix() == domain and (Path(item["file"]).stem == name or (Path(item["file"]).stem.startswith(name + "-") and Path(item["file"]).stem[len(name) + 1:].isdigit()))]
            inventory.append({"requested": f"{domain}/{name}.docx", "savedCandidates": candidates, "visualAndFeatureAcceptance": "See OBSERVED.md; package existence alone is not acceptance"})
    originals = []
    if args.input_zip.exists():
        with ZipFile(args.input_zip) as package:
            for name in package.namelist():
                if not name.startswith("_roundtrip/") or not name.endswith(".docx"):
                    continue
                digest = hashlib.sha256(package.read(name)).hexdigest()
                path = root / name
                originals.append({"file": name, "inputZipSha256": digest, "deliveredOriginalMatches": path.exists() and sha256(path) == digest})
    result = {"method": "Read-only ZIP and XML checks on copied DOCX files. No Word operations and no package rewriting.", "requestedCount": len(inventory), "requestedWithSavedCandidate": sum(bool(item["savedCandidates"]) for item in inventory), "suppliedOriginals": originals, "inventory": inventory, "files": audits}
    output = root / "_scripts" / "corpus-audit.json"
    output.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({"report": str(output), "requested": len(inventory), "requestedWithSavedCandidate": result["requestedWithSavedCandidate"], "docxFiles": len(audits), "crcFailures": sum(bool(item["crcError"]) for item in audits), "xmlFailures": sum(bool(item["xmlIssues"]) for item in audits), "brokenInternalRelationships": sum(bool(item["brokenInternalRelationships"]) for item in audits), "sourceHashesUnchanged": all(item["sourceUnchanged"] for item in audits), "suppliedOriginalsMatched": sum(item["deliveredOriginalMatches"] for item in originals)}, indent=2))


if __name__ == "__main__":
    main()
