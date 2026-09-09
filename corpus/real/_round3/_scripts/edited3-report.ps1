# Rebuild report from current COM results and separately observed UI evidence.
# JSONL remains the append-only record of actual Word attempts.
function Update-Edited3Report {
    [CmdletBinding()]
    param(
        [string] $OutputDirectory = 'C:/word/real-word-round3-20260907',
        [string] $InputDirectory = 'C:/word/round3-work-20260907/real-word-round3-inputs/edited',
        [string] $Round2ResultsPath = 'C:/word/real-word-round2-20260907/edited-results.json',
        [switch] $AllowPartial
    )
    $ErrorActionPreference = 'Stop'
    $utf8 = New-Object Text.UTF8Encoding($false)
    $resultsPath = Join-Path $OutputDirectory 'edited3-results.json'
    $uiPath = Join-Path $OutputDirectory 'ui-edited3.json'
    $allManifest = @((Get-Content -LiteralPath (Join-Path $InputDirectory 'manifest.json') -Raw -Encoding UTF8 | ConvertFrom-Json) | ForEach-Object { $_ })
    $manifest = @($allManifest | Where-Object status -eq 'generated')
    if ($manifest.Count -ne 1544) { throw "Expected 1544 generated entries; got $($manifest.Count)." }
    $rows = @((Get-Content -LiteralPath $resultsPath -Raw -Encoding UTF8 | ConvertFrom-Json) | ForEach-Object { $_ })
    if (-not $AllowPartial -and $rows.Count -ne 1544) { throw "Incomplete COM pass: $($rows.Count)/1544." }
    $manifestByFile = @{}
    foreach ($item in $manifest) {
        if ($manifestByFile.ContainsKey([string]$item.file)) { throw "Duplicate manifest filename: $($item.file)" }
        $manifestByFile[[string]$item.file] = $item
    }
    $rowByFile = @{}
    foreach ($row in $rows) {
        if ($rowByFile.ContainsKey([string]$row.file)) { throw "Duplicate COM filename: $($row.file)" }
        if (-not $manifestByFile.ContainsKey([string]$row.file)) { throw "Unexpected COM file: $($row.file)" }
        if ($row.base -ne $manifestByFile[$row.file].base -or $row.op -ne $manifestByFile[$row.file].op) { throw "Manifest mismatch: $($row.file)" }
        $rowByFile[[string]$row.file] = $row
    }
    $uiByFile = @{}
    if (Test-Path -LiteralPath $uiPath) {
        foreach ($ui in @((Get-Content -LiteralPath $uiPath -Raw -Encoding UTF8 | ConvertFrom-Json) | ForEach-Object { $_ })) {
            if ($uiByFile.ContainsKey([string]$ui.file)) { throw "Duplicate UI filename: $($ui.file)" }
            if (-not $manifestByFile.ContainsKey([string]$ui.file)) { throw "Unexpected UI file: $($ui.file)" }
            foreach ($key in @('recovery', 'observation', 'consistent')) {
                if ($null -eq $ui.PSObject.Properties[$key]) { throw "UI entry $($ui.file) requires '$key'; use null when unknown." }
            }
            if (-not $AllowPartial) {
                if ([string]::IsNullOrWhiteSpace([string]$ui.observation)) { throw "UI observation is blank: $($ui.file)" }
                if ([string]::IsNullOrWhiteSpace([string]$ui.recovery)) { throw "UI recovery conclusion is blank: $($ui.file)" }
                $screenshots = @($ui.screenshot | Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_) })
                if ($screenshots.Count -eq 0) { throw "No UI screenshot: $($ui.file)" }
                foreach ($key in @('screenshot', 'pdf', 'resaved')) {
                    foreach ($artifact in @($ui.$key)) {
                        if ([string]::IsNullOrWhiteSpace([string]$artifact)) { continue }
                        $path = if ([IO.Path]::IsPathRooted([string]$artifact)) { [string]$artifact } else { Join-Path $OutputDirectory ([string]$artifact) }
                        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing $key artifact for $($ui.file): $path" }
                    }
                }
            }
            $uiByFile[[string]$ui.file] = $ui
        }
    }
    $oldByFile = @{}
    foreach ($row in @((Get-Content -LiteralPath $Round2ResultsPath -Raw -Encoding UTF8 | ConvertFrom-Json) | ForEach-Object { $_ })) { $oldByFile[[string]$row.file] = $row }
    $mandatoryInk = @('ink-pen', 'ink-highlighter', 'ink-to-shape') | ForEach-Object { $base = $_; @('newimage', 'newchart', 'ink') | ForEach-Object { "$base--$_.docx" } }
    $mandatoryChart = @('chart-no-title--chartdata.docx', 'chart-scatter--chartdata.docx', 'chart-scatter-lines--chartdata.docx', 'chart-bubble--chartdata.docx')
    $mandatory = @($mandatoryInk) + $mandatoryChart
    $planPath = Join-Path $OutputDirectory '_scripts/edited3-plan.json'
    $plan = if (Test-Path -LiteralPath $planPath) { Get-Content -LiteralPath $planPath -Raw -Encoding UTF8 | ConvertFrom-Json } else { $null }

    function Set-E3Property($Row, [string] $Key, $Value) { $Row | Add-Member -MemberType NoteProperty -Name $Key -Value $Value -Force }
    function Get-E3SharedHash([string] $Path) {
        $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
        $hasher = [Security.Cryptography.SHA256]::Create()
        try { return [BitConverter]::ToString($hasher.ComputeHash($stream)).Replace('-', '') }
        finally { $hasher.Dispose(); $stream.Dispose() }
    }
    $inkAuditPath = Join-Path $OutputDirectory '_scripts/edited3-ink-mismatch-audit.json'
    $inkAuditByFile = @{}
    $inkAudit = $null
    $inkAuditHash = $null
    if (Test-Path -LiteralPath $inkAuditPath) {
        $inkAudit = Get-Content -LiteralPath $inkAuditPath -Raw -Encoding UTF8 | ConvertFrom-Json
        $inkAuditHash = Get-E3SharedHash $inkAuditPath
        foreach ($evidence in @($inkAudit.codeEvidence)) {
            if ((Get-E3SharedHash ([string]$evidence.path)) -ne [string]$evidence.sha256) { throw "Ink review code evidence changed: $($evidence.path)" }
        }
        foreach ($inkRow in @($inkAudit.rows)) {
            if ($inkAuditByFile.ContainsKey([string]$inkRow.file)) { throw "Duplicate ink review row: $($inkRow.file)" }
            if (-not $manifestByFile.ContainsKey([string]$inkRow.file)) { throw "Ink review file absent from generated manifest: $($inkRow.file)" }
            $inkAuditByFile[[string]$inkRow.file] = $inkRow
        }
    }
    function Get-E3IndependentInkReview($Row) {
        if (-not $inkAuditByFile.ContainsKey([string]$Row.file)) { return $null }
        $review = $inkAuditByFile[[string]$Row.file]
        $expectedInput = [IO.Path]::GetFullPath((Join-Path $InputDirectory ([string]$Row.file)))
        $expectedSource = [IO.Path]::GetFullPath([string]$Row.baseline.source_path)
        if (-not $expectedInput.Equals([IO.Path]::GetFullPath([string]$review.edited.path), [StringComparison]::OrdinalIgnoreCase) -or -not $expectedSource.Equals([IO.Path]::GetFullPath([string]$review.baseline.path), [StringComparison]::OrdinalIgnoreCase)) { throw "Ink review paths do not match the actual batch sources: $($Row.file)" }
        $editedHash = Get-E3SharedHash $expectedInput
        $baselineHash = Get-E3SharedHash $expectedSource
        if ($editedHash -ne [string]$review.edited.sha256 -or $baselineHash -ne [string]$review.baseline.sha256) { throw "Ink review hashes are stale: $($Row.file)" }
        if ($Row.op -ne 'ink' -or $review.manifestExpectation -ne $Row.expect -or $review.batchShapes.actual -ne $Row.metrics.shapes -or $review.batchShapes.baseline -ne $Row.baseline.metrics.shapes -or $review.batchShapes.naiveExpectedPlusOne -ne $Row.checks.shape_check.expected_shapes) { throw "Ink review no longer matches the recorded COM readings: $($Row.file)" }
        $diagnosis = $review.diagnosis
        foreach ($key in @('nativeWordCountAgreesWithReplacement', 'allNonOverlayDrawingSemanticTreesUnchanged', 'nativeInkPartsByteIdentical', 'otherFeatureCountsUnchanged', 'editedDocPrIdsUnique')) {
            if ($diagnosis.$key -ne $true) { throw "Ink independent review did not establish ${key}: $($Row.file)" }
        }
        if ($diagnosis.editedAidocsOverlays -ne 1 -or $diagnosis.expectedIfAuthoritativeOneOverlayList -ne $Row.metrics.shapes) { throw "Ink authoritative-list count disagrees with Word: $($Row.file)" }
        return [ordered]@{
            assessment = 'pass'
            scope = 'Independent XML and repository-source review of authoritative ink-list replacement; UI conclusion remains separate.'
            explanation = 'The manifest requires one front overlay. SaveOptions.inks replaces the editor overlay list, so the correct shape count is baseline minus existing aidocs-ink overlays plus one. The raw baseline-plus-one mismatch is a collector false positive and remains preserved.'
            audit_file = '_scripts/edited3-ink-mismatch-audit.json'
            audit_sha256 = $inkAuditHash
            input_sha256 = $editedHash
            baseline_sha256 = $baselineHash
            source_code_hashes_verified = $true
            original_object_model_assessment = $Row.object_model_assessment
            baseline_editor_overlays = $diagnosis.baselineAidocsOverlays
            expected_shapes_under_authoritative_list = $diagnosis.expectedIfAuthoritativeOneOverlayList
            actual_word_shapes = $Row.metrics.shapes
            other_drawings_and_native_ink_preserved = $true
        }
    }
    function ConvertTo-E3Cell($Value) {
        if ($null -eq $Value -or [string]::IsNullOrWhiteSpace([string]$Value)) { return '-' }
        return (([string]$Value).Replace('&', '&amp;').Replace('<', '&lt;').Replace('>', '&gt;').Replace('|', '&#124;').Replace('`', '&#96;') -replace '\r\n|\r|\n', '<br>')
    }
    function Get-E3Comparison($Row, $Ui) {
        $old = $oldByFile[[string]$Row.file]
        if ($null -eq $old) { return [ordered]@{ category = 'added'; label = '新增'; reason = 'No identically named edited file in round 2.' } }
        $currentPass = $Row.open -eq 'ok' -and $Row.object_model_assessment -eq 'pass'
        $oldFailed = $old.open -ne 'ok' -or $old.object_model_assessment -in @('error', 'mismatch')
        if ($mandatoryInk -contains [string]$Row.file) {
            if ($null -ne $Ui -and $Ui.recovery -in @('none', 'no_prompt', 'no recovery prompt', '无', '无恢复提示') -and $Row.open -eq 'ok') {
                return [ordered]@{ category = 'fixed'; label = '修复'; reason = 'Prior unreadable-content failure; current actual UI open had no recovery prompt.' }
            }
            return [ordered]@{ category = 'still_failed'; label = '仍失败 / 待确认'; reason = 'The mandatory no-recovery UI conclusion is absent or not successful.' }
        }
        if ($mandatoryChart -contains [string]$Row.file) { $oldFailed = $true }
        if ($oldFailed -and $currentPass -and ($null -eq $Ui -or $Ui.consistent -ne $false)) {
            return [ordered]@{ category = 'fixed'; label = '修复'; reason = 'Current preactivation object-model check meets the manifest; activation rollback is reported separately.' }
        }
        if ($oldFailed -and -not $currentPass) { return [ordered]@{ category = 'still_failed'; label = '仍失败 / 读数不完整'; reason = 'Prior failure or mismatch; current object-model result is not a complete pass.' } }
        if ($old.object_model_assessment -eq 'pass' -and -not $currentPass) { return [ordered]@{ category = 'new_issue'; label = '新增异常 / 待读数'; reason = 'Round 2 passed the collected checks; current result is not a complete pass.' } }
        return [ordered]@{ category = 'unchanged'; label = '不变'; reason = 'No confirmed change in the collected result relative to round 2.' }
    }

    $lines = New-Object Collections.Generic.List[string]
    $lines.Add('# Task B: round 3 edited documents')
    $lines.Add('')
    $lines.Add("Recorded $($rows.Count)/1544 generated documents and $($uiByFile.Count) separate UI observations. The manifest contains $($allManifest.Count) entries; 12 were not generated (11 unsupported ChartEx chartdata edits and one fields-toc-stale deleteblock invariant failure). Those 12 are not Word open failures.")
    $lines.Add('')
    $lines.Add('Batch method: actual read-only Word COM opens, DisplayAlerts=0, no resave; chart-edit documents and baselines are opened with Visible=true and activated. Each chartdata record preserves the readings before ChartData.Activate, workbook values, and the readings after the workbook closes without saving. A suppressed-alert COM open does not establish absence of a UI recovery prompt. JSONL preserves every actual attempt, including failures and retries. UI evidence is separately attributed below.')
    $lines.Add('')
    $lines.Add('An object-model pass covers only the listed checks. It does not prove unchanged unmeasured content or rendering. COM comparison of paragraph/table counts can be incomplete for tracked or merged content; raw readings and errors are retained. Image geometry is measured in points and recorded separately from visual image-content checks.')
    $lines.Add('')
    if ($inkAuditByFile.Count -gt 0) {
        $lines.Add('Independent ink review: the ink manifest requires one front overlay, and SaveOptions.inks is an authoritative replacement list. For sources already containing editor overlays, the collector baseline-plus-one count predicate is inapplicable. Hash-bound XML/code review is shown beside each raw mismatch below; it confirms baseline minus existing aidocs-ink overlays plus one, with non-overlay drawings and native ink retained. Original checks, object_model_assessment, and JSONL are unchanged. reviewed_object_model_assessment and reviewed_counts are additional interpretations, not replacement measurements or UI observations.')
        $lines.Add('')
    }
    if ($null -ne $plan) {
        $lines.Add("Sampling plan: 36 regular cases + 13 mandatory regressions + 11 available M7-operation cases = $($plan.unique_selected_count) unique files. No chartdata derivative exists among the 17 M7 baselines, so its requested additional sample is unavailable in the supplied input. Actual source count is 180 (110 original + 53 round-2 resaves + 17 M7); the task text's 127 omits the 53 resaves.")
        $lines.Add('')
    }
    $lines.Add('| 文件 | open | compat | 恢复提示 | marker / shapes / chart 读数 | 抽检：看到什么（未抽检写 -） | 与期望是否一致 | 与第二轮相比（新增 / 修复 / 仍失败 / 不变） |')
    $lines.Add('| --- | --- | --- | --- | --- | --- | --- | --- |')
    $orderedRows = New-Object Collections.ArrayList
    foreach ($item in $manifest) {
        if (-not $rowByFile.ContainsKey([string]$item.file)) { continue }
        $row = $rowByFile[[string]$item.file]
        $ui = $uiByFile[[string]$item.file]
        Set-E3Property $row 'ui' $ui
        Set-E3Property $row 'ui_checked' ($null -ne $ui)
        $independentReview = Get-E3IndependentInkReview $row
        Set-E3Property $row 'independent_ink_review' $independentReview
        $reviewedAssessment = if ($null -ne $independentReview) { [string]$independentReview.assessment } else { [string]$row.object_model_assessment }
        Set-E3Property $row 'reviewed_object_model_assessment' $reviewedAssessment
        $comparison = Get-E3Comparison $row $ui
        Set-E3Property $row 'round2_comparison' $comparison
        $reading = [ordered]@{ paragraphs = $row.metrics.paragraphs; tables = $row.metrics.tables; comments = $row.metrics.comments; inline_shapes = $row.metrics.inline_shapes; shapes = $row.metrics.shapes; first_table_rows = $row.metrics.first_table_rows; first_table_cells = $row.metrics.first_table_cells; checks = $row.checks; charts = $row.charts; shape_geometry = $row.shape_geometry }
        foreach ($key in @('error', 'close_error')) { if ($null -ne $row.$key) { $reading[$key] = $row.$key } }
        if ($null -ne $row.metrics -and @($row.metrics.errors).Count -gt 0) { $reading.metric_errors = $row.metrics.errors }
        if ($null -ne $independentReview) { $reading.independent_ink_review = $independentReview }
        $recovery = 'not checked (batch alerts suppressed)'
        $observation = '-'
        $assessment = "object model: $($row.object_model_assessment); UI: not checked"
        if ($null -ne $ui) {
            $recovery = [string]$ui.recovery
            $observation = [string]$ui.observation
            foreach ($key in @('screenshot', 'pdf', 'resaved')) {
                if (@($ui.$key).Count -gt 0) { $observation += "; ${key}: $(@($ui.$key) -join ', ')" }
            }
            $assessment = "object model: $($row.object_model_assessment); UI: $($ui.consistent)"
        }
        if ($null -ne $independentReview) { $assessment += '; independent XML/code review: pass (authoritative overlay replacement; raw +1 count predicate is inapplicable)' }
        $cells = @($row.file, $row.open, $row.metrics.compat, $recovery, ($reading | ConvertTo-Json -Depth 45 -Compress), $observation, $assessment, ($comparison.label + ': ' + $comparison.reason)) | ForEach-Object { ConvertTo-E3Cell $_ }
        $lines.Add('| ' + ($cells -join ' | ') + ' |')
        [void]$orderedRows.Add($row)
    }
    $missingMandatory = @($mandatory | Where-Object { -not $uiByFile.ContainsKey($_) })
    $missingPlanned = if ($null -ne $plan) { @($plan.selected.file | Where-Object { -not $uiByFile.ContainsKey($_) }) } else { @() }
    $summary = [ordered]@{ recorded = $orderedRows.Count; expected = 1544; actual_unique_baselines = @($manifest.base | Sort-Object -Unique).Count; open_ok = @($orderedRows | Where-Object open -eq 'ok').Count; open_error = @($orderedRows | Where-Object open -ne 'ok').Count; assessments = @($orderedRows | Group-Object object_model_assessment | ForEach-Object { [ordered]@{ result = $_.Name; count = $_.Count } }); ui_checked = $uiByFile.Count; missing_mandatory_ui = $missingMandatory; missing_planned_ui = $missingPlanned; comparison_counts = @($orderedRows | Group-Object { $_.round2_comparison.category } | ForEach-Object { [ordered]@{ result = $_.Name; count = $_.Count } }); non_generated_manifest_entries = @($allManifest | Where-Object status -ne 'generated'); declared_unavailable_sampling = @('M7 chartdata: no generated derivative in supplied manifest') }
    $summary.reviewed_counts = @($orderedRows | Group-Object reviewed_object_model_assessment | ForEach-Object { [ordered]@{ result = $_.Name; count = $_.Count } })
    $summary.independent_ink_review_count = @($orderedRows | Where-Object { $null -ne $_.independent_ink_review }).Count
    $summary.reviewed_counts_scope = 'Original measured checks plus separately hash-verified ink-list interpretation; UI findings remain separate.'
    [IO.File]::WriteAllText($resultsPath, (ConvertTo-Json -InputObject $orderedRows.ToArray() -Depth 65), $utf8)
    [IO.File]::WriteAllLines((Join-Path $OutputDirectory 'EDITED3.md'), $lines, $utf8)
    [IO.File]::WriteAllText((Join-Path $OutputDirectory '_scripts/edited3-summary.json'), ($summary | ConvertTo-Json -Depth 16), $utf8)
    if (-not $AllowPartial -and ($missingMandatory.Count -gt 0 -or $missingPlanned.Count -gt 0)) { throw "UI coverage incomplete: $($missingMandatory.Count) mandatory and $($missingPlanned.Count) planned files missing. Reports written honestly." }
    return [pscustomobject]$summary
}
