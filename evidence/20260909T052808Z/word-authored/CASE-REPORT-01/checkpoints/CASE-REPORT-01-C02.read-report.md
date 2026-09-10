# CASE-REPORT-01 C02 read report

## Word UI actions

- Continued from `C01`.
- Positioned at `CASE-REPORT-TOC` with Word Find, inserted an automatic TOC from the References ribbon.
- Positioned at `CASE-REPORT-FOOTNOTE`, inserted a Word footnote, and typed/pasted the footnote body.
- Entered Word footer editing and inserted a `PAGE` field through the Word Field dialog.
- Used Word Save As to create `CASE-REPORT-01-C02.docx`.

## Independent package/XML checks

- `unzip -t`: all archive entries passed.
- `xmllint --noout`: `word/document.xml`, `word/footnotes.xml`, `word/header1.xml`, and `word/footer1.xml` were well-formed.
- Main part contains `TOC \o "1-3" \h \z \u` and six `PAGEREF` instructions.
- `word/footnotes.xml` contains `CASE-REPORT-FOOTNOTE-END`.
- `word/footer2.xml` contains `PAGE \* MERGEFORMAT`.

## Rust read

```json
{"contains_marker":true,"file":"evidence/20260909T052808Z/word-authored/CASE-REPORT-01/checkpoints/CASE-REPORT-01-C02.docx","marker":"CASE-REPORT-BODY-END","no_edit_save_byte_identical":true}
```

SHA-256:

```text
08f8f2678538ee62b1a557aebb91982568d7e8bdae80cc39a3fcc29310970292
```

## Limitation

The Rust driver currently proves package open, model rebuild, marker visibility,
and no-edit byte fidelity. It does not yet emit a dedicated semantic report for
footnotes, headers/footers, or TOC field structure.
