# CASE-REPORT-01 C05 reopen report

## Reopen

- `CASE-REPORT-01-C04.docx` was closed and reopened in Word.
- Word Find located `CASE-REPORT-REV-01` on page 4.
- Word Find located `CASE-REPORT-COMMENT-END` on page 4.
- Word Save As produced the independent final file `CASE-REPORT-01-C05.docx`.

## Package/XML checks

- Zip integrity passed.
- `word/document.xml` and `word/comments.xml` were well-formed.
- The final file retains:
  - `TOC \o "1-3" \h \z \u`
  - 3 `<w:sectPr>` elements
  - `CASE-REPORT-APPENDIX`
  - `CASE-REPORT-REV-01`
  - `CASE-REPORT-COMMENT-END`

## Rust read

```json
{"contains_marker":true,"file":"evidence/20260909T052808Z/word-authored/CASE-REPORT-01/checkpoints/CASE-REPORT-01-C05.docx","marker":"CASE-REPORT-REV-01","no_edit_save_byte_identical":true}
```

SHA-256:

```text
2aa1e334c8e6a45aa5640713ba69b2dc0e37b58f8221aea6a27d886a78dc956e
```

## Result

The first Word-authored theme document is complete through `C05`. It is not a
claim that all corpus testing is finished; four themes and the combined document
remain.
