# CASE-COMBINED-01 read report

## Document provenance

`CASE-COMBINED-01-C05.docx` was created through Microsoft Word 16.112.3 UI
operations. It combines the Word-authored heading/body skeleton, tables,
images, Unicode stress content, sections, TOC, bookmark, external hyperlink,
cross-reference, headers/footers, tracked revisions, comments, tab, and manual
break. The final C05 step closed and reopened C04, updated the entire TOC while
Track Changes was enabled, saved twice, used Save As, and reopened C05.

Final package:

- Path: `CASE-COMBINED-01-C05.docx`
- Size: 59,924 bytes
- SHA-256: `459a4016749e178503cfd9d7a6e976ec90c9c9cb98802cd3775669eace406a84`

## Kernel read

Driver command form:

```text
rsword_e2e_driver --check CASE-COMBINED-01-C05.docx <marker>
```

Sixteen marker checks passed. Every result reported
`contains_marker=true` and `no_edit_save_byte_identical=true`.

- C04 markers: `CASE-COMBINED-C04-PARA-01`,
  `CASE-COMBINED-C04-REV-INSERT-01`, `CASE-COMBINED-C04-REV-INSERT-02`,
  `CASE-COMBINED-C04-REV-REPLACED-01`,
  `CASE-COMBINED-C04-REV-REPLACED-02`,
  `CASE-COMBINED-C04-TOC-ENTRY`, `CASE-COMBINED-C04-TAB-01`,
  `CASE-COMBINED-C04-BREAK-01`.
- C03 retained markers: `CASE-COMBINED-C03-START`,
  `CASE-COMBINED-C03-HEADING-ONE`, `CASE-COMBINED-C03-HEADING-TWO`,
  `CASE-COMBINED-C03-BOOKMARK-TARGET`,
  `CASE-COMBINED-C03-EXTERNAL-LINK`,
  `CASE-COMBINED-C03-CROSS-REFERENCE`, `CASE-COMBINED-C03-TOC`,
  `CASE-COMBINED-C03-SECTION3`.

Raw output is in `checkpoints/C05-driver.txt`.

## Persisted structure

Independent OOXML checks are the structure oracle and are recorded in
`checkpoints/xml-check-C05.txt`.

- All 29 XML/relationship parts are well formed; ZIP integrity passed.
- TOC field count is 1 and `PAGEREF` count is 16, one more than C04.
- The updated TOC cache contains `CASE-COMBINED-C04-TOC-ENTRY`, its tab
  marker, and its break marker.
- Comment bodies and ranges use IDs `20` and `27`.
- The final package retains 3 sections, 1 landscape section, 5 tables,
  1 drawing, 3 default header references, 3 default footer references,
  3 header markers, 3 footer markers, and 2 `PAGE` fields in header/footer
  parts.
- The bookmark target, external hyperlink relationship, `REF` field, and TOC
  field remain present.
- TOC update under Track Changes added substantial revision markup; the final
  counts are 42 `w:ins`, 32 `w:del`, and 32 `w:delText`.

## Interpretation and limitations

- The kernel successfully opened the final package, exposed all selected body
  markers through `Document::rebuild`, and preserved every byte on a no-edit
  save.
- The driver does not prove full semantic projection for table interiors,
  comments, fields, or headers/footers. Those remain independently verified
  through OOXML and are not reported as kernel semantic support.
- Word's TOC rewrite changed structural counts and direct `w:hyperlink`
  elements. Those are observed Word outcomes, not hidden passes or assumed
  equivalence.
