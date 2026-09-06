"""Check copied packages produced by the three real Word chart paste modes."""

from datetime import datetime, timezone
import importlib.util
import json
from pathlib import Path
import re
import shutil
import sys
from urllib.parse import unquote, urlsplit


spec = importlib.util.spec_from_file_location("chartex_checks", Path(__file__).with_name("verify-chartex.py"))
cx = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cx)
C = "http://schemas.openxmlformats.org/drawingml/2006/chart"
A = "http://schemas.openxmlformats.org/drawingml/2006/main"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
NS = {"c": C, "a": A, "r": R}
CASES = ("chart-pasted-embedded", "chart-pasted-linked", "chart-pasted-picture")


def points(node, path):
    return [point.findtext("c:v", default="", namespaces=NS) for point in node.findall(path, NS)]


def inspect(package, name):
    p = package
    p.check("before and after markers", "before \u524d\u6587" in p.text() and "after \u540e\u6587" in p.text())
    chart_parts = sorted(part for part in p.names if re.fullmatch(r"word/charts/chart\d+\.xml", part))
    document_rels = cx.relationships(p, "word/document.xml")
    references = p.document.findall(".//c:chart", NS)
    actual = {"chartParts": chart_parts, "documentRelationships": document_rels}
    parents = {child: parent for parent in p.document.iter() for child in parent}
    floating = []
    for reference in references:
        ancestor = reference
        is_floating = False
        while ancestor in parents:
            ancestor = parents[ancestor]
            is_floating = is_floating or cx.helper.local_name(ancestor.tag) == "anchor"
        floating.append(is_floating)
    actual["floatingCharts"] = floating
    if name == "chart-pasted-picture":
        p.check("picture paste contains no native chart part or reference", not chart_parts and not references)
        image_ids = [node.get("{" + R + "}embed") for node in p.document.findall(".//a:blip", NS)]
        p.check("picture paste has an embedded drawing image", bool(image_ids))
        images = []
        for rid in image_ids:
            rel = document_rels.get(rid, {})
            part = cx.related_path("word/document.xml", rel.get("Target", ""))
            internal = bool(rel) and rel.get("TargetMode") != "External" and part in p.names
            p.check("picture relationship resolves internally", internal, {"id": rid, "relationship": rel})
            if internal:
                payload = p.archive.read(part)
                signature = payload[:16].hex()
                is_png = payload.startswith(b"\x89PNG\r\n\x1a\n")
                is_emf = len(payload) >= 44 and payload[:4] == b"\x01\x00\x00\x00" and payload[40:44] == b" EMF"
                p.check("picture has recognized PNG or EMF signature", is_png or is_emf, {"part": part, "signature": signature})
                images.append({"part": part, "bytes": len(payload), "format": "png" if is_png else "emf" if is_emf else "unknown"})
        actual["images"] = images
        anchors = [node for node in p.document.iter() if cx.helper.local_name(node.tag) == "anchor"]
        p.check("picture paste is inline", not anchors)
        return actual
    p.check("one classic chart part and reference", len(chart_parts) == 1 and len(references) == 1, chart_parts)
    if len(chart_parts) != 1:
        return actual
    part = chart_parts[0]
    chart = p.xml(part)
    rid = references[0].get("{" + R + "}id") if references else None
    referenced_part = cx.related_path("word/document.xml", document_rels.get(rid, {}).get("Target", ""))
    p.check("document chart relationship resolves", referenced_part == part, referenced_part)
    title_node = chart.find("c:chart/c:title", NS)
    title = "" if title_node is None else "".join(node.text or "" for node in title_node.iter() if cx.helper.local_name(node.tag) in {"t", "v"})
    p.check("requested sales title", title == "\u9500\u552e\u7edf\u8ba1", title)
    plot = chart.find("c:chart/c:plotArea", NS)
    kinds = [cx.helper.local_name(node.tag) for node in plot if cx.helper.local_name(node.tag).endswith("Chart")] if plot is not None else []
    bar = chart.find("c:chart/c:plotArea/c:barChart", NS)
    direction = None if bar is None else bar.find("c:barDir", NS)
    grouping = None if bar is None else bar.find("c:grouping", NS)
    p.check("clustered column chart", bar is not None and direction is not None and direction.get("val") == "col" and grouping is not None and grouping.get("val") == "clustered", kinds)
    series_data = []
    for series in chart.findall(".//c:ser", NS):
        names = points(series, "c:tx/c:strRef/c:strCache/c:pt")
        categories = points(series, "c:cat/c:strRef/c:strCache/c:pt")
        values = [cx.number_or_text(value) for value in points(series, "c:val/c:numRef/c:numCache/c:pt")]
        series_data.append({"name": names[0] if names else series.findtext("c:tx/c:v", default="", namespaces=NS), "categories": categories, "values": values, "formulas": [node.text for node in series.findall(".//c:f", NS)]})
    expected = [
        {"name": "Series 1", "categories": ["Category 1", "Category 2", "Category 3"], "values": [10, 20, 30]},
        {"name": "Series 2", "categories": ["Category 1", "Category 2", "Category 3"], "values": [15, 25, 35]},
    ]
    p.check("two cached series match intended categories and values", [{key: row[key] for key in ("name", "categories", "values")} for row in series_data] == expected, series_data)
    legend = chart.find("c:chart/c:legend/c:legendPos", NS)
    p.check("legend is right", legend is not None and legend.get("val") == "r")
    p.check("native pasted chart is inline", floating == [False], floating)
    chart_rels = cx.relationships(p, part)
    external = chart.find("c:externalData", NS)
    p.check("externalData exists", external is not None)
    data_rel = chart_rels.get(None if external is None else external.get("{" + R + "}id"), {})
    if name == "chart-pasted-embedded":
        workbook_part = cx.related_path(part, data_rel.get("Target", ""))
        embedded = bool(data_rel) and data_rel.get("TargetMode") != "External" and workbook_part.endswith(".xlsx") and workbook_part in p.names
        p.check("embedded paste links to internal XLSX", embedded, data_rel)
        if embedded:
            book = cx.workbook_data(p.archive.read(workbook_part))
            actual["embeddedWorkbook"] = book
            expected_cells = cx.expected_cells("chartex-waterfall")
            cells = book["sheets"][0]["cells"] if book["sheets"] else {}
            mismatches = {address: {"expected": value, "actual": cells.get(address)} for address, value in expected_cells.items() if cells.get(address) != value}
            p.check("embedded XLSX matches chart cache dataset", not mismatches, mismatches)
    else:
        p.check("linked paste has external workbook relationship", data_rel.get("TargetMode") == "External" and bool(data_rel.get("Target")), data_rel)
        target = data_rel.get("Target", "")
        parsed = urlsplit(target)
        local_path = None
        if parsed.scheme == "file" and not parsed.netloc:
            decoded = unquote(parsed.path)
            local_path = Path(decoded[1:] if re.match(r"^/[A-Za-z]:", decoded) else decoded)
        elif re.match(r"^[A-Za-z]:[\\/]", target):
            local_path = Path(unquote(target))
        actual["externalWorkbook"] = {"relationship": data_rel, "localPath": None if local_path is None else str(local_path), "localFileExists": None if local_path is None else local_path.is_file()}
    actual.update({"title": title, "chartKinds": kinds, "series": series_data, "legend": None if legend is None else legend.get("val"), "chartRelationships": chart_rels})
    return actual


