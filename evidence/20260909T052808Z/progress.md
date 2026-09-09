# Run 20260909T052808Z progress

## State

- Branch: `docs/codex-test-handoff`, synced to `79e046c` for the incremental handoff.
- macOS 26.6.2, Apple M3, 24 GiB RAM, Word 16.112.3.
- Rust 1.88.0 and Cargo 1.88.0 were made available in `~/.cargo`.
- Raw baseline before fixes: `cargo fmt --all --check` passed; strict clippy failed on generated `true && ...` expressions.
- Fixes applied before functional testing: generated-code boolean joining, removal of a no-op debug assertion, and two warning-cleanliness fixes. Strict clippy then passed.
- `cargo test --workspace --locked` passed after the fixes.

## Automation run

- Added `crates/rsword/src/bin/rsword_e2e_driver.rs`.
- Driver mode uses the public low-level `Dom::set_text` path, **not** public `EditOp::InsertText`, which remains unsupported on this branch.
- Input batch: 24 selected synthetic DOCX sources.
- Driver results: 19 edited outputs created; 5 source cases failed or were skipped for reasons recorded in `gui-results.jsonl`.
- Every successful driver case verified that a no-edit save was byte-identical and that the edited output reopened through `Document::rebuild` with the marker.

## Word / computer use run

- Word 16.112.3 was driven through real UI open/check/close cycles.
- 15 generated outputs opened in Word and showed `RSWORD-E2E 中文 😀 &<>` through AX.
- Four generated outputs triggered Word errors or repair prompts. In all four cases the corresponding original synthetic fixture also failed in Word, so these are recorded as source-fixture compatibility findings rather than proof of an edit-only regression.
- One Word round trip was saved as `gui-inputs/entity-decoding__001.edited.word-resaved.docx`, reopened in Word, and re-parsed by the Rust driver; the marker remained visible.
- Table cases driven into `w:tbl` content were skipped from the model oracle because M1 `Document::rebuild` represents tables as placeholders and `text_blocks()` does not expose table paragraphs.

## Next work

- Received incremental handoff `79e046c`: create complex corpus through real Word UI actions, not scripted DOCX generation. Continue the existing low-level run as completed evidence; keep it separate from this new Word-authored subline.
- Extend the driver to select a real body paragraph (outside `w:tbl`) rather than the first DOM `w:t`.
- Preserve screenshots and AX captures for every GUI case in per-case directories.
- Re-run after fixtures with known Word incompatibilities are excluded or repaired.
- Build public `EditOp::InsertText` and other supported EditOp tests separately; do not merge their results with the low-level driver run.

## Word-authored complex corpus subline

- Target: one complete theme document first, then the remaining themes and a combined document.
- First theme: `CASE-REPORT-01`, Chinese technical report with headings, TOC, footnotes, sections, and headers/footers.
- Creation rule: use Word UI only for body/structure creation; pasted synthetic text is allowed, script-generated DOCX is not.
- `C01` checkpoint created through Word UI and archived at `word-authored/CASE-REPORT-01/checkpoints/CASE-REPORT-01-C01.docx`.
- `C01` result: `CASE-REPORT-BODY-END` is visible to `Document::rebuild`; no-edit save is byte-identical; XML confirms heading styles `1`, `2`, `3`, and `4`.
- `C01` SHA-256: `3bc3ce0de465914239a14fb820efb8acc4489463ceecd1150c25fb669bb446d5`.
- Input note: ASCII `typeText` worked, while Chinese/emoji `typeText` did not reliably reach Word; Chinese body text was inserted with Word's desktop paste action. This is allowed by the incremental handoff.
- `C02` checkpoint created through Word UI: automatic TOC, one footnote, and PAGE fields.
  - SHA-256: `08f8f2678538ee62b1a557aebb91982568d7e8bdae80cc39a3fcc29310970292`.
  - Independent XML checks found `TOC \o "1-3" \h \z \u`, six `PAGEREF` fields, `CASE-REPORT-FOOTNOTE-END` in `word/footnotes.xml`, and `PAGE \* MERGEFORMAT` in a footer.
  - Rust `Document::rebuild` saw `CASE-REPORT-BODY-END`; no-edit save stayed byte-identical.
