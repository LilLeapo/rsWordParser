# CASE-REPORT-01 C04 read report

## Word UI actions

- Continued from `C03`.
- Enabled Word Track Changes through Review > Track Changes.
- Moved to `CASE-REPORT-SECTION2-END` and typed ` CASE-REPORT-REV-01`, creating a tracked insertion.
- Selected the surrounding revision line and inserted a Word comment.
- Word initially delivered the pasted comment text late; an additional typed anchor caused the text `CASE-REPORT-COMMENT-END` to occur twice. The duplicate is retained and recorded as a UI automation observation.
- Used Word Save As to create `CASE-REPORT-01-C04.docx`.

## Independent package/XML checks

- `unzip -t`: all archive entries passed.
- `xmllint --noout`: `word/document.xml`, `word/comments.xml`, and `word/footnotes.xml` were well-formed.
- Main part counts:

```text
2 <w:ins>
1 <w:commentRangeStart>
1 CASE-REPORT-REV-01
```

- `word/comments.xml` contains comment id `5`, author `燚坡 李`, and two occurrences of `CASE-REPORT-COMMENT-END`.

## Rust read

```json
{"contains_marker":true,"file":"evidence/20260909T052808Z/word-authored/CASE-REPORT-01/checkpoints/CASE-REPORT-01-C04.docx","marker":"CASE-REPORT-REV-01","no_edit_save_byte_identical":true}
```

SHA-256:

```text
28ce5f8a31656287da1980ce00013382c42b617dfb79d276b44659f0e4aec33a
```
