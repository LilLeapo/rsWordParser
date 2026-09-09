# Task A: Word revisions and before/after reference files

24 native Word DOCX files, 24 direct Word GUI observations, 24 Word-exported PDFs / 26 independently viewed pages. All 125 recorded structural checks pass. Every current DOCX hash matches the original authoring readout; each accepted/rejected pair independently starts from the same unchanged tracked DOCX.

## Method and exact-procedure differences

Windows Word created and saved every DOCX. The edits, acceptance/rejection, section operations and shape changes were executed through native Word COM APIs. They were not performed by clicking every ribbon command in the task. The GUI was directly inspected and photographed after saving. This report claims native Word output and observed GUI behavior, not completion of every literal menu/keyboard/mouse procedure.

Accepted and rejected files were opened independently from the same tracked bytes, never made through accept/undo/reject. Each output path was saved once. The exception in observation sequence is run-edits/accepted: it was reopened read-only for its screenshot after a window-activation interruption and never resaved. Original readouts retain their historical `uiPending=true`; the final joined summary records the completed GUI checks without altering those snapshots.

Section deletion used `Range.Delete` on the section-break character, not Draft-view keyboard input. Move/resize used native shape position properties and aspect-ratio locking, not a Shift mouse drag. The first move/resize attempt stopped on a PowerShell type-cast error before writing after.docx; its error is preserved in [task-a-move-resize-first-attempt.json](_readouts/task-a-move-resize-first-attempt.json). The completed after.docx was subsequently created from unchanged before.docx.

The four revision cases used TrackMoves=true and TrackFormatting=true. Revision author is `作者甲`, except tracked-two-authors also uses `作者乙`. The four before/after pairs have TrackRevisions=false. All saved documents have compatibility mode 15. PDF exports contain final-view text, not revision balloons; revision display statements below are root-agent GUI observations. OOXML checks and independent PDF observations are separate evidence.

Slash-separated body paragraphs below omit empty trailing paragraphs. Table cell text is reported separately; in the merged first cell, A1 and B1 are separate stacked paragraphs even though the compact XML text summary is A1B1.

## run-edits

Accept keeps the inserted sentence and removes the second original sentence. Only the three characters `第三句` remain bold red. Reject restores all three original sentences and their original bold/color state.

| case | 文件 | 做了什么（逐条） | Word 里看到什么（含修订标记的显示） | 自检结果 |
| --- | --- | --- | --- | --- |
| run-edits | [base.docx](revfix/run-edits/base.docx) | 1. Created required baseline with TrackRevisions=false; saved once. | Word page shows before, the three original Chinese sentences, and after. No revision markup visible.<br>[GUI screenshot](screenshots/a-run-edits-base-0.jpg) | Word revision items=0; OOXML revision elements=0; sections=1; compat=15; ZIP/XML/hash valid; [PDF 1 page(s), independently reviewed](_previews/revfix/run-edits/base.pdf) |
| run-edits | [tracked.docx](revfix/run-edits/tracked.docx) | 1. Opened unchanged base.docx independently.<br>2. Inserted requested sentence after first sentence.<br>3. Deleted second original sentence.<br>4. Set three characters of third sentence bold and red. | Word shows added sentence with red underline, second original sentence struck through, and the three characters of third sentence bold red. A formatting balloon names author A and bold/red changes.<br>[GUI screenshot](screenshots/a-run-edits-tracked-0.jpg) | Word revision items=3; OOXML revision elements=3; sections=1; compat=15; ZIP/XML/hash valid; [PDF 1 page(s), independently reviewed](_previews/revfix/run-edits/tracked.pdf)<br>ins=1, del=1, rPrChange=1 |
| run-edits | [accepted.docx](revfix/run-edits/accepted.docx) | 1. Opened unchanged tracked.docx independently.<br>2. Word Document.AcceptAllRevisions; not Undo-based. | Word shows first original sentence, added sentence, and third original sentence; the three characters of third sentence remain bold red. No revision marks visible. This accepted file was reopened read-only for the screenshot after a window-activation interruption; never saved again.<br>[GUI screenshot](screenshots/a-run-edits-accepted-0.jpg) | Word revision items=0; OOXML revision elements=0; sections=1; compat=15; ZIP/XML/hash valid; [PDF 1 page(s), independently reviewed](_previews/revfix/run-edits/accepted.pdf) |
| run-edits | [rejected.docx](revfix/run-edits/rejected.docx) | 1. Opened unchanged tracked.docx independently.<br>2. Word Document.RejectAllRevisions from the same tracked file; not Undo-based. | Word shows the original three sentences in ordinary black text, matching base, with no revision markup.<br>[GUI screenshot](screenshots/a-run-edits-rejected-0.jpg) | Word revision items=0; OOXML revision elements=0; sections=1; compat=15; ZIP/XML/hash valid; [PDF 1 page(s), independently reviewed](_previews/revfix/run-edits/rejected.pdf) |

