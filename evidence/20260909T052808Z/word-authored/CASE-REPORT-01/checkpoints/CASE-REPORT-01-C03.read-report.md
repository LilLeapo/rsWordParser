# CASE-REPORT-01 C03 read report

## Word UI actions

- Continued from `C02`.
- Added appendix anchors and body text using Word typing/paste.
- Inserted a next-page section break through Word's Insert menu.
- Changed the new section to landscape through Layout > Orientation.
- Added `CASE-REPORT-APPENDIX` and `CASE-REPORT-SECTION2-END` in the later section.
- Used Word Save As to create `CASE-REPORT-01-C03.docx`.

## Independent package/XML checks

- `unzip -t`: all archive entries passed.
- `xmllint --noout`: document, footnotes, and checked footers were well-formed.
- Main part has 3 `<w:sectPr>` elements.
- Page sizes are:

```text
11906x16838 portrait
11906x16838 portrait
16838x11906 landscape
```

- TOC, six `PAGEREF` instructions, footnote marker, and appendix anchors remain.

## Rust read

```json
{"contains_marker":true,"file":"evidence/20260909T052808Z/word-authored/CASE-REPORT-01/checkpoints/CASE-REPORT-01-C03.docx","marker":"CASE-REPORT-SECTION2-END","no_edit_save_byte_identical":true}
```

SHA-256:

```text
3a39f1a7afba04795730485e51e7eb27a87d1f9a8ad9850469c01d30016522d2
```

## Limitation

The Word UI experiment left an earlier draft appendix before the section break.
Both draft and final appendix text are retained deliberately as evidence; later
checkpoint cleanup will either remove or explicitly label the draft content.