- `C03` checkpoint created through Word UI: multiple sections and landscape appendix.
  - SHA-256: `3a39f1a7afba04795730485e51e7eb27a87d1f9a8ad9850469c01d30016522d2`.
  - Independent XML checks found 3 `sectPr` elements and one landscape `pgSz` (`16838x11906`).
  - TOC, footnote, PAGE fields, `CASE-REPORT-APPENDIX`, and `CASE-REPORT-SECTION2-END` remain present.
  - Rust read found the section-2 anchor; no-edit save stayed byte-identical.
- `C04` checkpoint created through Word UI: tracked insertion and a comment.
  - SHA-256: `28ce5f8a31656287da1980ce00013382c42b617dfb79d276b44659f0e4aec33a`.
  - Independent XML checks found two `w:ins` elements, one `commentRangeStart`, `CASE-REPORT-REV-01`, and `CASE-REPORT-COMMENT-END` in `word/comments.xml`.
  - Rust read found the revision marker; no-edit save stayed byte-identical.
- `C05` close/reopen and final checkpoint completed:
  - Closed and reopened `CASE-REPORT-01-C04.docx` in Word; Word Find located both revision and comment markers.
  - Word Save As produced `CASE-REPORT-01-C05.docx`.
  - Final SHA-256: `2aa1e334c8e6a45aa5640713ba69b2dc0e37b58f8221aea6a27d886a78dc956e`.
  - Final package/XML checks found TOC, three sections, appendix, revision, and comment markers.
  - Rust read found the revision marker and no-edit save stayed byte-identical.
- The first Word-authored theme document (`CASE-REPORT-01`) is complete through `C05`; the broader five-theme plus combined-document goal remains active.
- Verification at this checkpoint: `cargo fmt --all --check`, strict `cargo clippy --workspace --all-targets`, and `cargo test --workspace --locked` all passed.
- Second theme `CASE-BID-01` is complete through `C05`.
  - `C01`: heading hierarchy, numbered scope list, and opening markers; SHA-256 `985b308930decee630921add2a54b5861c63cfbc95179cfd4fd19166a7f122ac`.
  - `C02`: 2 tables, one horizontal merge (`gridSpan=1`), one image inside a table cell, and numbered list content; SHA-256 `f54a48fcc3b7c2f748248dde40da74f0f26db4d13d4f02581a876a063b69c344`.
  - `C03`: 2 sections, one landscape section, and 3 tables; SHA-256 `e8bfe413928f16aad52ffb5eb4dd613ec8f1dbd8ad5aeb2998f9ae69d3346f4a`.
  - `C04`: tracked insertion plus comment; SHA-256 `1814208818761a5f0396489233fca1579f46fab9d11391b896532aab3bd6d9b1`.
  - `C05`: close/reopen then Word Save As; final SHA-256 `e94ccd1743f3b7351a1907c7e573ba48c3e76f9379885bf84148432d33206733`.
  - Final independent XML checks retain 2 `sectPr`, one landscape `pgSz`, 3 tables, one in-table drawing, 2 `w:ins`, and one comment range.
  - Rust reads the body revision marker `CASE-BID-REV-01` and preserves no-edit bytes. It does not expose in-table anchors through `text_blocks`; this is classified as model/table projection `NOT_IMPLEMENTED`, not package-open failure.
