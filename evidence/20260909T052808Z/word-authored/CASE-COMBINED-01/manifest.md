# CASE-COMBINED-01 · comprehensive Word-authored complex document

## Goal

Create the sixth required document in Word 16.112.3 by crossing the five
completed theme domains. The initial target is 30-60 pages; page count is
observational, while structural coverage and multi-round edit stability are
required.

## Required cross-domain coverage

- Heading levels 1-4, automatic TOC, PAGE and cross-reference fields.
- At least 3 sections, landscape appendix, and differing header/footer setup.
- Multilevel numbered list and ordinary paragraph interruption.
- Five structurally different tables, including horizontal merge, vertical
  merge, in-cell list/image, repeated header, and a wide landscape table.
- Inline and floating images, square wrap, Word captions, text box, formula,
  and symbol.
- Bookmark, external hyperlink, cross-reference, bold and clearing direct
  formatting.
- CJK/Latin, emoji surrogate pair, combining mark, Arabic/Hebrew, tabs, NBSP,
  manual break, and multiple direct-formatted runs.
- At least 20 meaningful editing actions, tracked insertion/deletion, and two
  comments.

## Planned checkpoints

1. `C00`: identity marker only.
2. `C01`: long heading/body skeleton with style and direct-format boundaries.
3. `C02`: lists, tables, images, captions, text box, formula, symbol, Unicode.
4. `C03`: sections, TOC, bookmark, hyperlink, references, headers/footers.
5. `C04`: multi-round real edits plus revisions and comments.
6. `C05`: close/reopen, field update, final Save As, XML/Rust verification.

## Verification rules

- Word UI performs all structure and object operations; prepared synthetic text
  and local PNG assets may be inserted through Word.
- Each checkpoint is independently archived and hashed.
- Independent OOXML checks are the structure oracle.
- Rust driver proves package open, visible body markers, and no-edit byte
  fidelity. Unsupported table/text-box/comment projection is recorded as
  `NOT_IMPLEMENTED`, not converted into a pass.
- Word page count and Find results are UI observations; XML remains the
  persistent-structure oracle.


## Completion

CASE-COMBINED-01 is complete through C05.

- C00-C04 packages and verification remain archived at their recorded hashes.
- C04 remains `cb93aacb84eec61df540309fca629000758704b3bbc15039c3be86aadce84987`.
- C05 final package is 59,924 bytes with SHA-256
  `459a4016749e178503cfd9d7a6e976ec90c9c9cb98802cd3775669eace406a84`.
- C05 closed/reopened Word verification showed 34 pages and retained the
  updated TOC, comments, revisions, bookmark/hyperlink/reference fields, and
  section headers/footers.
- Independent package audit: 29 XML/relationship parts passed, ZIP passed, and
  all assertions in `checkpoints/xml-check-C05.txt` passed.
- Rust driver: 16/16 C03/C04 marker checks returned
  `contains_marker=true` and `no_edit_save_byte_identical=true`; see
  `checkpoints/C05-driver.txt`.
- Word's tracked TOC update changed revision and field counts; this is recorded
  as the actual Word outcome rather than treated as byte-stable field behavior.
- Evidence: `checkpoints/C05.md`, `checkpoints/C05-final.png`,
  `checkpoints/xml-check-C05.txt`, `checkpoints/C05-driver.txt`, and
  `read-report.md`.
