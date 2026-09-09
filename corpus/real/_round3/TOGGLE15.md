# Compatibility15 Toggle Readings

Word / Office LTSC Professional Plus 2021 x64. WINWORD.EXE version: 16.0.14334.20848. Exact object-model version/build is retained in each raw readout and environment.json.

Method: the same25 round2 points, with two real Word object-model readings per point. Font properties use a caret inside each sentence; Hidden uses the full sentence with hidden display disabled. The header point navigates to physical page2 and selects the inherited default header. GUI observations below are attached only when recorded evidence exists. No fixture is saved by the reading helper.

The old eight fixtures are byte-identical to round2. Each supplied compatibility15 fixture adds word/settings.xml plus its required relationship/content-type registrations; document.xml and styles.xml bytes remain unchanged (round3-input-audit.json). Round2 toggle-para-and-char lacks a recorded numeric mode value, although its titlebar showed compatibility mode; this historical gap is retained.

Earlier collector attempts are preserved in _scripts/toggle15-failed-attempts. Direct Content.Text offset mapping failed on Word table cell terminators, and substring-based locating confused strike/dstrike and caps/smallcaps. The corrected collector uses native Range.Find, case-insensitive text validation and the preserved source-range reference. The table below uses the latest readouts; unresolved reads remain explicit. The archived failures were helper-location errors, not Word-open failures.

| 文件 | 句子 | 属性 | 兼容模式 15 下开/关 | 与第二轮（模式 12）是否一致 |
| --- | --- | --- | --- | --- |
| toggle-other-toggles-compat15.docx | i twice | Italic | 关; 0 / 0; [page](screenshots/c-toggle-other-toggles-compat15-0.jpg) | 一致 |
| toggle-other-toggles-compat15.docx | i once | Italic | 开; -1 / -1; [page](screenshots/c-toggle-other-toggles-compat15-0.jpg) | 一致 |
| toggle-other-toggles-compat15.docx | strike twice | StrikeThrough | 关; 0 / 0; [page](screenshots/c-toggle-other-toggles-compat15-0.jpg), [font-dialog](screenshots/c-strike-twice-font-0.jpg) | 一致 |
| toggle-other-toggles-compat15.docx | strike once | StrikeThrough | 开; -1 / -1; [page](screenshots/c-toggle-other-toggles-compat15-0.jpg) | 一致 |
| toggle-other-toggles-compat15.docx | caps twice | AllCaps | 关; 0 / 0; [page](screenshots/c-toggle-other-toggles-compat15-0.jpg) | 一致 |
| toggle-other-toggles-compat15.docx | caps once | AllCaps | 开; -1 / -1; [page](screenshots/c-toggle-other-toggles-compat15-0.jpg) | 一致 |
| toggle-other-toggles-compat15.docx | smallcaps twice | SmallCaps | 关; 0 / 0; [page](screenshots/c-toggle-other-toggles-compat15-0.jpg) | 一致 |
| toggle-other-toggles-compat15.docx | smallcaps once | SmallCaps | 开; -1 / -1; [page](screenshots/c-toggle-other-toggles-compat15-0.jpg) | 一致 |
| toggle-other-toggles-compat15.docx | dstrike twice | DoubleStrikeThrough | 开; -1 / -1; [page](screenshots/c-toggle-other-toggles-compat15-0.jpg), [font-dialog](screenshots/c-dstrike-twice-font-0.jpg) | 一致 |
| toggle-other-toggles-compat15.docx | dstrike once | DoubleStrikeThrough | 开; -1 / -1; [page](screenshots/c-toggle-other-toggles-compat15-0.jpg) | 一致 |
| toggle-other-toggles-compat15.docx | vanish twice | Hidden | 关; 0 / 0; [page](screenshots/c-toggle-other-toggles-compat15-0.jpg) | 一致 |
| toggle-other-toggles-compat15.docx | vanish once | Hidden | 开; -1 / -1; [page](screenshots/c-toggle-other-toggles-compat15-0.jpg), [font-dialog](screenshots/c-vanish-once-font-0.jpg) | 一致 |
| toggle-para-and-char-compat15.docx | para b + char b | Bold | 关; 0 / 0; [page](screenshots/c-toggle-para-and-char-compat15-0.jpg) | 一致 |
| toggle-para-and-char-compat15.docx | para b only | Bold | 开; -1 / -1; [page](screenshots/c-toggle-para-and-char-compat15-0.jpg) | 一致 |
| toggle-docdefaults-and-para-compat15.docx | docDefaults b + para b | Bold | 开; -1 / -1; [page](screenshots/c-toggle-docdefaults-and-para-compat15-0.jpg) | 一致 |
| toggle-docdefaults-and-para-compat15.docx | docDefaults b only | Bold | 开; -1 / -1; [page](screenshots/c-toggle-docdefaults-and-para-compat15-0.jpg) | 一致 |
| toggle-docdefaults-and-para-off-compat15.docx | docDefaults b + para b=0 | Bold | 关; 0 / 0; [page](screenshots/c-toggle-docdefaults-and-para-off-compat15-0.jpg) | 一致 |
| toggle-docdefaults-and-para-off-compat15.docx | docDefaults b only | Bold | 开; -1 / -1; [page](screenshots/c-toggle-docdefaults-and-para-off-compat15-0.jpg) | 一致 |
| toggle-based-on-two-levels-compat15.docx | basedOn b + derived b | Bold | 开; -1 / -1; [page](screenshots/c-toggle-based-on-two-levels-compat15-0.jpg) | 一致 |
| toggle-based-on-two-levels-compat15.docx | base b only | Bold | 开; -1 / -1; [page](screenshots/c-toggle-based-on-two-levels-compat15-0.jpg) | 一致 |
| toggle-table-first-row-compat15.docx | table firstRow b + para b | Bold | 关; 0 / 0; [page](screenshots/c-toggle-table-first-row-compat15-0.jpg) | 一致 |
| toggle-table-first-row-compat15.docx | table body + para b | Bold | 开; -1 / -1; [page](screenshots/c-toggle-table-first-row-compat15-0.jpg) | 一致 |
| toggle-direct-off-compat15.docx | direct b=0 over style b | Bold | 关; 0 / 0; [page](screenshots/c-toggle-direct-off-compat15-0.jpg) | 一致 |
| toggle-direct-off-compat15.docx | style b, no direct | Bold | 开; -1 / -1; [page](screenshots/c-toggle-direct-off-compat15-0.jpg) | 一致 |
| sections-inherit-default-compat15.docx | 第二页页眉 | HeaderDefault | 第一节页眉; LinkToPrevious=True; 第一节页眉  / 第一节页眉 ; [page](screenshots/c-sections-inherit-default-compat15-0.jpg) | 一致 |