- Verification after `CASE-BID-01`: `cargo fmt --all --check`, strict `cargo clippy --workspace --all-targets`, and `cargo test --workspace --locked` all passed.
- Branch sync after the user merged `main`: merged `origin/main` (`11759c1`) into `docs/codex-test-handoff` as local commit `e35a79a`; no push was performed.
- Sixth theme `CASE-COMBINED-01` is complete through `C05`.
  - `C00`: identity-only Word-authored document. First Save As check was `b3083d0c5f16a1bd4b4007a87336d57d83d3255d29242bfabf63d4fb6af2775d`; the final archived package after the second save is SHA-256 `a7fbaafedb7818f775016a9f3c6bec0d233034f9bd9be23245fe297b799136cb`.
  - `C01`: 30-page skeleton with four H1/H2/H3/H4 layers; SHA-256 `e9a93eb960e5ca5c510e7755a4605aef9438aff2d803382ec442dd21bd410363`.
  - `C02`: five tables, numbered lists, in-cell list/image, repeated header, Unicode stress, NBSP, manual line break, Arabic/Hebrew, and bold/italic/underline direct formatting completed; SHA-256 `c0a0a88ec8013f1c15ecd3f2118efc0f947b0b308c1f9817e6e9014df24ac207`.
  - `C02` OOXML evidence is in `word-authored/CASE-COMBINED-01/checkpoints/xml-check-C02.txt`; the Word UI evidence includes screenshots for tables and Unicode.
  - The Rust driver reads body/H1/Unicode markers and preserves no-edit bytes. Table-interior projection remains `NOT_IMPLEMENTED`.
  - C00-C02 lessons and automation pitfalls are recorded in `word-authored/CASE-COMBINED-01/checkpoints/C00-C02-lessons.md`.
  - Post-C02 strict clippy failed after the `main` merge on `uninlined_format_args`; fixed in commit `611e10a` across `crates/rsword/src/edit/session.rs`, `crates/rsword/src/edit/shape_gen.rs`, `crates/rsword/src/model/tests.rs`, and `tools/agent-query/src/session.rs`.
  - `C03`: Word-authored sections, automatic TOC, bookmark, external hyperlink, cross-reference, distinct headers/footers, and a section-3 `PAGE` field completed; final SHA-256 `2f41fb60a9b5f675b1aeef0034636e0acd6a707a9f5b24f1a6a49f564a5c3531`.
  - C03 independent XML found 3 `sectPr` elements (default portrait, continuous portrait, landscape), 3 distinct default header targets, 3 distinct default footer targets, one `TOC \o "1-3" \h \z \u`, 15 `PAGEREF` fields, bookmark/`REF`, an external hyperlink relationship, and `PAGE` in the section-3 footer.
  - C03 Rust driver checks all returned `contains_marker=true` and `no_edit_save_byte_identical=true` for eight C03 anchors; evidence is in `checkpoints/C03-driver.txt` and `checkpoints/xml-check-C03.txt`.
  - Verification gates after the clippy fix: strict `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace --locked` both passed before C03 Word UI work began.
  - `C04`: Word-authored tracked insertions/deletions/replacements, Heading 2 TOC entry, tab, manual break, and two persisted comments completed; final SHA-256 `cb93aacb84eec61df540309fca629000758704b3bbc15039c3be86aadce84987`.
  - C04 independent XML found 10 `w:ins`, 2 `w:del`/2 `w:delText`, 2 comment ranges with persisted bodies in `word/comments.xml`, 32 tabs, 2 breaks, 3 sections, 5 tables, 1 drawing, TOC/`PAGEREF`, and retained C03 bookmark/hyperlink/reference/header/footer structures.
  - C04 Rust driver checks all returned `contains_marker=true` and `no_edit_save_byte_identical=true` for eight C04 anchors; evidence is in `checkpoints/C04-driver.txt`, `checkpoints/xml-check-C04.txt`, `checkpoints/C04.md`, and `checkpoints/C04-comments-final.png`.
  - C04 actual-outcome note: the planned clear-direct-formatting actions did not remove formatting; the final OOXML retains direct properties with `w:rPrChange`, recorded as an observed Word outcome discrepancy.
  - `C05`: closed/reopened C04, updated the entire TOC with Track Changes enabled, saved twice, used Save As, and reopened the final package. C04 was restored to `cb93aacb84eec61df540309fca629000758704b3bbc15039c3be86aadce84987` after the temporary TOC-updated working copy `d99159ce749bc7001c5050b1c3d31f9fe39d891a1e98a3ca2784735fbcb8db96` was generated.
  - C05 final SHA-256 is `459a4016749e178503cfd9d7a6e976ec90c9c9cb98802cd3775669eace406a84` (59,924 bytes). Close/reopen showed 34 pages and retained the updated TOC, comments, revisions, bookmark/hyperlink/reference fields, and section headers/footers.
  - C05 independent XML checked 29/29 XML/relationship parts with 0 failures: 42 `w:ins`, 32 `w:del`/`w:delText`, comments `20,27`, 3 sections including 1 landscape, 5 tables, 1 drawing, 16 `PAGEREF` fields, and retained C03 bookmark/hyperlink/REF/TOC/header/footer structures.
  - The TOC cache now contains `CASE-COMBINED-C04-TOC-ENTRY` and its tab/break markers; `PAGEREF` increased from 15 to 16. Word's tracked TOC rewrite reduced direct `w:hyperlink` elements from 16 to 1; this is recorded as the actual Word field-update outcome.
  - C05 Rust driver passed 16/16 C03/C04 marker checks with `contains_marker=true` and `no_edit_save_byte_identical=true`; evidence is in `checkpoints/C05-driver.txt`, `checkpoints/xml-check-C05.txt`, `checkpoints/C05.md`, `checkpoints/C05-final.png`, and `read-report.md`.
  - `CASE-COMBINED-01` now has a complete C00-C05 Word-authored chain. Remaining work is the broader cross-cutting test program in `docs/11-codex-test-handoff.md`, not additional C05 actions.