Case structural checks: 19/19.

**accepted final body:** `before 前文` / `第一句原文。新增的一句。第三句原文。` / `after 后文`.

**rejected final body:** `before 前文` / `第一句原文。第二句原文。第三句原文。` / `after 后文`.

## para-split-merge

The split is after `甲段`. The second fragment `落的文字。` is centered with `w:firstLineChars="200"` (two characters). Accept merges `乙段落的文字。` directly with `after 后文`; reject restores the original paragraph boundaries and formatting.

| case | 文件 | 做了什么（逐条） | Word 里看到什么（含修订标记的显示） | 自检结果 |
| --- | --- | --- | --- | --- |
| para-split-merge | [base.docx](revfix/para-split-merge/base.docx) | 1. Created required baseline with TrackRevisions=false; saved once. | Word shows separate before, paragraph A, paragraph B, and after lines.<br>[GUI screenshot](screenshots/a-para-split-merge-base-0.jpg) | Word revision items=0; OOXML revision elements=0; sections=1; compat=15; ZIP/XML/hash valid; [PDF 1 page(s), independently reviewed](_previews/revfix/para-split-merge/base.pdf) |
| para-split-merge | [tracked.docx](revfix/para-split-merge/tracked.docx) | 1. Opened unchanged base.docx independently.<br>2. Inserted paragraph break after first two characters of paragraph A.<br>3. Deleted paragraph mark after paragraph B, merging it with after paragraph.<br>4. Centered second split paragraph with 2-character first-line indent. | Paragraph A is split after its first two characters. The remainder appears centered. Word formatting balloon explicitly says centered and first-line indent 2 characters. Paragraph B and after appear on separate lines in the markup view; the paragraph-mark deletion is checked in the DOCX.<br>[GUI screenshot](screenshots/a-para-split-merge-tracked-0.jpg) | Word revision items=3; OOXML revision elements=3; sections=1; compat=15; ZIP/XML/hash valid; [PDF 1 page(s), independently reviewed](_previews/revfix/para-split-merge/tracked.pdf)<br>ins=1, del=1, pPrChange=1 |
| para-split-merge | [accepted.docx](revfix/para-split-merge/accepted.docx) | 1. Opened unchanged tracked.docx independently.<br>2. Word Document.AcceptAllRevisions; not Undo-based. | Word retains the first two characters on one line and the centered remainder on a separate line. Paragraph B is followed immediately by after on the same line. No revision markup.<br>[GUI screenshot](screenshots/a-para-split-merge-accepted-0.jpg) | Word revision items=0; OOXML revision elements=0; sections=1; compat=15; ZIP/XML/hash valid; [PDF 1 page(s), independently reviewed](_previews/revfix/para-split-merge/accepted.pdf) |
| para-split-merge | [rejected.docx](revfix/para-split-merge/rejected.docx) | 1. Opened unchanged tracked.docx independently.<br>2. Word Document.RejectAllRevisions from the same tracked file; not Undo-based. | Word restores full paragraph A and paragraph B on separate left-aligned lines, with after on its own line. No revision markup.<br>[GUI screenshot](screenshots/a-para-split-merge-rejected-0.jpg) | Word revision items=0; OOXML revision elements=0; sections=1; compat=15; ZIP/XML/hash valid; [PDF 1 page(s), independently reviewed](_previews/revfix/para-split-merge/rejected.pdf) |

