# CASE-UNICODE-01 · Unicode and formatting stress document

## Goal

Create a synthetic document in Word 16.112.3 that exercises UTF-16 boundaries,
combining marks, bidirectional text, ordinary and unusual whitespace, manual
line breaks, tabs, style inheritance, and multiple direct-formatted runs in one
paragraph.

## Stable anchors

- `CASE-UNICODE-00`: identity marker.
- `CASE-UNICODE-HEADING-A`: level-1 heading.
- `CASE-UNICODE-HEADING-B`: level-2 heading.
- `CASE-UNICODE-MIX`: CJK and Latin text in one paragraph.
- `CASE-UNICODE-EMOJI`: ASCII sentinels around an emoji surrogate pair.
- `CASE-UNICODE-COMBINING`: precomposed and combining-mark spellings.
- `CASE-UNICODE-BIDI`: Latin anchor adjacent to Arabic and Hebrew runs.
- `CASE-UNICODE-SPACE`: tabs, NBSP, and a manual line break.
- `CASE-UNICODE-FORMAT`: one paragraph containing multiple runs.
- `CASE-UNICODE-END`: final marker.

## Planned checkpoints

1. `C01`: heading/body skeleton and the principal Unicode anchors.
2. `C02`: whitespace/manual break boundaries and direct formatting.
3. `C03`: bidirectional text, emoji surrogate, combining marks, and a symbol.
4. `C04`: tracked edit and comment around stable Unicode text.
5. `C05`: close/reopen, final Save As, XML checks, and Rust reads.

## Verification rules

- All Word body structure and formatting must be created in the desktop UI.
- Prepared synthetic text may be pasted through Word's paste action.
- Independent XML checks count run boundaries, tabs, breaks, bidi runs, emoji
  code units, combining sequences, symbols, revisions, and comments.
- Rust checks package opening, visible markers, and no-edit byte fidelity.
- If Unicode text is missing or reordered, record the exact Word checkpoint and
  parser projection without substituting a script-authored DOCX.
