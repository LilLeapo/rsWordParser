"""Inspect copied Word ChartEx packages, caches, embedded XLSX and PNG fallbacks."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import importlib.util
from io import BytesIO
import json
from pathlib import Path
import posixpath
import re
import shutil
import struct
import sys
import xml.etree.ElementTree as ET
from zipfile import ZipFile


helper_spec = importlib.util.spec_from_file_location("corpus_package_checks", Path(__file__).with_name("verify-p1.py"))
helper = importlib.util.module_from_spec(helper_spec)
helper_spec.loader.exec_module(helper)
CX = "http://schemas.microsoft.com/office/drawing/2014/chartex"
A = "http://schemas.openxmlformats.org/drawingml/2006/main"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
MC = "http://schemas.openxmlformats.org/markup-compatibility/2006"
NS = {"cx": CX, "a": A, "r": R, "mc": MC}
LAYOUTS = {
    "chartex-sunburst": "sunburst", "chartex-treemap": "treemap",
    "chartex-waterfall": "waterfall", "chartex-histogram": "clusteredColumn",
    "chartex-boxwhisker": "boxWhisker", "chartex-funnel": "funnel",
}


def related_path(part: str, target: str) -> str:
    return target.lstrip("/") if target.startswith("/") else posixpath.normpath(posixpath.join(posixpath.dirname(part), target))


def relationship_part(part: str) -> str:
    return posixpath.join(posixpath.dirname(part), "_rels", posixpath.basename(part) + ".rels")


def relationships(package, part: str) -> dict:
    xml = package.xml(relationship_part(part), optional=True)
    return {} if xml is None else {node.get("Id"): dict(node.attrib) for node in xml}


def number_or_text(value: str):
    try:
        number = float(value)
        return int(number) if number.is_integer() else number
    except (TypeError, ValueError):
        return value


def workbook_data(payload: bytes) -> dict:
    with ZipFile(BytesIO(payload), "r") as archive:
        book = ET.fromstring(archive.read("xl/workbook.xml"))
        ns = {"s": book.tag.partition("}")[0].lstrip("{")}
        string_part = ET.fromstring(archive.read("xl/sharedStrings.xml")) if "xl/sharedStrings.xml" in archive.namelist() else None
        shared = [] if string_part is None else ["".join(t.text or "" for t in item.iter() if helper.local_name(t.tag) == "t") for item in string_part]
        rels = ET.fromstring(archive.read("xl/_rels/workbook.xml.rels"))
        targets = {node.get("Id"): related_path("xl/workbook.xml", node.get("Target", "")) for node in rels}
        sheets = []
        for sheet in book.findall("s:sheets/s:sheet", ns):
            rid = next((v for k, v in sheet.attrib.items() if helper.local_name(k) == "id"), None)
            part = targets[rid]
            xml = ET.fromstring(archive.read(part))
            cells = {}
            formulas = {}
            for cell in xml.findall(".//s:sheetData/s:row/s:c", ns):
                address = cell.get("r")
                value_node = cell.find("s:v", ns)
                raw = "" if value_node is None else value_node.text or ""
                cell_type = cell.get("t", "n")
                if cell_type == "s":
                    value = shared[int(raw)]
                elif cell_type == "inlineStr":
                    value = "".join(t.text or "" for t in cell.iter() if helper.local_name(t.tag) == "t")
                elif cell_type == "b":
                    value = raw == "1"
                else:
                    value = number_or_text(raw) if cell_type == "n" else raw
                if value != "":
                    cells[address] = value
                formula = cell.find("s:f", ns)
                if formula is not None:
                    formulas[address] = formula.text or ""
            sheets.append({"name": sheet.get("name"), "part": part, "cells": cells, "formulas": formulas})
        return {"sheets": sheets, "bytes": len(payload)}


def expected_cells(name: str, numeric_column: str = "C") -> dict:
    if name in {"chartex-sunburst", "chartex-treemap"}:
        rows = [
            ["Category", "Subcategory", "Value"],
            ["Group A", "Item 1", 10], ["Group A", "Item 2", 20], ["Group B", "Item 3", 30],
        ]
    else:
        rows = [["Category", "Series 1", "Series 2"]] + [[f"Category {row}", row * 10, row * 10 + 5] for row in range(1, 4)]
    columns = "AB" + numeric_column if name in {"chartex-sunburst", "chartex-treemap"} else "ABC"
    return {f"{column}{row_index}": value for row_index, row in enumerate(rows, 1) for column, value in zip(columns, row)}


def caches(chart) -> list[dict]:
    datasets = []
    for data in chart.findall("cx:chartData/cx:data", NS):
        dimensions = []
        for dimension in data:
            kind = helper.local_name(dimension.tag)
            if kind not in {"strDim", "numDim"}:
                continue
            levels = []
            for level in dimension.findall("cx:lvl", NS):
                points = [{"index": int(point.get("idx", "0")), "value": "".join(point.itertext())} for point in level.findall("cx:pt", NS)]
                if kind == "numDim":
                    for point in points:
                        point["value"] = number_or_text(point["value"])
                levels.append({"declaredPointCount": level.get("ptCount"), "formatCode": level.get("formatCode"), "points": points})
            formula = dimension.find("cx:f", NS)
            dimensions.append({"kind": kind, "type": dimension.get("type"), "formula": None if formula is None else formula.text, "levels": levels})
        datasets.append({"id": data.get("id"), "dimensions": dimensions})
    return datasets


def inspect(package, name: str, dataset: str = "standard") -> dict:
    p = package
    chart_parts = sorted(part for part in p.names if re.fullmatch(r"word/charts/chartEx\d+\.xml", part))
    p.check("one chartEx part", len(chart_parts) == 1, chart_parts)
    p.check("before and after markers", "before \u524d\u6587" in p.text() and "after \u540e\u6587" in p.text())
    if len(chart_parts) != 1:
        return {"chartParts": chart_parts}
    part = chart_parts[0]
    chart = p.xml(part)
    p.check("cx chartSpace root", chart.tag == "{" + CX + "}chartSpace", chart.tag)
    series = chart.findall("cx:chart/cx:plotArea/cx:plotAreaRegion/cx:series", NS)
    layout_ids = [node.get("layoutId") for node in series]
    p.check("requested modern chart layout", LAYOUTS[name] in layout_ids, layout_ids)
    if name == "chartex-histogram":
        p.check("histogram binning settings", chart.find(".//cx:binning", NS) is not None)
    data = caches(chart)
    ids = {item["id"] for item in data}
    data_ids = [node.get("val") for item in series for node in item.findall("cx:dataId", NS)]
    p.check("series reference existing cached datasets", bool(data_ids) and all(value in ids for value in data_ids), data_ids)
    p.check("nonempty cached chart data", bool(data) and any(level["points"] for item in data for dim in item["dimensions"] for level in dim["levels"]))
    numeric_lists = []
    labels = []
    for item in data:
        for dimension in item["dimensions"]:
            for level in dimension["levels"]:
                points = level["points"]
                values = [point["value"] for point in sorted(points, key=lambda point: point["index"])]
                if dimension["kind"] == "numDim" and dimension["type"] in {"val", "size"}:
                    numeric_lists.append(values)
                elif dimension["kind"] == "strDim":
                    labels.extend(str(value) for value in values)
                declared = level["declaredPointCount"]
                valid_indices = declared is None or all(0 <= point["index"] < int(declared) for point in points)
                p.check("cache indices fit declared point count", valid_indices and len({point["index"] for point in points}) == len(points), {"dataId": item["id"], "dimension": dimension["type"], "declared": declared, "indices": [point["index"] for point in points]})
    hierarchical = name in {"chartex-sunburst", "chartex-treemap"}
    broken_cache = [value for value in labels if value in {"#REF!", "#VALUE!", "#NAME?", "#N/A"}]
    p.check("cached categories contain no reference errors", not broken_cache, broken_cache)
    single_series_retry = dataset == "modern-retry" and name in {"chartex-waterfall", "chartex-funnel"}
    expected_numeric = [[10, 20, 30]] if hierarchical or single_series_retry else [[10, 20, 30], [15, 25, 35]]
    matched = [expected for expected in expected_numeric if expected in numeric_lists]
    p.check("chart cache reflects written numeric data", bool(matched), {"cached": numeric_lists, "matchedExpectedSeries": matched})
    if not hierarchical and not single_series_retry and len(matched) < 2:
        p.warnings.append("Only one requested numeric series is present in the chart cache. Some modern chart types plot one series; record the actual UI display and do not claim two displayed series.")
    if dataset == "modern-retry":
        p.check("all requested retry series are cached", len(matched) == len(expected_numeric), {"expected": expected_numeric, "cached": numeric_lists})
        counts = [level["declaredPointCount"] for item in data for dim in item["dimensions"] for level in dim["levels"]]
        p.check("retry cache contains exactly three source slots", bool(counts) and all(count == "3" for count in counts), counts)
    expected_labels = ["Item 1", "Item 2", "Item 3"] if hierarchical else ["Category 1", "Category 2", "Category 3"]
    if name != "chartex-histogram":
        p.check("chart cache reflects written category labels", all(label in labels for label in expected_labels), labels)
    external_nodes = chart.findall(".//cx:externalData", NS)
    chart_rels = relationships(p, part)
    p.check("externalData exists", bool(external_nodes))
    workbooks = []
    for external in external_nodes:
        rid = external.get("{" + R + "}id")
        rel = chart_rels.get(rid, {})
        target = related_path(part, rel.get("Target", ""))
        valid = bool(rel) and rel.get("TargetMode") != "External" and target.endswith(".xlsx") and target in p.names
        p.check("externalData resolves to embedded xlsx", valid, {"id": rid, "relationship": rel, "part": target})
        if valid:
            book = workbook_data(p.archive.read(target))
            book["part"] = target
            workbooks.append(book)
            numeric_column = "C"
            if hierarchical:
                numeric_formula = next((dim["formula"] for item in data for dim in item["dimensions"] if dim["kind"] == "numDim" and dim["type"] == "size" and dim["formula"]), "")
                match = re.search(r"!\$?([A-Z]+)\$?\d+", numeric_formula)
                if match:
                    numeric_column = match.group(1)
            expected = expected_cells(name, numeric_column)
            if dataset == "modern-retry":
                headers = ["Category", "Series 1"] if single_series_retry else ["Category", "Series 1", "Series 2"]
                retry_rows = [headers] + [[f"Category {row}", row * 10] + ([] if single_series_retry else [row * 10 + 5]) for row in range(1, 4)]
                expected = {f"{column}{row_index}": value for row_index, row in enumerate(retry_rows, 1) for column, value in zip("CDE", row)}
                numeric_column = "D"
            actual = book["sheets"][0]["cells"] if book["sheets"] else {}
            mismatches = {address: {"expected": value, "actual": actual.get(address)} for address, value in expected.items() if actual.get(address) != value}
            p.check("embedded workbook contains the intended three data rows", not mismatches, {"numericColumn": numeric_column, "mismatches": mismatches})
            extra_rows = sorted({int(re.search(r"\d+$", address).group()) for address, value in actual.items() if value != "" and int(re.search(r"\d+$", address).group()) > 4})
            p.check("no old populated workbook rows beyond row 4", not extra_rows, extra_rows)
            sheet_names = {sheet["name"] for sheet in book["sheets"]}
            missing_sheets = []
            for item in data:
                for dim in item["dimensions"]:
                    formula = dim["formula"] or ""
                    if "!" in formula:
                        sheet_reference = formula.split("!", 1)[0].strip("'").replace("''", "'")
                        if sheet_reference not in sheet_names:
                            missing_sheets.append({"formula": formula, "referencedSheet": sheet_reference})
            p.check("cached formulas refer to actual embedded worksheet names", not missing_sheets, missing_sheets)
    document_rels = relationships(p, "word/document.xml")
    parents = {child: parent for parent in p.document.iter() for child in parent}
    references = [node for node in p.document.iter("{" + CX + "}chart") if related_path("word/document.xml", document_rels.get(node.get("{" + R + "}id"), {}).get("Target", "")) == part]
    p.check("document cx chart reference resolves", bool(references))
    fallbacks = []
    floating = False
    for reference in references:
        ancestor = reference
        alternate = None
        while ancestor in parents:
            ancestor = parents[ancestor]
            floating = floating or helper.local_name(ancestor.tag) == "anchor"
            if ancestor.tag == "{" + MC + "}AlternateContent" and alternate is None:
                alternate = ancestor
        p.check("chart reference belongs to AlternateContent", alternate is not None)
        fallback = None if alternate is None else alternate.find("mc:Fallback", NS)
        image_ids = set()
        if fallback is not None:
            for image in fallback.iter():
                if helper.local_name(image.tag) in {"blip", "imagedata"}:
                    image_ids.update(value for key, value in image.attrib.items() if helper.local_name(key) in {"embed", "id"})
        p.check("matching fallback contains an image relationship", bool(image_ids), sorted(image_ids))
        for rid in sorted(image_ids):
            rel = document_rels.get(rid, {})
            target = related_path("word/document.xml", rel.get("Target", ""))
            payload = p.archive.read(target) if target in p.names else b""
            is_png = target.lower().endswith(".png") and payload.startswith(b"\x89PNG\r\n\x1a\n")
            p.check("fallback relationship resolves to real embedded PNG", is_png and rel.get("TargetMode") != "External", {"id": rid, "target": target})
            size = list(struct.unpack(">II", payload[16:24])) if is_png and len(payload) >= 24 else None
            fallbacks.append({"relationshipId": rid, "part": target, "png": is_png, "pixelSize": size, "bytes": len(payload)})
    title_node = chart.find("cx:chart/cx:title", NS)
    title = "" if title_node is None else "".join(node.text or "" for node in title_node.iter() if helper.local_name(node.tag) in {"t", "v"})
    legend = chart.find("cx:chart/cx:legend", NS)
    p.check("title is the requested sales title", title == "\u9500\u552e\u7edf\u8ba1", title)
    return {"chartPart": part, "layoutIds": layout_ids, "series": [{"attributes": dict(node.attrib), "dataIds": [child.get("val") for child in node.findall("cx:dataId", NS)], "layoutPropertiesXml": ET.tostring(node.find("cx:layoutPr", NS), encoding="unicode") if node.find("cx:layoutPr", NS) is not None else None} for node in series], "cache": data, "embeddedWorkbooks": workbooks, "fallbackImages": fallbacks, "title": title, "legend": None if legend is None else dict(legend.attrib), "floating": floating, "chartRelationships": chart_rels}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument("--only", nargs="+")
    parser.add_argument("--suffix", default="")
    parser.add_argument("--dataset", choices=("standard", "modern-retry"), default="standard", help="modern-retry expects the retry helper's C:D or C:E data and exactly three source slots")
    parser.add_argument("--require-all", action="store_true")
    args = parser.parse_args()
    selected = list(LAYOUTS) if not args.only else [name.rsplit("/", 1)[-1] for name in args.only]
    unknown = set(selected) - set(LAYOUTS)
    if unknown:
        parser.error("Unknown ChartEx cases: " + ", ".join(sorted(unknown)))
    if args.suffix and not re.fullmatch(r"-?\d+", args.suffix):
        parser.error("--suffix must be an integer such as 2")
    suffix = "-" + args.suffix.lstrip("-") if args.suffix else ""
    root = args.root.resolve()
    stamp = datetime.now(timezone.utc).strftime("%Y%m%d-%H%M%S-%f")
    results = []
    for name in selected:
        source = root / "chart" / (name + suffix + ".docx")
        result = {"case": name, "file": str(source), "status": "missing", "datasetExpectation": args.dataset, "checks": [], "actual": None, "observationStatus": "Package evidence only; actual Word UI observation must be supplied separately."}
        results.append(result)
        if not source.is_file():
            continue
        snapshot = root / "_checks" / "chartex" / stamp / source.name
        snapshot.parent.mkdir(parents=True, exist_ok=True)
        package = None
        try:
            digest = helper.sha256(source)
            shutil.copy2(source, snapshot)
            package = helper.Package(snapshot)
            package.check("copied bytes equal original", helper.sha256(snapshot) == digest)
            result["actual"] = inspect(package, name, args.dataset)
            package.check("original bytes remain unchanged", helper.sha256(source) == digest)
            result.update({"snapshot": str(snapshot), "sha256": digest, "checks": package.checks, "warnings": package.warnings})
            result["status"] = "passed" if all(check["passed"] for check in package.checks) else "failed"
        except Exception as error:
            result["status"] = "error"
            result["error"] = f"{type(error).__name__}: {error}"
            if package is not None:
                result["checks"] = package.checks
        finally:
            if package is not None:
                package.close()
    output = root / "_scripts" / ("chartex-package-check-" + stamp + ".json")
    output.parent.mkdir(parents=True, exist_ok=True)
    counts = {status: sum(item["status"] == status for item in results) for status in ("passed", "failed", "error", "missing")}
    output.write_text(json.dumps({"method": "Copy originals, inspect copied ZIP/XML/XLSX read-only; no Office automation", "createdUtc": datetime.now(timezone.utc).isoformat(), "counts": counts, "results": results}, ensure_ascii=False, indent=2), encoding="utf-8")
    for result in results:
        failures = [check["name"] for check in result["checks"] if not check["passed"]]
        print(result["case"] + ": " + result["status"] + ("; " + "; ".join(failures) if failures else ""))
    print("Report:", output)
    return 1 if counts["failed"] or counts["error"] else (2 if args.require_all and counts["missing"] else 0)


if __name__ == "__main__":
    sys.exit(main())