- Third theme `CASE-CONTRACT-01` is complete through `C05`.
  - `C01`: heading/body skeleton and anchors; SHA-256 `13aa728a2ee21d842163562c8a14de8630355986e7d78374160b92c8d319c519`.
  - `C02`: bookmark `CASE_PARTY_A`, external hyperlink relationship, and `REF CASE_PARTY_A \h`; final C02 SHA-256 `c9895267fc54b78912849c1b104b7b7a11b613110cbc51264c9875dbce841196`.
  - `C03`: direct bold formatting plus retained links/bookmark/field; SHA-256 `f69301c287a47d99a49de44e5a6ae797f49852569af839b28a725e30250d55c8`.
  - `C04`: tracked deletion and insertion plus comment; SHA-256 `bcc082967d6aab96b958524aaba553ef3e9bb6a0a49aca085da092f79da109a5`.
  - `C05`: close/reopen and final Save As; SHA-256 `e45c072ff691d0b706ef8cb6bb868bc83106192f34cb69f0a23fbb596b8d21df`.
  - Final XML retains 3 `w:ins`, 1 `w:del`/`w:delText`, one comment range, bookmark, external hyperlink, REF field, and direct bold runs.
  - Rust reads `CASE-CONTRACT-REV-INSERT` and no-edit save remains byte-identical.
  - Automation risk: two Word Save As calls initially exposed a 0-byte path immediately after the UI returned; explicit second save + wait produced valid packages. This is recorded as tooling timing evidence, not a kernel defect.
- Verification after `CASE-CONTRACT-01`: `cargo fmt --all --check`, strict `cargo clippy --workspace --all-targets`, and `cargo test --workspace --locked` all passed.
- Fourth theme `CASE-LAYOUT-01` is complete through `C05`.
  - `C01`: heading/body skeleton and stable anchors; SHA-256 `5563090941609b6a7993e86be61b6e43308e09509a41aef8615935dd70c50f5f`.
  - `C02`: one inline image, floating images with square wrapping, Word captions, and 2 media relationships; SHA-256 `9a04cb806467038fdf9971a3777ee2790596a43e6d3f2e7c96520704a3ccc4d0`.
  - `C03`: Word text box with `CASE-LAYOUT-TEXTBOX-CONTENT` and formula `x=1`; SHA-256 `1732e04186ef22e0c10ba9c8cdf3db6c5bfb46a0d17cb14f8436c64d80ed6ab6`.
  - `C04`: 3 tracked insertions, 1 tracked deletion/`w:delText`, and one comment; SHA-256 `9450659314c6736b104e2c63db4e5c22a4bc58c8981f0f5b02fcbcb0bc1b07ee`.
  - `C05`: close/reopen, Word Find checks for revision/comment/image/text-box anchors, then final Save As; SHA-256 `5a97df32041ab82b633b02639041b4fa49987e373326b31e15af3b53d48fb873`.
  - Final OOXML checks retain images, wrapping, captions/text box, formula, revisions, and comment. Rust reads body/revision markers and preserves no-edit bytes; text-box and comment projections are recorded as `NOT_IMPLEMENTED`.
- Fifth theme `CASE-UNICODE-01` is complete through `C05`.
  - `C01`: heading hierarchy, CJK/Latin text, emoji surrogate pair, combining mark, Arabic, and Hebrew; SHA-256 `d605c8b0f23b6fbcd7d170a040e65eb19fc4b4a97bb8403147948490512c57a5`.
  - `C02`: tabs, NBSP, manual line break, bold/italic/underline, and larger direct-formatted run; SHA-256 `384458d063852cb09567bbdae3d9f4c05adcd303050d596917c8932b70eaccf0`.
  - `C03`: Word Advanced Symbol inserted copyright text while retaining all Unicode stress cases; SHA-256 `6e57eec1897e71c6d59d6009659659b36d278d29148afa57305f177ba37cef7e`.
  - `C04`: tracked replacement (2 `w:ins`, 1 `w:del`/`w:delText`) and comment; SHA-256 `0cf70939eb46215c5878e869d629ef8be16049221488549a59ecad339550cea3`.
  - `C05`: close/reopen, Word Find checks for all seven stress anchors, final Save As; SHA-256 `d4e608501ec8e4375ab9ea2ae928f1cb2e5c8f03191fd41789e63712c42ac069`.
  - Final Rust reads end, revision, and emoji anchors; no-edit save remains byte-identical. Comment content remains model projection `NOT_IMPLEMENTED`.