def main():
    root = Path(__file__).resolve().parent.parent
    stamp = datetime.now(timezone.utc).strftime("%Y%m%d-%H%M%S-%f")
    results = []
    for name in CASES:
        source = root / "chart" / (name + ".docx")
        result = {"case": name, "file": str(source), "status": "missing"}
        results.append(result)
        if not source.is_file():
            continue
        snapshot = root / "_checks" / "pasted" / stamp / source.name
        snapshot.parent.mkdir(parents=True, exist_ok=True)
        digest = cx.helper.sha256(source)
        shutil.copy2(source, snapshot)
        p = cx.helper.Package(snapshot)
        try:
            p.check("snapshot bytes equal original", cx.helper.sha256(snapshot) == digest)
            result["actual"] = inspect(p, name)
            result["status"] = "passed" if all(check["passed"] for check in p.checks) else "failed"
        except Exception as error:
            result.update({"status": "error", "error": f"{type(error).__name__}: {error}"})
        finally:
            p.check("original bytes remain unchanged", cx.helper.sha256(source) == digest)
            if result["status"] == "passed" and not all(check["passed"] for check in p.checks):
                result["status"] = "failed"
            result.update({"sha256": digest, "snapshot": str(snapshot), "checks": p.checks})
            p.close()
    report = root / "_scripts" / ("pasted-chart-package-check-" + stamp + ".json")
    report.write_text(json.dumps({"method": "Copied-package ZIP/XML/XLSX checks only; no Office automation", "results": results}, ensure_ascii=False, indent=2), encoding="utf-8")
    for result in results:
        failures = [check["name"] for check in result.get("checks", []) if not check["passed"]]
        print(result["case"] + ": " + result["status"] + ("; " + "; ".join(failures) if failures else ""))
    print("Report:", report)
    return 0 if all(result["status"] == "passed" for result in results) else 1


if __name__ == "__main__":
    sys.exit(main())
