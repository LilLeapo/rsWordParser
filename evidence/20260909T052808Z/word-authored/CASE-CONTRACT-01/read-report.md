# CASE-CONTRACT-01 read report

## Word creation

Created in Word 16.112.3 through desktop UI. Body text was typed/pasted through
Word; headings, bookmark, external hyperlink, cross-reference field, direct bold
formatting, Track Changes insertion/deletion, and comment were applied in Word.

## C01

- Heading hierarchy: 4 Heading 1 paragraphs and 1 Heading 2 paragraph.
- All stable anchors present.
- Rust read `CASE-CONTRACT-END`; no-edit save was byte-identical.
- SHA-256: `13aa728a2ee21d842163562c8a14de8630355986e7d78374160b92c8d319c519`.

## C02

- Bookmark: one `bookmarkStart/@w:name="CASE_PARTY_A"` and matching end.
- External hyperlink relationship target:
  `https://example.com/rsword-contract` with `TargetMode="External"`.
- Cross-reference field instruction: `REF CASE_PARTY_A \h`.
- Rust read `CASE-CONTRACT-XREF-FIELD`; no-edit save was byte-identical.
- SHA-256: `c9895267fc54b78912849c1b104b7b7a11b613110cbc51264c9875dbce841196`.

## C03

- Retained bookmark, hyperlink, and REF field.
- Direct formatting: 2 `<w:b/>` runs.
- Rust read the formatted cross-reference anchor; no-edit save was byte-identical.
- SHA-256: `f69301c287a47d99a49de44e5a6ae797f49852569af839b28a725e30250d55c8`.

## C04

- Word Track Changes produced 3 `w:ins`, 1 `w:del`, and 1 `w:delText`.
- One comment range anchors `CASE-CONTRACT-COMMENT-END` in `word/comments.xml`.
- Rust read `CASE-CONTRACT-REV-INSERT`; no-edit save was byte-identical.
- SHA-256: `bcc082967d6aab96b958524aaba553ef3e9bb6a0a49aca085da092f79da109a5`.

## C05

- Closed and reopened C04; Word Find located the tracked insertion and comment.
- Word Save As produced the final C05.
- Final XML: 3 `w:ins`; 1 `w:del` + `w:delText`; one comment range; bookmark;
  external hyperlink; `REF CASE_PARTY_A`; 2 bold runs; stable markers.
- Rust read `CASE-CONTRACT-REV-INSERT`; no-edit save was byte-identical.
- SHA-256: `e45c072ff691d0b706ef8cb6bb868bc83106192f34cb69f0a23fbb596b8d21df`.

## Automation observation

Two Word Save As operations briefly exposed a 0-byte destination after the UI
reported success. In both cases, an explicit second save plus wait produced a
valid OOXML package. Original valid C04/C05 results are the files above; no
kernel conclusion is drawn from the transient empty destination.
