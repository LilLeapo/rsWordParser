# CASE-LAYOUT-01 read report

## Word creation

Created in Microsoft Word 16.112.3 through desktop UI actions. Word menus and
ribbon controls created the image layouts, captions, text box, formula, tracked
edits, and comment. The two 120x120 PNG files were pre-made local assets
selected through Word; no DOCX was generated outside Word.

## C01

- Heading/body skeleton: 3 Heading 1, 1 Heading 2, and 1 Heading 3 paragraphs.
- All planned body anchors are present.
- Rust read `CASE-LAYOUT-END`; no-edit save remained byte-identical.
- SHA-256: `5563090941609b6a7993e86be61b6e43308e09509a41aef8615935dd70c50f5f`.

## C02

- Word inserted one inline image and two floating images with square wrapping.
- Word References > Insert Caption created two caption fields.
- Independent XML/zip checks found 1 inline drawing, 2 anchored drawings, 2
  `wrapSquare` elements, and 2 media PNG relationships.
- Rust read `CASE-LAYOUT-END`; no-edit save remained byte-identical.
- SHA-256: `9a04cb806467038fdf9971a3777ee2790596a43e6d3f2e7c96520704a3ccc4d0`.

## C03

- Word Insert > Text Box created a floating text box. Its unique content is
  `CASE-LAYOUT-TEXTBOX-CONTENT`.
- Word Insert > Formula created `x=1`.
- Independent XML checks found 4 `txbxContent` elements (including MC fallback
  copies), 2 direct occurrences of the text-box marker, 1 `m:oMath`, and the
  expected formula text.
- Retained image structures: 1 inline drawing, 3 anchored drawings, and 2
  square-wrap settings.
- Rust read `CASE-LAYOUT-END`; no-edit save remained byte-identical.
- The text-box marker is not projected by `Document::text_blocks()`; this is
  `NOT_IMPLEMENTED`, not a package-open or save-fidelity failure.
- SHA-256: `1732e04186ef22e0c10ba9c8cdf3db6c5bfb46a0d17cb14f8436c64d80ed6ab6`.

## C04

- Word Review > Track Changes was enabled.
- `drawing` in existing body text was tracked-replaced with
  `CASE-LAYOUT-REV-DELETE`; this produced 1 `w:del` and 1 `w:delText`.
- `CASE-LAYOUT-REV-INSERT` was added as a tracked insertion.
- A Word comment anchored to the revision range contains
  `CASE-LAYOUT-COMMENT-END`.
- Final C04 XML: 3 `w:ins`, 1 `w:del`/`w:delText`, one comment range, and one
  comment marker in `word/comments.xml`.
- Rust read `CASE-LAYOUT-REV-INSERT`; no-edit save remained byte-identical.
- SHA-256: `9450659314c6736b104e2c63db4e5c22a4bc58c8981f0f5b02fcbcb0bc1b07ee`.

## C05

- Closed C04 and reopened it through Word File > Open Recent.
- Word Find found the revision, comment, inline-image anchor, floating-image
  anchor, and text-box anchor.
- Word Save As produced the final C05 archive.
- Final XML/zip checks: valid OOXML, 2 media PNGs, 1 inline drawing, 3 anchored
  drawings, 2 square-wrap settings, 4 `txbxContent`, the text-box content, 1
  `m:oMath` containing `x=1`, 3 `w:ins`, 1 `w:del`/`w:delText`, 1 comment
  range, and the comment marker.
- Flattened `w:t` text contains `CASE-LAYOUT-REV-INSERT`,
  `CASE-LAYOUT-REV-DELETE`, and `CASE-LAYOUT-END`.
- Rust read both body markers; no-edit save remained byte-identical.
- Text-box and comment content remain `NOT_IMPLEMENTED` in the current model
  projection; independent OOXML checks cover their persisted structure.
- SHA-256: `5a97df32041ab82b633b02639041b4fa49987e373326b31e15af3b53d48fb873`.

## Automation observations

- The first attempt to insert a text box selected `CASE-LAYOUT-TEXTBOX` and
  converted it into the text box. That action was undone in Word; a new empty
  insertion point was created before drawing the final text box, preserving the
  body anchor separately from the object content.
- Replacing Word's newly tracked insertion did not produce a deletion against
  original body text. C04 therefore adds a second tracked replacement against
  the pre-existing word `drawing` to obtain the required `w:del`/`w:delText`.
- Save As was followed by an explicit second save and wait because prior cases
  exposed a transient zero-byte destination. C03-C05 all passed `unzip -t`.

## Representative evidence

- Final Word UI capture:
  `C05-final-word-ui.png`.
- Per-checkpoint SHA, zip, XML, and Rust outputs are in `checkpoints/`.