## Word Conversion Cross-Check

The conversion operation and SaveAs2 are performed separately in real Word. The reading helper never converts or saves. The converted file and raw reading retain its SHA256 and actual compatibility mode.

[Converted DOCX](_resaved/toggle-other-toggles-converted.docx); [conversion operation record](_readouts/task-c-conversion.json).

Actual method: Byte-identical working copy; actual Word UI File > Info > Convert > OK; Word COM SaveAs2 to separate output.

Source mode: 12; Word-converted mode: 15. The working copy SHA256 before conversion equals the preserved original: True.

[UI](screenshots/c-convert-mode12-before-0.jpg), [UI](screenshots/c-convert-info-ready-0.jpg), [UI](screenshots/c-convert-action-0.jpg), [UI](screenshots/c-convert-completed-0.jpg)

| Sentence | Property | Round2 mode12 | Supplied mode15 | Word-converted | All three agree |
| --- | --- | --- | --- | --- | --- |
| i twice | Italic | 关 | 关 | 关 | True |
| i once | Italic | 开 | 开 | 开 | True |
| strike twice | StrikeThrough | 关 | 关 | 关 | True |
| strike once | StrikeThrough | 开 | 开 | 开 | True |
| caps twice | AllCaps | 关 | 关 | 关 | True |
| caps once | AllCaps | 开 | 开 | 开 | True |
| smallcaps twice | SmallCaps | 关 | 关 | 关 | True |
| smallcaps once | SmallCaps | 开 | 开 | 开 | True |
| dstrike twice | DoubleStrikeThrough | 开 | 开 | 开 | True |
| dstrike once | DoubleStrikeThrough | 开 | 开 | 开 | True |
| vanish twice | Hidden | 关 | 关 | 关 | True |
| vanish once | Hidden | 开 | 开 | 开 | True |

Primary: 25/25 same as round2; 0 different. Conversion cross-check: 12/12 three-way agreement.

## Actual GUI Observations

- sections-inherit-default-compat15.docx: Page 2 header displays first-section header text, section 2 header label and same-as-previous label visible.
- toggle-based-on-two-levels-compat15.docx: Both lines are bold; Bold ribbon button is pressed at second line. Translation suggestion shown away from content, not activated.
- toggle-direct-off-compat15.docx: First line normal weight, second line bold and Bold button pressed. Translation suggestion away from content, not activated.
- toggle-docdefaults-and-para-compat15.docx: Both lines are bold, with Bold ribbon button pressed. Translation suggestion does not cover text.
- toggle-docdefaults-and-para-off-compat15.docx: First line normal weight; second line bold and Bold ribbon button pressed.
- toggle-para-and-char-compat15.docx: Paragraph plus character bold line is normal weight; paragraph-only line is bold, with Bold button pressed.
- toggle-other-toggles-compat15.docx: i twice upright and i once italic; strike twice without line and strike once struck; caps twice lowercase and CAPS ONCE uppercase; smallcaps twice normal lowercase and SMALLCAPS ONCE small capitals; both dstrike phrases double-struck; vanish twice visible and vanish once absent with hidden display disabled.
- toggle-other-toggles-compat15.docx / strike twice: Native Font dialog: single strike checkbox unchecked, double strike unchecked; preview has no strike line.
- toggle-other-toggles-compat15.docx / dstrike twice: Native Font dialog: double-strike checkbox checked; single strike unchecked; preview double-struck.
- toggle-other-toggles-compat15.docx / vanish once: Native Font dialog: Hidden checked for entire sentence range126..137; text absent from page with hidden display disabled.
- toggle-table-first-row-compat15.docx: First table row text normal weight; second table row bold, with Bold ribbon button pressed.
- toggle-other-toggles-converted.docx: Converted document page matches supplied mode15: italic only once; single strike only once; uppercase and smallcaps only once; both dstrike phrases double-struck; only vanish twice visible.

## Incomplete / Uncertain

No outstanding TaskC measurement or evidence items. The historical missing numeric mode for toggle-para-and-char remains documented above.