Case structural checks: 19/19.

**accepted final body:** `before 前文` / `甲段` / `落的文字。` / `乙段落的文字。after 后文`.

**rejected final body:** `before 前文` / `甲段落的文字。` / `乙段落的文字。` / `after 后文`.

## table-and-move

The move instruction literally targets the paragraph's existing position. Word nevertheless saved real moveFrom/moveTo markup, with paired ranges, so the requested movement structure is present. Native rejection restores A3/B3 and removes Inserted A/Inserted B but leaves the first row merged. This is Word's actual reference outcome, not a fabricated restoration to base. The inserted row was labeled Inserted A / Inserted B for readability.

| case | 文件 | 做了什么（逐条） | Word 里看到什么（含修订标记的显示） | 自检结果 |
| --- | --- | --- | --- | --- |
| table-and-move | [base.docx](revfix/table-and-move/base.docx) | 1. Created required baseline with TrackRevisions=false; saved once. | Word shows a 3-row 2-column table A1/B1 through A3/B3. Moving paragraph is already immediately before after.<br>[GUI screenshot](screenshots/a-table-and-move-base-0.jpg) | Word revision items=0; OOXML revision elements=0; sections=1; compat=15; ZIP/XML/hash valid; [PDF 1 page(s), independently reviewed](_previews/revfix/table-and-move/base.pdf) |
| table-and-move | [tracked.docx](revfix/table-and-move/tracked.docx) | 1. Opened unchanged base.docx independently.<br>2. Inserted row before original row 2.<br>3. Deleted original final row.<br>4. Merged original first row cells.<br>5. TrackMoves=true; cut whole moving paragraph and pasted immediately before after, its literal original location. Package check determines whether Word recorded a move. | Merged first row retains A1 and B1 on two lines. Inserted row is blue shaded with underlined text; A3/B3 row is pink shaded and struck through. Below table, moving paragraph appears once green double-struck and once green double-underlined before after.<br>[GUI screenshot](screenshots/a-table-and-move-tracked-0.jpg) | Word revision items=5; OOXML revision elements=25; sections=1; compat=15; ZIP/XML/hash valid; [PDF 1 page(s), independently reviewed](_previews/revfix/table-and-move/tracked.pdf)<br>ins=5, del=5, moveFrom=2, moveTo=2, moveFromRangeStart=1, moveFromRangeEnd=1, moveToRangeStart=1, moveToRangeEnd=1, tblGridChange=1, tcPrChange=6 |
| table-and-move | [accepted.docx](revfix/table-and-move/accepted.docx) | 1. Opened unchanged tracked.docx independently.<br>2. Word Document.AcceptAllRevisions; not Undo-based. | Merged first row remains; inserted row and original A2/B2 row are visible. A3/B3 absent. One ordinary moving paragraph appears before after. No revision markup.<br>[GUI screenshot](screenshots/a-table-and-move-accepted-0.jpg) | Word revision items=0; OOXML revision elements=0; sections=1; compat=15; ZIP/XML/hash valid; [PDF 1 page(s), independently reviewed](_previews/revfix/table-and-move/accepted.pdf) |
| table-and-move | [rejected.docx](revfix/table-and-move/rejected.docx) | 1. Opened unchanged tracked.docx independently.<br>2. Word Document.RejectAllRevisions from the same tracked file; not Undo-based. | Inserted row is gone; A2/B2 and A3/B3 are present. First row still remains merged, with A1/B1 stacked. One moving paragraph remains before after; no revision marks. This is Word's actual rejection result, not identical to base table geometry.<br>[GUI screenshot](screenshots/a-table-and-move-rejected-0.jpg) | Word revision items=0; OOXML revision elements=0; sections=1; compat=15; ZIP/XML/hash valid; [PDF 1 page(s), independently reviewed](_previews/revfix/table-and-move/rejected.pdf) |

Case structural checks: 31/31.

**accepted final body:** `before 前文` / `可移动段落。` / `after 后文`.
Table: `row 1: [A1B1; span=2]; row 2: [Inserted A; span=1] | [Inserted B; span=1]; row 3: [A2; span=1] | [B2; span=1]`.

