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
