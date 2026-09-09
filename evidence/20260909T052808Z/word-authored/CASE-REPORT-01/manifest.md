# CASE-REPORT-01 · Chinese technical report

## Goal

Create a real Word-authored Chinese technical report through Word UI. This is the
first complete checkpoint for the incremental complex-corpus handoff.

## Planned checkpoints

1. `C00`: visible anchors `CASE-REPORT-00`, `CASE-REPORT-TOC`, `CASE-REPORT-FOOTNOTE`.
2. `C01`: four heading levels and mixed Chinese/English body text.
3. `C02`: TOC field, footnote, page numbers.
4. `C03`: three sections with distinct headers/footers and landscape appendix.
5. `C05`: close/reopen and final semantic verification.

## First batch

The current batch will create `C00` plus the first heading/body structure. It
will be saved independently as `CASE-REPORT-01-C01.docx`, then read back by the
current Rust parser and checked for headings, text, and no-edit byte fidelity.
