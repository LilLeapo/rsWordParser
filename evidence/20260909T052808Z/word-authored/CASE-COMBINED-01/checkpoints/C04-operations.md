# CASE-COMBINED-01 C04 operation manifest

## Goal

Create a real Word-authored editing checkpoint with at least 20 meaningful UI
actions, tracked insertions and deletions, direct-format changes, two comments,
and stable anchors. Start from the C03 Word document and preserve the C03
sections, TOC, bookmark, hyperlink, reference, headers, and footers.

## Planned Word UI actions

| step | target | action | expected persistent result |
| --- | --- | --- | --- |
| C04-01 | Review ribbon | Enable Track Changes | subsequent text changes persist as revisions |
| C04-02 | CASE-COMBINED-C03-START | Move to the C03 start anchor | edit cursor is in the intended body region |
| C04-03 | after C03 start | Insert paragraph marker `CASE-COMBINED-C04-PARA-01` | tracked insertion and new paragraph |
| C04-04 | same paragraph | Insert `CASE-COMBINED-C04-REV-INSERT-01` | tracked insertion |
| C04-05 | same paragraph | Insert `CASE-COMBINED-C04-REV-INSERT-02` | tracked insertion |
| C04-06 | REV-INSERT-02 | Apply bold | direct bold on the revised text |
| C04-07 | REV-INSERT-02 | Apply italic | direct italic on the revised text |
| C04-08 | REV-INSERT-02 | Apply underline | direct underline on the revised text |
| C04-09 | REV-INSERT-02 | Clear direct formatting | direct formatting removed while revision remains |
| C04-10 | C03 section-two intro | Delete one existing phrase | tracked deletion/`w:delText` |
| C04-11 | deletion location | Insert `CASE-COMBINED-C04-REV-REPLACED-01` | tracked replacement insertion |
| C04-12 | second C03 intro phrase | Delete one existing phrase | second tracked deletion |
| C04-13 | deletion location | Insert `CASE-COMBINED-C04-REV-REPLACED-02` | second tracked replacement insertion |
| C04-14 | after replacements | Insert Heading 2 paragraph `CASE-COMBINED-C04-TOC-ENTRY` | new heading not yet reflected in cached TOC |
| C04-15 | TOC entry paragraph | Insert `CASE-COMBINED-C04-TAB-01` after a tab | persisted tab |
| C04-16 | TOC entry paragraph | Insert manual line break and `CASE-COMBINED-C04-BREAK-01` | persisted `w:br` |
| C04-17 | TOC entry paragraph | Apply bold to a local marker | direct bold formatting |
| C04-18 | TOC entry paragraph | Clear local direct formatting | direct formatting removed |
| C04-19 | revised paragraph | Add comment `CASE-COMBINED-C04-COMMENT-01` | one comment range/comment |
| C04-20 | TOC entry paragraph | Add comment `CASE-COMBINED-C04-COMMENT-02` | second comment range/comment |
| C04-21 | document | Save twice and verify package | stable final C04 package |

## Assertions

- `w:ins` >= 1 and `w:del`/`w:delText` >= 1.
- Two `commentRangeStart` and two `commentRangeEnd`; `word/comments.xml`
  contains both comment markers.
- C03 section, TOC, bookmark, hyperlink, REF, header, and footer structures
  remain present.
- `unzip -t` and `xmllint` pass.
- Rust driver sees the new stable markers and keeps a no-edit save byte-identical.

## Execution result

- Completed the planned tracked insertions, tracked deletions/replacements,
  Heading 2 TOC entry, tab, manual break, and two comment ranges.
- Final package SHA-256:
  `cb93aacb84eec61df540309fca629000758704b3bbc15039c3be86aadce84987`.
- Independent XML confirmed 10 `w:ins`, 2 `w:del`/2 `w:delText`,
  2 comment ranges, 2 persisted comment bodies, 32 tabs, 2 breaks, 3 sections,
  5 tables, 1 drawing, 1 TOC field, 15 `PAGEREF` fields, and retained C03
  bookmark/hyperlink/reference/header/footer structures.
- The two planned clear-direct-formatting actions did not remove formatting.
  The final runs retain `w:b`/`w:i`/`w:u` where applicable and contain
  `w:rPrChange`; this is recorded as an actual Word outcome discrepancy rather
  than rewritten as a pass.
- Rust driver checks for all eight C04 anchors returned
  `contains_marker=true` and `no_edit_save_byte_identical=true`.
