# CASE-COMBINED-01 C00-C02 lessons learned

## What worked

- Building the case in small checkpoints (`C00` identity, `C01` headings/body, `C02` tables/Unicode/formatting) made failures easy to localize and avoided debugging a huge final document.
- Keeping stable ASCII anchors such as `CASE-COMBINED-TABLE-03` and `CASE-COMBINED-END` made both Word Find and Rust package checks repeatable after saves and UI scrolling.
- Using `unzip -t`, `xmllint`, and the Rust driver as three independent checks was useful: package integrity, OOXML structure, and kernel reading could fail for different reasons.
- Checking no-edit save byte identity at every checkpoint caught unexpected package mutation before it could be mistaken for a semantic edit.

## Word UI automation

- Do not cache AX indexes. Re-read the Word window after each dialog, save, scroll, or ribbon action; indexes shifted repeatedly during table creation.
- `typeText` was reliable for ASCII but not for CJK/emoji. Put non-ASCII text on the clipboard and paste through Word UI instead.
- Word's Save As flow may expose a first saved package before the document is fully settled. Perform a second `Cmd+S`, wait, then record size/hash. The original C00 record used the first-save hash `b3083d0c5f16a1bd4b4007a87336d57d83d3255d29242bfabf63d4fb6af2775d`; the final archived package is `a7fbaafedb7818f775016a9f3c6bec0d233034f9bd9be23245fe297b799136cb`.
- The sidebar Find panel once reported no match for text that was present. The floating Find toolbar (`Cmd+F`) was more reliable for locating anchors.
- Word shortcuts cannot be assumed to match other platforms. `Cmd+Option+4` did not apply Heading 4; the ribbon styles gallery did.
- Never parameterize a keyboard shortcut as a bare character. One formatting attempt sent `b`, `i`, and `u` as plain text and damaged words. Explicit `keystroke "b" using {command down}` and equivalent commands worked.
- For destructive recovery, use Word's Edit menu Undo. It made the accidental formatting replacement recoverable without guessing the macOS shortcut.
- For vertical merge, fill all first-column cells first, select the three cells with Shift+Up from the bottom cell, then use Table > Merge Cells. Do not type while a multi-cell selection is active.
- Use Table > Repeat Header Rows for `w:tblHeader`; manually typed repeated header text is not the same persistent structure.
- Manual line break is `Shift+Enter`; tab and NBSP are distinct from ordinary spaces and must be checked in XML.

## OOXML and Rust verification

- Word may display NBSP as a space and a line break as a new visual line, but the XML differences are exact: `U+00A0` and `w:br/`. Verify the persisted XML, not only the visible text.
- Do not count shared marker text globally. `R2-B` and `R3-B` appear in both `TABLE-02` and `TABLE-04`, so expected duplicate counts must be explicit.
- `w:vMerge` counting must distinguish restart and continue cells. One three-row vertical merge produces one restart plus two continues.
- Tables are persisted successfully even when `Document::rebuild` does not expose table interiors. Record that as model/table projection `NOT_IMPLEMENTED`; do not pretend the Rust reader proved table semantics.
- For images, verify both `w:drawing` and the media relationship/package contents. A visible image alone does not prove the relationship is intact.
- Keep body anchors outside tables. In this kernel revision, body anchors are easy to project, while in-table anchors are not.

## Repository and run discipline

- Stage evidence selectively. Word creates lock files such as `~$SE-COMBINED-01-C02.docx`; never commit those.
- Keep binary checkpoints and screenshots, but verify size before committing. The C00-C02 screenshot set is about 10 MiB, which is acceptable for this run but should not be multiplied casually.
- Keep code fixes and evidence commits separate. The current post-merge source has a pending `crates/rsword/src/error.rs` correction, so it should not silently ride along with C00-C02 evidence.
- Strict clippy is not optional even after all DOCX checks pass. After the `main` merge, `cargo clippy --workspace --all-targets -- -D warnings` currently fails on `uninlined_format_args`; fix and rerun before continuing C03.
