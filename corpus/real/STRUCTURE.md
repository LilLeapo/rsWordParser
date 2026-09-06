# Roundtrip Structural Inspection

Date: 2026-09-07. This report is independent of the Word UI observations in
`ROUNDTRIP.md`. It reads ZIP entries, parses XML, resolves package relationships,
reads embedded XLSX cells, and decodes media through Windows System.Drawing.
It does not modify DOCX files, execute rsword, or use Word automation.

Reproduce with `_scripts/inspect-roundtrip.ps1`. Detailed evidence, original
SHA256 values, media hashes, chart caches, workbook cells, and drawing coordinates
are in `_scripts/roundtrip-structure.json`.

## Integrity

All nine original DOCX files in this delivery have SHA256 values identical to
the corresponding files in
`C:\Users\Administrator\rsWordParser\corpus\real\_roundtrip`.
All inspected XML parts parse successfully. Every internal package relationship
target exists. Every PNG decodes successfully as a 1 x 1 image.

This is targeted structural inspection, not complete OOXML schema validation.
An issue below is an observed package defect or a README mismatch. Its causal
role in Word's error remains a hypothesis unless stated otherwise; no modified
test packages were produced to isolate individual causes.

## Findings

| Files | Structural evidence | Implication for Word testing |
| --- | --- | --- |
| `05-ink-insert.docx` | `[Content_Types].xml` contains two identical `Default Extension="png"` entries. | The duplicate extension declaration is a package defect consistent with Word's recovery prompt. The Word-resaved copy contains only one PNG default. |
| `06-chart-insert-line-pie.docx` | `[Content_Types].xml` contains two identical `Default Extension="xlsx"` entries. | The duplicate extension declaration is a package defect consistent with Word's recovery prompt. The Word-resaved copy contains only one XLSX default. |
| `02-chart-source.docx`, `02-chart-setdata.docx` | `word/charts/chart1.xml` references axes `111` and `222` in `c:barChart`, but its `c:plotArea` contains no `c:catAx` or `c:valAx` definitions. | The incomplete chart exists in the source and remains in the edited sample, consistent with both failing to open. This failure cannot be attributed solely to the cache edit. |
| `02-chart-source.docx`, `02-chart-setdata.docx` | Neither package has an embedded XLSX, chart relationships, or `c:externalData`. | The README request to test whether an old workbook overwrites the edited cache cannot be exercised with these supplied files. Source README line 12 already acknowledges the missing workbook; edited-sample line 13 assumes a workbook exists. |
| `04-image-source.docx`, `04-image-replace.docx` | Each `pic:pic` has only `pic:blipFill`; required children `pic:nvPicPr` and `pic:spPr` are absent. | The malformed picture exists in both source and replacement, consistent with both failing to open. |
| `04-image-source.docx`, `04-image-replace.docx` | Source `word/media/image1.png` and replacement `word/media/image2.png` have identical bytes and hashes. Neither original has `a:srcRect` or `a:fillRect`. | There is no visible media change, crop removal, or fill-window removal available to confirm in these particular supplied originals, despite the README description. |

## Drawing And Data Baselines

- `01-text-source.docx`: two paragraphs with the text specified by the sample
  README, no drawings.
- `01-chart-insert-bar.docx`: clustered column chart, the expected title and
  Q1/Q2/Q3 categories. First series values are 120, 88.5, 96; second series is
  70, missing, 110. Chart cache and embedded workbook agree. Cell C3 is absent,
  matching the missing Q2 second-series point. Size is 576 x 336 px.
- `02-chart-setdata.docx`: edited title and first series name are present in the
  cache, with first-series values 9, 8, 7. The second series remains 70, missing,
  110. It has no workbook, as noted above.
- `03-image-insert-inline-and-square.docx`: one right-aligned square-wrapped
  anchor and one centered inline picture, both 192 x 96 px. Both refer to the
  same PNG. In document order the floating drawing appears before the inline
  drawing. The same geometry and wrap types remain in the Word-resaved copy.
- `04-image-source.docx` and `04-image-replace.docx`: right-aligned square-wrapped
  anchor, vertical paragraph offset 0, size 96 x 96 px. Replacement changes the
  relationship ID and media filename but the media bytes are identical.
- `05-ink-insert.docx`: two ordinary floating pictures named `aidocs-ink 1` and
  `aidocs-ink 2`, no native InkML. Each is 200 x 80 px, `wrapNone`, `behindDoc=0`.
  Column/paragraph offsets are (40, -10) and (300, 20) px. The Word-resaved copy
  retains those positions and sizes; Word deduplicates the identical images.
- `06-chart-insert-line-pie.docx`: two inline charts, each 480 x 288 px. Document
  order is the pie chart before the line chart. Both contain the expected
  month categories and values 3, 5, 2. There are two embedded workbooks with
  matching category labels and numeric cells. The line chart's referenced
  axes do have matching axis definitions.

Every source PNG used by samples 03, 04, and 05 is the same 70-byte image, decoded
as ARGB (127, 0, 255, 0): half-transparent green. The red color in the ink
`docPr/@descr` metadata does not color these raster pixels. Visual inspection
should therefore expect translucent green blocks, not red pen strokes.

## Word-Resaved Copies

The five resaved copies present at inspection time are 01 text, 01 bar, 03 image,
05 ink, and 06 line/pie. The targeted checks above find no corresponding
structural issues in these five copies. This does not replace the requirement
to record whether each original opened with a prompt and what Word displayed.

The four unavailable resaved copies belong to the failing 02 and 04 source/edit
pairs. Their absence must remain explicit in the delivery status.