**rejected final body:** `before 前文` / `可移动段落。` / `after 后文`.
Table: `row 1: [A1B1; span=2]; row 2: [A2; span=1] | [B2; span=1]; row 3: [A3; span=1] | [B3; span=1]`.

## tracked-two-authors

作者甲 inserts `作者甲插入的句子。` after the original sentence. 作者乙 inserts `作者乙插入的句子。` before it and deletes `正文。`, leaving `原始`. Both author names are present in tracked revision metadata.

| case | 文件 | 做了什么（逐条） | Word 里看到什么（含修订标记的显示） | 自检结果 |
| --- | --- | --- | --- | --- |
| tracked-two-authors | [base.docx](revfix/tracked-two-authors/base.docx) | 1. Created required baseline with TrackRevisions=false; saved once. | Word shows the original body sentence between before and after, no revision markup.<br>[GUI screenshot](screenshots/a-tracked-two-authors-base-0.jpg) | Word revision items=0; OOXML revision elements=0; sections=1; compat=15; ZIP/XML/hash valid; [PDF 1 page(s), independently reviewed](_previews/revfix/tracked-two-authors/base.pdf) |
| tracked-two-authors | [tracked.docx](revfix/tracked-two-authors/tracked.docx) | 1. Opened unchanged base.docx independently.<br>2. Author A inserted a sentence.<br>3. Author B inserted a separate sentence before original text and deleted its latter part (body text and punctuation). | Word shows author B inserted sentence underlined blue before original text; the latter part of original text is struck through blue. Author A inserted sentence is underlined red after original text.<br>[GUI screenshot](screenshots/a-tracked-two-authors-tracked-0.jpg) | Word revision items=3; OOXML revision elements=3; sections=1; compat=15; ZIP/XML/hash valid; [PDF 1 page(s), independently reviewed](_previews/revfix/tracked-two-authors/tracked.pdf)<br>ins=2, del=1 |
| tracked-two-authors | [accepted.docx](revfix/tracked-two-authors/accepted.docx) | 1. Opened unchanged tracked.docx independently.<br>2. Word Document.AcceptAllRevisions; not Undo-based. | Word shows author B inserted sentence, the remaining first half of original text, then author A inserted sentence, all ordinary black text without markup.<br>[GUI screenshot](screenshots/a-tracked-two-authors-accepted-0.jpg) | Word revision items=0; OOXML revision elements=0; sections=1; compat=15; ZIP/XML/hash valid; [PDF 1 page(s), independently reviewed](_previews/revfix/tracked-two-authors/accepted.pdf) |
| tracked-two-authors | [rejected.docx](revfix/tracked-two-authors/rejected.docx) | 1. Opened unchanged tracked.docx independently.<br>2. Word Document.RejectAllRevisions from the same tracked file; not Undo-based. | Word restores the original body sentence alone between before and after, no revision markup.<br>[GUI screenshot](screenshots/a-tracked-two-authors-rejected-0.jpg) | Word revision items=0; OOXML revision elements=0; sections=1; compat=15; ZIP/XML/hash valid; [PDF 1 page(s), independently reviewed](_previews/revfix/tracked-two-authors/rejected.pdf) |

Case structural checks: 17/17.

**accepted final body:** `before 前文` / `作者乙插入的句子。原始作者甲插入的句子。` / `after 后文`.

**rejected final body:** `before 前文` / `原始正文。` / `after 后文`.

## sect-insert

The next-page section break precedes `第二节正文。`. The saved section count changes from one to two; page 1 is portrait and page 2 landscape. Both PDF pages were independently viewed.

