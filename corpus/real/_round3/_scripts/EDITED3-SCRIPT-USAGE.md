# Round 3 Task B Script Usage

These scripts were prepared without starting or controlling Word. Static
PowerShell parsing and all 180 baseline path resolutions passed. Word execution
and actual visual conclusions belong to the root operator.

The input manifest contains 1544 generated files across 180 source DOCX files:
110 original sources, 53 Word resaves from round 2, and 17 M7 sources. The task's
127-source count omits the 53 resaves. There are 12 other manifest records, but
no corresponding generated DOCX: 11 unsupported ChartEx chartdata edits and one
fields-toc-stale deleteblock invariant failure. These are engine generation
statuses, not Word failures.

`edited3-plan.json` records source paths/hashes and 60 unique UI files, with no
duplicates: 13 mandatory, 36 regular, and 11 additional M7 files. There is no
M7-source chartdata derivative in the input, so the twelfth requested M7 sample
is unavailable. The regular set keeps 34 round-2 samples; two were replaced to
keep mandatory cases additional. All 12 operations have three regular samples.

## Batch

Use the existing task-owned Word worker after Task A. No script creates a new
Word application, quits Word/Excel, or changes any input/source DOCX.

```powershell
. 'C:/word/real-word-round3-20260907/_scripts/batch-edited3.ps1'
$plan = Get-Content 'C:/word/real-word-round3-20260907/_scripts/edited3-plan.json' -Raw | ConvertFrom-Json
$mandatory = @($plan.selected | Where-Object selection_group -eq 'mandatory' | ForEach-Object file)
Invoke-Edited3Batch -Word $word -OnlyFiles $mandatory
# After Task C, continue all remaining files:
Invoke-Edited3Batch -Word $word
# A genuine retry preserves the prior attempt and appends a fresh JSONL record:
Invoke-Edited3Batch -Word $word -OnlyFiles @('example--insert.docx') -RetryFiles @('example--insert.docx')
```

The batch resumes from `edited3-results.jsonl`. It does not turn skipped files
into fabricated errors. `-Limit` caps newly processed files, and `-NoResume`
refuses to overwrite an existing log. The current stage is recorded in
`edited3-current-item.json` for observation of long COM calls. An external
watchdog may diagnose a hang, but the batch never kills applications.

Chart edits and source documents open visibly and activate before collecting
chart readings. Other edited documents open hidden. All opens are read-only
with alerts suppressed, and close without saving. Every chartdata record has
preactivation readings, activation/workbook errors if any, workbook values,
and postactivation readings. Newchart workbook activation is available in the
UI helper. Original ChartEx COM limitations remain separate recorded errors.

The incomplete gate also includes shape-geometry errors and baseline
read/close/metric errors. A successful open does not mean that later COM
collection or closing succeeded. Any per-file top-level `error` or
`close_error`, including a row with `open: ok`, needs a separate UI follow-up
and a genuine retry. Add those observations to the planned 60. For repeated
low-level diagnostics such as unsupported ChartEx getters, the operator must
review and document the shared limitation; do not silently convert it to a
pass. The report's fixed coverage check does not itself enforce this additional
diagnostic triage.

## Actual UI Evidence

```powershell
. 'C:/word/real-word-round3-20260907/_scripts/edited3-ui.ps1'
Open-Edited3UiCase -Word $word -File 'chart-no-title--chartdata.docx'
# Inspect the actual window and any prompt, then capture a screenshot.
Read-Edited3ActiveCase -Word $word -File 'chart-no-title--chartdata.docx' -ReadoutTag initial
# Capture the unactivated chart before requesting embedded data.
Read-Edited3ActiveCase -Word $word -File 'chart-no-title--chartdata.docx' -ActivateData -ReadoutTag activated
# Optional native resave and PDF export. Do this after all active-original readings.
Save-Edited3UiArtifacts -Word $word -File 'chart-no-title--chartdata.docx' -StateDescription 'After embedded-data activation; report the observed state.'
$word.ActiveDocument.Close(0)
```

`Open-Edited3UiCase` uses DisplayAlerts=-1 and returns only when the Word open
call completes; a modal recovery prompt can require GUI handling while the
worker job is pending. It never closes an existing document. `ReadoutTag` must
be unique, because readouts are never overwritten. `-FocusFeature` selects the
relevant header, comment, or table for screenshot inspection. Readouts alone
are not visual observations.

The root operator maintains `ui-edited3.json`, an array of actual observations:

```json
[
  {
    "file": "chart-no-title--chartdata.docx",
    "recovery": "none",
    "observation": "Replace this with the actual directly observed result.",
    "consistent": null,
    "screenshot": ["screenshots/example.jpg"],
    "pdf": "_previews/edited3/example.pdf",
    "resaved": "_resaved/example.docx"
  }
]
```

Use `null` for unknown consistency, and use the exact displayed prompt text
when a prompt appears. Never infer prompt absence from a suppressed-alert
batch open. UI helper readouts go to `_readouts`; resaves/PDFs are optional
artifacts distinct from the required actual UI screenshots.

## Final Reports

```powershell
. 'C:/word/real-word-round3-20260907/_scripts/edited3-report.ps1'
Update-Edited3Report
```

The batch also rebuilds partial reports every 25 files. The final call requires
1544 unique COM rows, all 60 planned UI records, the 13 mandatory conclusions,
and existing paths for all claimed UI artifacts. It writes `EDITED3.md`,
`edited3-results.json`, and `_scripts/edited3-summary.json`. Raw JSONL is never
rewritten by the merger. It preserves unknown/incomplete statuses and records
comparison to the identically named round-2 result. A measured-check pass does
not imply that all other content and rendering were verified.

`edited3-report.ps1` contains the required Chinese column labels and uses a
UTF-8 BOM so Windows PowerShell 5.1 decodes it correctly.
JSON arrays are explicitly unwrapped after `ConvertFrom-Json`, including UI
manifest lookup. This was verified by rebuilding the report from the first
13 actual Word COM records in `powershell.exe` 5.1, without opening Word again.