## Cross-cutting automated test batch

- Added independent harnesses under the test tree:
  - `crates/rsword/tests/reference_model.rs`: test-side UTF-16/text/run/paragraph model, exhaustive 1-4 step sequences, and stateful save/reopen random sequences.
  - `crates/rsword/tests/structure_variants.rs`: equivalent namespace/prefix/attribute/on-off forms and Strict/Transitional/Mixed package variants.
  - `crates/rsword/tests/metamorphic.rs`: insert-delete, property idempotence, save/reopen, independent paragraph swap, and opaque-part preservation.
- Added `crates/rsword/src/bin/rsword_e2e_driver.rs` for Word-authored marker/open/no-edit-save checks.
- Added standard-library-only independent OOXML/ZIP audit in `tools/independent-ooxml-audit.py` plus mutation self-tests in `tools/test-independent-ooxml-audit.py`.
- Reference model results:
  - Exhaustive: 1,554 sequences / 5,910 operations, PASS.
  - Random baseline: 1,000 seeds x 100 steps = 100,000 operations / 9,828 save-reopen cycles, PASS.
  - Extended: 10,000 x 100 = 1,000,000 operations / 97,480 save-reopen cycles, PASS.
  - Extended: 1,000 x 1,000 = 1,000,000 operations / 105,394 save-reopen cycles, PASS.
- Fuzz: six targets (`xml`, `zip`, `instr`, `bind`, `edit`, `embedded`) each ran 601 seconds with `-timeout=20 -rss_limit_mb=4096`; no crashes. Execution counts are recorded in `logs/fuzz-long-fuzz_*.log`.
- Mutation: initial `edit/plan.rs` run found 34 mutants (19 caught, 13 missed, 2 unviable). New boundary tests reduced the targeted rerun to 23 caught, 1 unviable, 1 missed; the final missed `delete !` was covered by a deleted-but-parented Replace-old regression and reran 1/1 caught. Logs: `logs/mutants-plan-final.log`, `logs/mutants-plan-survivors-final.log`, `logs/mutants-plan-141-final.log`.
- Independent audit: self-test 4/4 passed; the C05 Word-authored package had 29/29 XML/relationship parts valid and 0 failures for the edited/no-edit comparisons. Evidence: `word-authored/CASE-COMBINED-01/checkpoints/independent-audit/`.
- Final gates after the cross-cutting additions:
  - `cargo fmt --all --check` PASS.
  - `cargo clippy --workspace --all-targets -- -D warnings` PASS.
  - `cargo test --workspace --locked` PASS: 981 passed, 0 failed, 13 ignored.
  - `cargo clippy --workspace --all-targets --features compat-ts -- -D warnings` PASS.
  - `cargo test --workspace --locked --features compat-ts` PASS: 1100 passed, 0 failed, 13 ignored.
  - `RUSTFLAGS='-D warnings' cargo check -p rsword --lib` PASS.
- Final confirmation after the last driver-documentation edit:
  - `cargo fmt --all --check` PASS.
  - `cargo clippy --workspace --all-targets -- -D warnings` PASS; log: `logs/final-confirmation-clippy.log`.
  - `cargo test --workspace --locked` PASS; log: `logs/final-confirmation-tests.log`.
- Added `capability-matrix.csv` and `report.md` with PASS/NOT_IMPLEMENTED/SKIPPED_WITH_REASON classifications and artifact paths.

## Remaining scope

- Table interiors, comments, text boxes, and parts of fields/headers/footers remain `NOT_IMPLEMENTED` in the current semantic projection; their package/XML preservation is independently verified.
- Whole-workspace mutation testing was not run; the completed mutation campaign is limited to `crates/rsword/src/edit/plan.rs`.
- Fuzz results are bounded to the recorded 601-second windows. The 10,000 x 1,000 single-batch random configuration was not run; both 10,000 x 100 and 1,000 x 1,000 were run separately and passed.