| case | 文件 | 做了什么（逐条） | Word 里看到什么（含修订标记的显示） | 自检结果 |
| --- | --- | --- | --- | --- |
| sect-insert | [before.docx](revfix/sect-insert/before.docx) | 1. Created required baseline with TrackRevisions=false; saved once. | Word shows both section-labelled body paragraphs together on one portrait page with before and after.<br>[GUI screenshot](screenshots/a-sect-insert-before-0.jpg) | Word revision items=0; OOXML revision elements=0; sections=1; compat=15; ZIP/XML/hash valid; [PDF 1 page(s), independently reviewed](_previews/revfix/sect-insert/before.pdf) |
| sect-insert | [after.docx](revfix/sect-insert/after.docx) | 1. Opened unchanged before.docx independently.<br>2. Inserted next-page section break before second section text; made new second section landscape. | Word multi-page view shows first page portrait with before and first-section body; second page landscape with second-section body and after.<br>[GUI screenshot](screenshots/a-sect-insert-after-0.jpg) | Word revision items=0; OOXML revision elements=0; sections=2; compat=15; ZIP/XML/hash valid; [PDF 2 page(s), independently reviewed](_previews/revfix/sect-insert/after.pdf) |

Case structural checks: 9/9.

before: section 1: portrait, nextPage, 11906 x 16838 twips.

after: section 1: portrait, nextPage, 11906 x 16838 twips; section 2: landscape, nextPage, 16838 x 11906 twips.

## sect-delete

The original next-page break between portrait and landscape sections is removed. Word leaves one landscape section; all body paragraphs now appear on one landscape page. The later section's page setup is inherited.

| case | 文件 | 做了什么（逐条） | Word 里看到什么（含修订标记的显示） | 自检结果 |
| --- | --- | --- | --- | --- |
| sect-delete | [before.docx](revfix/sect-delete/before.docx) | 1. Created required baseline with TrackRevisions=false; saved once. | Word multi-page view shows portrait page 1 and landscape page 2, with separate section body text.<br>[GUI screenshot](screenshots/a-sect-delete-before-0.jpg) | Word revision items=0; OOXML revision elements=0; sections=2; compat=15; ZIP/XML/hash valid; [PDF 2 page(s), independently reviewed](_previews/revfix/sect-delete/before.pdf) |
| sect-delete | [after.docx](revfix/sect-delete/after.docx) | 1. Opened unchanged before.docx independently.<br>2. Deleted the section-break character; Word determines inherited page setup. | Word shows both section body paragraphs together on a single landscape page; before and after retained. Word inherited the later section's landscape setup.<br>[GUI screenshot](screenshots/a-sect-delete-after-0.jpg) | Word revision items=0; OOXML revision elements=0; sections=1; compat=15; ZIP/XML/hash valid; [PDF 1 page(s), independently reviewed](_previews/revfix/sect-delete/after.pdf) |

Case structural checks: 8/8.

before: section 1: portrait, nextPage, 11906 x 16838 twips; section 2: landscape, nextPage, 16838 x 11906 twips.

after: section 1: landscape, nextPage, 16838 x 11906 twips.

## z-order

The same bitmap occurs in three floating square-wrapped anchors with unique docPr IDs 1, 2, 3. Stored back-to-front order changes from Picture 1 < 2 < 3 to Picture 2 < 3 < 1. Picture content, position, dimensions, and relative coordinate bases remain unchanged; visible occlusion agrees.

| case | 文件 | 做了什么（逐条） | Word 里看到什么（含修订标记的显示） | 自检结果 |
| --- | --- | --- | --- | --- |
| z-order | [before.docx](revfix/z-order/before.docx) | 1. Created required baseline with TrackRevisions=false; saved once. | Three teal/yellow pictures step diagonally down-right. Picture 2 covers picture 1 at their overlap; picture 3 covers picture 2. before/after remain readable.<br>[GUI screenshot](screenshots/a-z-order-before-0.jpg) | Word revision items=0; OOXML revision elements=0; sections=1; compat=15; ZIP/XML/hash valid; [PDF 1 page(s), independently reviewed](_previews/revfix/z-order/before.pdf) |
| z-order | [after.docx](revfix/z-order/after.docx) | 1. Opened unchanged before.docx independently.<br>2. Brought lowest floating picture to front: Round3 Picture 1 | Upper-left picture 1 is now fully visible and covers picture 2. Picture 3 still covers picture 2; all three picture positions are unchanged and before/after readable.<br>[GUI screenshot](screenshots/a-z-order-after-0.jpg) | Word revision items=0; OOXML revision elements=0; sections=1; compat=15; ZIP/XML/hash valid; [PDF 1 page(s), independently reviewed](_previews/revfix/z-order/after.pdf) |

