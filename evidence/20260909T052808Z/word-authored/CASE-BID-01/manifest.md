# CASE-BID-01 · complex bid quotation

## Goal

Create a synthetic Chinese bid/quotation document through Word UI. This is the
second of the five required theme documents. It focuses on tables, merged
cells, numbered lists, images inside table cells, and section orientation.

## Stable anchors

- `CASE-BID-00`: opening identity marker.
- `CASE-BID-SCOPE`: numbered scope list.
- `CASE-BID-TABLE-01`: wide pricing table with horizontally merged header cells.
- `CASE-BID-TABLE-02`: table containing a list and an image in separate cells.
- `CASE-BID-LANDSCAPE`: landscape section summary table.
- `CASE-BID-END`: final semantic marker.

## Planned checkpoints

1. `C00`: blank Word document with opening marker and first heading.
2. `C01`: title/heading structure, direct formatting, and scope list.
3. `C02`: two tables, merged header cell, cell paragraphs, cell image.
4. `C03`: landscape section and section-specific header/footer text.
5. `C04`: a small tracked edit/comment history.
6. `C05`: close/reopen and final independent verification.

## Creation and oracle rules

- All DOCX structure will be created through Microsoft Word 16.112.3 UI.
- Synthetic Chinese body text may be pasted through the Word desktop paste action.
- The local PNG is a pre-made test asset, not a script-generated DOCX.
- Each checkpoint will be independently checked with zip/XML tools and opened
  through the current Rust parser.
- Kernel support for table semantics is expected to be partial on this branch,
  so unsupported semantic reading is recorded as `NOT_IMPLEMENTED`; package
  fidelity and marker visibility remain independently testable.
