# This helper uses existing readouts only. It never opens Word or changes DOCX files.
# Dot-source, then call Update-EditedChartDerived before Update-EditedUiReport.

function Update-EditedChartDerived {
    [CmdletBinding()]
    param(
        [string] $OutputDirectory = 'C:/word/real-word-round2-20260907',
        [string] $ResultsPath,
        [string] $LargeReportBaselinePath
    )
    $ErrorActionPreference = 'Stop'
    if (-not $ResultsPath) { $ResultsPath = Join-Path $OutputDirectory 'edited-results.json' }
    if (-not $LargeReportBaselinePath) {
        $LargeReportBaselinePath = Join-Path $OutputDirectory '_readouts/large-report-baseline.json'
        $alternateBaselinePath = Join-Path $OutputDirectory '_scripts/large-report-visible-baseline.json'
        if (-not (Test-Path -LiteralPath $LargeReportBaselinePath) -and (Test-Path -LiteralPath $alternateBaselinePath)) { $LargeReportBaselinePath = $alternateBaselinePath }
    }
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    $rows = @((Get-Content -LiteralPath $ResultsPath -Raw -Encoding UTF8 | ConvertFrom-Json) | ForEach-Object { $_ })
    if ($rows.Count -ne 944) { throw "Expected 944 result rows; found $($rows.Count)." }
    $byFile = @{}
    foreach ($row in $rows) {
        if (-not $row.file -or $byFile.ContainsKey([string]$row.file)) { throw 'Result file names must be present and unique.' }
        $byFile[[string]$row.file] = $row
    }
    $pending = New-Object System.Collections.Generic.List[string]
    $changes = New-Object System.Collections.Generic.List[string]

    function Add-DerivedProperty($Target, [string] $Name, $Value) {
        $Target | Add-Member -MemberType NoteProperty -Name $Name -Value $Value -Force
    }

    function Test-ValuesEqual($Left, $Right) {
        if ($null -eq $Left -or $null -eq $Right) { return $null }
        $leftItems = @($Left)
        $rightItems = @($Right)
        if ($leftItems.Count -ne $rightItems.Count) { return $false }
        for ($i = 0; $i -lt $leftItems.Count; $i++) {
            if ($null -eq $leftItems[$i] -or $null -eq $rightItems[$i]) {
                if ($null -ne $leftItems[$i] -or $null -ne $rightItems[$i]) { return $false }
            } elseif ($leftItems[$i] -ne $rightItems[$i]) { return $false }
        }
        return $true
    }

    function Get-VerifiedTarget($Row, [string] $Collection, [int] $Index) {
        $targets = @($Row.charts | Where-Object { $_.collection -eq $Collection -and $_.index -eq $Index })
        if ($targets.Count -ne 1) { throw "Expected one $Collection index $Index in $($Row.file)." }
        $target = $targets[0]
        if ($target.activation -ne 'ok' -or $null -ne $target.activation_error -or $null -ne $target.workbook_close_error) { throw "Chart activation was not completed cleanly in $($Row.file)." }
        if ($null -eq $target.before -or $null -eq $target.after -or $target.before.errors.Count -gt 0 -or $target.after.errors.Count -gt 0) { throw "Chart readings are incomplete in $($Row.file)." }
        return $target
    }

    function Set-RollbackDerived($Row, $Target, $BaselineValues, [string] $Evidence) {
        $afterMatches = Test-ValuesEqual $Target.after.first_series_values $BaselineValues
        $beforeMatchesAfter = Test-ValuesEqual $Target.before.first_series_values $Target.after.first_series_values
        if ($null -eq $afterMatches -or $null -eq $beforeMatchesAfter) { throw "Missing values prevent rollback derivation for $($Row.file)." }
        if ($null -eq $Row.derived_chart_correction) {
            Add-DerivedProperty $Row 'derived_chart_correction' ([pscustomobject][ordered]@{
                original_after_matches_baseline = $Row.checks.chart_check.after_matches_baseline
                original_rollback_to_baseline = $Row.checks.chart_check.rollback_to_baseline
                original_object_model_assessment = $Row.object_model_assessment
                original_target_matches = $Row.checks.chart_check.matches
            })
        }
        Add-DerivedProperty $Row.derived_chart_correction 'evidence' $Evidence
        Add-DerivedProperty $Row.derived_chart_correction 'target_collection' $Target.collection
        Add-DerivedProperty $Row.derived_chart_correction 'target_index' $Target.index
        Add-DerivedProperty $Row.derived_chart_correction 'baseline_first_series_values' @($BaselineValues)
        Add-DerivedProperty $Row.derived_chart_correction 'method' 'Compare existing before/after Word readings with the corresponding verified baseline; independent of title matching.'
        $Row.checks.chart_check.after_matches_baseline = [bool]$afterMatches
        $Row.checks.chart_check.rollback_to_baseline = (-not $beforeMatchesAfter -and $afterMatches)
        $changes.Add("$($Row.file): rollback_to_baseline=$($Row.checks.chart_check.rollback_to_baseline); original title/value expectation result retained.")
    }

    $noTitle = $byFile['chart-no-title--chartdata.docx']
    if ($null -eq $noTitle) { throw 'Missing chart-no-title--chartdata.docx result.' }
    $noTitleTarget = Get-VerifiedTarget $noTitle 'InlineShapes' 1
    $noTitleBaselines = @($noTitle.baseline.charts | Where-Object { $_.collection -eq 'InlineShapes' -and $_.index -eq 1 })
    if ($noTitleBaselines.Count -ne 1 -or $noTitleBaselines[0].before.errors.Count -gt 0) { throw 'The no-title chart baseline is unavailable or incomplete.' }
    Set-RollbackDerived $noTitle $noTitleTarget $noTitleBaselines[0].before.first_series_values 'Existing row.baseline.charts: InlineShapes index 1; visible Word readout.'

    if (Test-Path -LiteralPath $LargeReportBaselinePath) {
        $baselineReadout = Get-Content -LiteralPath $LargeReportBaselinePath -Raw -Encoding UTF8 | ConvertFrom-Json
        $baselineCharts = @($baselineReadout.charts)
        $baselineCandidates = @($baselineCharts | Where-Object { $_.collection -eq 'InlineShapes' -and $_.index -eq 3 })
        if ($baselineCandidates.Count -eq 0 -and $baselineReadout.collection -eq 'InlineShapes' -and $baselineReadout.index -eq 3 -and $baselineCharts.Count -eq 1) {
            $baselineCandidates = @($baselineCharts[0])
        }
        if ($baselineCandidates.Count -ne 1) { throw 'large-report-baseline.json must identify exactly one baseline chart as collection=InlineShapes, index=3, either on its chart entry or on the root with one chart.' }
        $baselineChart = $baselineCandidates[0]
        $baselineValues = $null
        if ($null -ne $baselineChart.before) {
            if ($baselineChart.before.errors.Count -gt 0) { throw 'Large-report baseline chart reading contains errors.' }
            $baselineValues = $baselineChart.before.first_series_values
        } elseif (@($baselineChart.series).Count -gt 0) {
            $baselineValues = @($baselineChart.series)[0].values
        }
        if ($null -eq $baselineValues) { throw 'Large-report baseline has no first-series values.' }
        $largeReport = $byFile['large-report--chartdata.docx']
        if ($null -eq $largeReport) { throw 'Missing large-report--chartdata.docx result.' }
        $largeTarget = Get-VerifiedTarget $largeReport 'InlineShapes' 3
        $readoutHash = (Get-FileHash -LiteralPath $LargeReportBaselinePath -Algorithm SHA256).Hash
        Set-RollbackDerived $largeReport $largeTarget $baselineValues "$LargeReportBaselinePath; SHA256=$readoutHash; InlineShapes index 3."
    } else {
        $pending.Add('large-report--chartdata.docx: raw edited readout shows InlineShapes index 3 changing 11/21/31 to 10/20/30. Exact baseline-derived rollback awaits _readouts/large-report-baseline.json identifying baseline InlineShapes index 3.')
    }

    $chartExRows = @($rows | Where-Object { $_.op -eq 'newchart' -and $_.file -like 'chartex-*' -and $_.checks.chart_check.matches -eq $true -and $_.checks.shape_check.matches -eq $true })
    foreach ($row in $chartExRows) {
        $added = @($row.charts | Where-Object { $_.before.title -eq $row.checks.chart_check.expected_title })
        if ($added.Count -ne 1 -or $added[0].before.errors.Count -gt 0) { continue }
        $originalLimitations = @($row.charts | Where-Object { $_.before.title -ne $row.checks.chart_check.expected_title -and ($null -ne $_.error -or $_.before.errors.Count -gt 0) })
        if ($originalLimitations.Count -eq 0) { continue }
        Add-DerivedProperty $row 'added_chart_assessment' 'pass'
        Add-DerivedProperty $row 'preexisting_chart_limitation' 'Added chart title, first-series values 3/1/2, and shape counts pass. The original ChartEx chart cannot expose first-series Values through this Word object model; the overall incomplete reading is retained.'
    }

    $chartdata = @($rows | Where-Object { $_.op -eq 'chartdata' })
    $summary = [ordered]@{
        result_count = $rows.Count
        chart_reread_count = @($rows | Where-Object { $_.reread_method -eq 'visible_chart_pass' }).Count
        chartdata_count = $chartdata.Count
        chartdata_with_activation = @($chartdata | Where-Object { @($_.charts | Where-Object { $_.activation -eq 'ok' }).Count -gt 0 }).Count
        chartdata_with_observed_value_change = @($chartdata | Where-Object { @($_.charts | Where-Object { $_.values_changed_after_activate -eq $true }).Count -gt 0 }).Count
        chartdata_rollback_to_baseline_true = @($chartdata | Where-Object { $_.checks.chart_check.rollback_to_baseline -eq $true }).Count
        chartdata_rollback_to_baseline_false = @($chartdata | Where-Object { $_.checks.chart_check.rollback_to_baseline -eq $false }).Count
        chartdata_rollback_to_baseline_unknown = @($chartdata | Where-Object { $null -eq $_.checks.chart_check.rollback_to_baseline }).Count
        chartdata_expectation_mismatches = @($chartdata | Where-Object { $_.checks.chart_check.matches -eq $false } | ForEach-Object { $_.file })
        chartex_added_chart_pass_with_original_reading_limitation = @($rows | Where-Object { $_.added_chart_assessment -eq 'pass' -and $_.preexisting_chart_limitation }).Count
        changes = $changes.ToArray()
        pending = $pending.ToArray()
        raw_artifacts_preserved = @('edited-results.jsonl', 'edited-results-hidden-pass.json', '_readouts/*.json', 'all DOCX files')
    }
    $summaryLines = New-Object System.Collections.Generic.List[string]
    $summaryLines.Add('# Chart Derived Results')
    $summaryLines.Add('')
    $summaryLines.Add("Visible chart reread: $($summary.chart_reread_count) documents. All $($summary.chartdata_with_activation) chartdata documents opened chart data. Existing Word readings show value changes after activation in $($summary.chartdata_with_observed_value_change) documents; exact comparisons to recorded baseline values confirm $($summary.chartdata_rollback_to_baseline_true) rollbacks, $($summary.chartdata_rollback_to_baseline_false) unchanged baseline values, and $($summary.chartdata_rollback_to_baseline_unknown) pending comparison(s).")
    $summaryLines.Add('')
    $summaryLines.Add('The bubble chart and two scatter charts already expose 10/20/30 before editing data. The no-title chart still has an empty title. These expectation mismatches are retained.')
    $summaryLines.Add('')
    $summaryLines.Add("In $($summary.chartex_added_chart_pass_with_original_reading_limitation) ChartEx-base documents, the added chart and shape-count tests pass. Overall incomplete readings refer to the pre-existing ChartEx series-values limitation; its raw diagnostics are retained.")
    foreach ($line in $pending) { $summaryLines.Add(''); $summaryLines.Add($line) }
    [IO.File]::WriteAllText($ResultsPath, (ConvertTo-Json -InputObject $rows -Depth 60), $utf8)
    [IO.File]::WriteAllText((Join-Path $OutputDirectory 'chart-derived-summary.json'), (ConvertTo-Json -InputObject $summary -Depth 15), $utf8)
    [IO.File]::WriteAllLines((Join-Path $OutputDirectory 'CHART-DERIVED.md'), $summaryLines, $utf8)
    return [pscustomobject]$summary
}
