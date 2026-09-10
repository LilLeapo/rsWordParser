# CASE-REPORT-01 C01 read report

## Word UI creation

- Created in Microsoft Word through desktop UI: typed ASCII anchors, applied Heading 1-4 styles, and used Word paste for Chinese/English body text.
- Word build under test: 16.112.3.
- Visible anchor used for parser verification: `CASE-REPORT-BODY-END`.

## Kernel read

Driver command: `rsword_e2e_driver --check CASE-REPORT-01-C01.docx CASE-REPORT-BODY-END`.

Result:

```json
{"contains_marker":true,"file":"evidence/20260909T052808Z/word-authored/CASE-REPORT-01/checkpoints/CASE-REPORT-01-C01.docx","marker":"CASE-REPORT-BODY-END","no_edit_save_byte_identical":true}
```

The driver checked `Document::rebuild` and `Package::save`; it does not yet print
the full heading projection. The persisted XML was therefore checked separately.

## Persisted XML

The main document contains paragraph style references with counts:

```text
2 <w:pStyle w:val="1">
1 <w:pStyle w:val="2">
1 <w:pStyle w:val="3">
1 <w:pStyle w:val="4">
```

In the saved `styles.xml`, IDs `1` through `4` map to `heading 1` through
`heading 4`, so all four planned heading levels are present.

## Checkpoint

- SHA-256: `3bc3ce0de465914239a14fb820efb8acc4489463ceecd1150c25fb669bb446d5`
- Path: `checkpoints/CASE-REPORT-01-C01.docx`

## Limitations

- The document is still at the heading/body checkpoint, not yet at TOC, footnote,
  section, header/footer, or final combined scope.
- Word UI style menus did not reliably apply Heading 3/4 in the first attempt;
  the persisted XML confirms that the final Word shortcut path did apply them.
