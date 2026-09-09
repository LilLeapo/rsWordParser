# CASE-CONTRACT-01 · collaborative revised contract

## Goal

Create a synthetic service contract through Microsoft Word UI and exercise
collaborative revision constructs. This is the third of five required theme
documents.

## Stable anchors

- `CASE-CONTRACT-00`: identity marker.
- `CASE-CONTRACT-PARTY-A`: bookmark target paragraph.
- `CASE-CONTRACT-LINK`: external hyperlink paragraph.
- `CASE-CONTRACT-XREF`: cross-reference paragraph.
- `CASE-CONTRACT-REV`: paragraph containing tracked insert and delete revisions.
- `CASE-CONTRACT-END`: final body marker.

## Planned checkpoints

1. `C01`: contract heading/body skeleton and stable anchors.
2. `C02`: bookmark spanning formatted text, external hyperlink, and cross-reference field.
3. `C03`: styled clauses with direct formatting and a heading hierarchy.
4. `C04`: one tracked insertion, one tracked deletion, and one comment anchored near both.
5. `C05`: close/reopen, final Word Save As, independent XML checks, and Rust no-edit fidelity.

## Creation/oracle rules

- All DOCX body/structure must be created through Word 16.112.3 UI.
- Synthetic Chinese contract text may enter via Word paste.
- No scripted/XML/DOCX generation may replace Word UI operations.
- Independent zip/XML checks will verify bookmark, relationship/link, `REF`
  field, `w:ins`, `w:del`, and comment structures.
- Rust verification covers package open, model marker visibility, and no-edit
  byte fidelity. Unsupported projection is explicitly classified as
  `NOT_IMPLEMENTED`, not treated as a pass.