Case structural checks: 12/12.

| file / picture | docPr id | relativeHeight | x / y EMU | width / height EMU |
| --- | --- | --- | --- | --- |
| before / Round3 Picture 1 | 1 | 251658240 | 2512 / 321547 | 1524000 / 762000 |
| before / Round3 Picture 2 | 2 | 251659264 | 484833 / 635893 | 1524000 / 762000 |
| before / Round3 Picture 3 | 3 | 251660288 | 967154 / 957440 | 1524000 / 762000 |
| after / Round3 Picture 1 | 1 | 251661312 | 2512 / 321547 | 1524000 / 762000 |
| after / Round3 Picture 2 | 2 | 251659264 | 484833 / 635893 | 1524000 / 762000 |
| after / Round3 Picture 3 | 3 | 251660288 | 967154 / 957440 | 1524000 / 762000 |

## move-resize

One square-wrapped floating picture is moved approximately 2 cm right/down and halved in both dimensions. The request was applied through Word COM with aspect ratio locked. Its saved OOXML, rather than an unrounded requested value, is the comparison reference. The before GUI screenshot has a Start panel in the lower-left margin outside the inspected body; the PDF is unobstructed.

| case | 文件 | 做了什么（逐条） | Word 里看到什么（含修订标记的显示） | 自检结果 |
| --- | --- | --- | --- | --- |
| move-resize | [before.docx](revfix/move-resize/before.docx) | 1. Created required baseline with TrackRevisions=false; saved once. | Word shows one wide teal/yellow picture under the anchor text, with before above and after below. A Windows Start panel is present in the lower-left screenshot margin, outside the inspected picture and body text.<br>[GUI screenshot](screenshots/a-move-resize-before-0.jpg) | Word revision items=0; OOXML revision elements=0; sections=1; compat=15; ZIP/XML/hash valid; [PDF 1 page(s), independently reviewed](_previews/revfix/move-resize/before.pdf) |
| move-resize | [after.docx](revfix/move-resize/after.docx) | 1. Opened unchanged before.docx independently.<br>2. Moved native floating picture 2cm right/down and halved width with aspect ratio locked through Word shape properties; precise COM equivalent, not a mouse drag. | Word shows the same picture moved right and down, with visibly half width and height. before, anchor text and after remain unchanged and clear. Exact geometry is separately recorded by Word and OOXML.<br>[GUI screenshot](screenshots/a-move-resize-after-0.jpg) | Word revision items=0; OOXML revision elements=0; sections=1; compat=15; ZIP/XML/hash valid; [PDF 1 page(s), independently reviewed](_previews/revfix/move-resize/after.pdf) |

Case structural checks: 10/10.

| file / picture | docPr id | relativeHeight | x / y EMU | width / height EMU |
| --- | --- | --- | --- | --- |
| before / Round3 Picture 1 | 1 | 251658240 | 2512 / 314346 | 1524000 / 762000 |
| after / Round3 Picture 1 | 1 | 251658240 | 721360 / 1033780 | 762000 / 381000 |

Saved offsets changed by 718848 EMU horizontally (1.996800 cm) and 719434 EMU vertically (1.998428 cm). Width/height changed from 120/60 pt to 60/30 pt, exactly half.

## Evidence index

- [Joined final summary](_scripts/task-a-final-summary.json): immutable authoring readouts, UI evidence hashes, independent inspections and same-tracked branching checks.
- [Independent structural/PDF report](_readouts/revfix-inspection.json): 24 file hashes and 125 per-requirement checks.
- [Direct Word GUI observations](_scripts/task-a-ui.json): 24 root-agent observations with screenshots.
- [Independent PDF visual annotations](_scripts/revfix-pdf-visual.json): 24 hash-bound reviews covering all 26 PDF pages.
- `_readouts/a-<case>-<stage>.json`: original native Word readouts; saved without rewriting their authoring metadata.
- `_previews/revfix/<case>/<stage>.pdf`: Word export after the only save at the output path.
