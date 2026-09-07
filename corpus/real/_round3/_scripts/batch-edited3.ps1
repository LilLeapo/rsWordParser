# Dot-source this file, then call Invoke-Edited3Batch -Word $word.
# The caller owns the Word application. This script never quits Word or Excel.

function Invoke-Edited3Batch {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [object] $Word,
        [string] $InputDirectory = 'C:/word/round3-work-20260907/real-word-round3-inputs/edited',
        [string] $OutputDirectory = 'C:/word/real-word-round3-20260907',
        [string] $BaselineRoot = 'C:/word/round2-work-20260907/baseline/real-word-corpus-20260906',
        [string] $Round2Root = 'C:/word/real-word-round2-20260907',
        [string[]] $OnlyFiles = @(),
        [string[]] $RetryFiles = @(),
        [int] $Limit = 0,
        [switch] $NoResume
    )

    $ErrorActionPreference = 'Stop'
    . (Join-Path $PSScriptRoot 'edited3-common.ps1')
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    $manifest = @((Get-Content -LiteralPath (Join-Path $InputDirectory 'manifest.json') -Raw -Encoding UTF8 | ConvertFrom-Json) | Where-Object { $_.status -eq 'generated' })
    if ($manifest.Count -ne 1544) { throw "Expected 1544 generated entries, found $($manifest.Count)." }
    $knownFiles = @($manifest.file)
    foreach ($file in @($OnlyFiles) + @($RetryFiles)) {
        if ($knownFiles -notcontains $file) { throw "Requested file is not a generated manifest entry: $file" }
    }
    [void][System.IO.Directory]::CreateDirectory($OutputDirectory)
    $logPath = Join-Path $OutputDirectory 'edited3-results.jsonl'
    $baselineLogPath = Join-Path $OutputDirectory 'edited3-baselines.jsonl'
    $stagePath = Join-Path $OutputDirectory 'edited3-current-item.json'
    $rows = @{}
    $baselines = @{}
    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    $script:editedBatchIndex = 0
    $script:editedBatchFile = ''

    function Write-BatchStage([string] $Stage, [string] $Detail = '') {
        $state = [ordered]@{
            timestamp = [DateTime]::Now.ToString('o')
            index = $script:editedBatchIndex
            total = $manifest.Count
            file = $script:editedBatchFile
            stage = $Stage
            detail = $Detail
            elapsed_seconds = [Math]::Round($clock.Elapsed.TotalSeconds, 1)
        }
        [System.IO.File]::WriteAllText($stagePath, ($state | ConvertTo-Json -Depth 6), $utf8)
    }

    function Get-Baseline([string] $RelativePath) {
        $visibleBaseline = $true
        if ($baselines.ContainsKey($RelativePath)) {
            if ($baselines[$RelativePath].reread_method -eq 'visible_chart_pass' -and $baselines[$RelativePath].open -eq 'ok') { return $baselines[$RelativePath] }
            $baselines.Remove($RelativePath)
        }
        $baseline = [ordered]@{ base = $RelativePath; open = 'error'; error = $null; metrics = $null; charts = @(); close_error = $null; reread_method = $(if ($visibleBaseline) { 'visible_chart_pass' } else { $null }) }
        $document = $null
        try {
            $path = Resolve-E3BaselinePath $RelativePath $BaselineRoot $Round2Root
            $baseline['source_path'] = $path
            if (-not (Test-Path -LiteralPath $path)) { throw "Baseline file not found: $path" }
            Write-BatchStage 'baseline-open' $RelativePath
            $document = Open-E3BatchDocument $path $visibleBaseline
            if ($null -eq $document) { throw 'Word returned no baseline document.' }
            $baseline.open = 'ok'
            if ($visibleBaseline) { Write-BatchStage 'baseline-activate' $RelativePath; [void]$document.Activate() }
            Write-BatchStage 'baseline-read' $RelativePath
            $baseline.metrics = Get-E3DocumentMetrics $document
            $baseline.charts = Get-E3DocumentCharts $document $false
        } catch { $baseline.error = Get-E3RawError $_ }
        finally {
            if ($null -ne $document) {
                try { Write-BatchStage 'baseline-close' $RelativePath; [void]$document.Close(0) }
                catch { $baseline.close_error = Get-E3RawError $_ }
            }
        }
        $baselines[$RelativePath] = $baseline
        [System.IO.File]::AppendAllText($baselineLogPath, (($baseline | ConvertTo-Json -Depth 30 -Compress) + [Environment]::NewLine), $utf8)
        return $baseline
    }

    function Write-EditedReports {
        $orderedRows = @($manifest | ForEach-Object { if ($rows.ContainsKey($_.file)) { $rows[$_.file] } })
        [System.IO.File]::WriteAllText((Join-Path $OutputDirectory 'edited3-results.json'), (ConvertTo-Json -InputObject $orderedRows -Depth 40), $utf8)
        . (Join-Path $PSScriptRoot 'edited3-report.ps1')
        [void](Update-Edited3Report -OutputDirectory $OutputDirectory -InputDirectory $InputDirectory -AllowPartial)

    }

    if ($NoResume) {
        if (Test-Path -LiteralPath $logPath) { throw 'NoResume refuses to overwrite an existing edited3-results.jsonl. Choose a new output directory.' }
    } else {
        if (Test-Path -LiteralPath $logPath) {
            foreach ($line in [System.IO.File]::ReadLines($logPath)) {
                if (-not $line.Trim()) { continue }
                try { $entry = $line | ConvertFrom-Json; if ($entry.file) { $rows[$entry.file] = $entry } }
                catch { Write-Warning 'Ignoring an incomplete JSONL line left by an interrupted write.' }
            }
        }
    }
    if (Test-Path -LiteralPath $baselineLogPath) {
        foreach ($line in [System.IO.File]::ReadLines($baselineLogPath)) {
            if (-not $line.Trim()) { continue }
            try { $entry = $line | ConvertFrom-Json; if ($entry.base) { $baselines[$entry.base] = $entry } }
            catch { Write-Warning 'Ignoring an incomplete baseline JSONL line.' }
        }
    }
    foreach ($base in @($manifest.base | Sort-Object -Unique)) { [void](Resolve-E3BaselinePath $base $BaselineRoot $Round2Root) }
    $oldAlerts = $Word.DisplayAlerts
    $Word.DisplayAlerts = 0
    $processed = 0
    try {
        foreach ($item in $manifest) {
            $script:editedBatchIndex++
            $script:editedBatchFile = [string]$item.file
            if ($item.file -ne [IO.Path]::GetFileName([string]$item.file)) { throw 'Manifest file must be a basename.' }
            if ($OnlyFiles.Count -gt 0 -and $OnlyFiles -notcontains [string]$item.file) { continue }
            $visibleItem = $item.op -in @('newchart', 'chartdata')
            if ($rows.ContainsKey($item.file) -and $RetryFiles -notcontains [string]$item.file) { continue }
            if ($Limit -gt 0 -and $processed -ge $Limit) { break }
            $document = $null
            $row = [ordered]@{
                file = $item.file
                base = $item.base
                op = $item.op
                expect = $item.expect
                timestamp = [DateTime]::Now.ToString('o')
                reread_method = $(if ($visibleItem) { 'visible_chart_pass' } else { 'hidden_nonchart_pass' })
                open_method = $(if ($visibleItem) { 'ReadOnly=true; Visible=true; Document.Activate; DisplayAlerts=0' } else { 'ReadOnly=true; Visible=false; DisplayAlerts=0' })
                prior_attempt = $(if ($rows.ContainsKey($item.file)) { $rows[$item.file] } else { $null })
                open = 'error'
                error = $null
                metrics = $null
                baseline = $null
                checks = $null
                shape_geometry = @()
                charts = @()
                close_error = $null
                repair_prompt = 'not_checked_alerts_suppressed'
                ui_checked = $false
                ui_observation = $null
                object_model_assessment = 'error'
            }
            try {
                Write-BatchStage 'item-start'
                $row.baseline = Get-Baseline $item.base
                Write-BatchStage 'document-open'
                $document = Open-E3BatchDocument (Join-Path $InputDirectory $item.file) ([bool]$visibleItem)
                if ($null -eq $document) { throw 'Word returned no edited document.' }
                $row.open = 'ok'
                if ($visibleItem) { Write-BatchStage 'document-activate'; [void]$document.Activate() }
                Write-BatchStage 'document-metrics'
                $row.metrics = Get-E3DocumentMetrics $document
                if ($item.op -in @('newchart', 'chartdata')) {
                    $row.charts = Get-E3DocumentCharts $document ($item.op -eq 'chartdata')
                    if (@($row.charts).Count -eq 0) {
                        try { throw 'No chart objects were collected from a chart-edit document; chart validation is incomplete.' }
                        catch { $row.charts = @([ordered]@{ collection = 'document'; index = $null; error = (Get-E3RawError $_) }) }
                    }
                }
                if ($item.op -in @('newimage', 'ink', 'replaceimage')) { $row.shape_geometry = Get-E3ShapeGeometry $document }
                Write-BatchStage 'document-checks'
                $row.checks = Get-E3Checks $document $item $row.metrics $row.baseline $row.charts
                $activeChecks = @($row.checks.marker, $row.checks.shape_check, $row.checks.chart_check, $row.checks.table_check | Where-Object { $null -ne $_ })
                $failedChecks = @($activeChecks | Where-Object { $null -ne $_.matches -and $_.matches -eq $false })
                $unknownChecks = @($activeChecks | Where-Object { $null -eq $_.matches })
                $chartErrors = @($row.charts | Where-Object {
                    $null -ne $_.error -or $null -ne $_.activation_error -or $null -ne $_.workbook_read_error -or $null -ne $_.workbook_close_error -or
                    ($null -ne $_.before -and $_.before.errors.Count -gt 0) -or
                    ($null -ne $_.after -and $_.after.errors.Count -gt 0)
                })
                $geometryErrors = @($row.shape_geometry | Where-Object { $null -ne $_.errors -and @($_.errors).Count -gt 0 })
                $metricsIncomplete = $null -eq $row.metrics -or ($null -ne $row.metrics.errors -and @($row.metrics.errors).Count -gt 0)
                $checksIncomplete = $null -eq $row.checks -or ($null -ne $row.checks.errors -and @($row.checks.errors).Count -gt 0)
                $baselineIncomplete = $null -eq $row.baseline -or $row.baseline.open -ne 'ok' -or $null -ne $row.baseline.error -or $null -ne $row.baseline.close_error -or $null -eq $row.baseline.metrics -or ($null -ne $row.baseline.metrics.errors -and @($row.baseline.metrics.errors).Count -gt 0)
                $row.object_model_assessment = 'pass'
                if (($null -ne $row.metrics -and $row.metrics.compat -ne 15) -or $failedChecks.Count -gt 0) { $row.object_model_assessment = 'mismatch' }
                if ($unknownChecks.Count -gt 0 -or $metricsIncomplete -or $checksIncomplete -or $chartErrors.Count -gt 0 -or $geometryErrors.Count -gt 0 -or $baselineIncomplete) { $row.object_model_assessment = 'incomplete' }
            } catch {
                $row.error = Get-E3RawError $_
                $row.object_model_assessment = 'error'
            } finally {
                if ($null -ne $document) {
                    try { Write-BatchStage 'document-close'; [void]$document.Close(0) }
                    catch { $row.close_error = Get-E3RawError $_; $row.object_model_assessment = 'incomplete' }
                }
            }
            Write-BatchStage 'record-result'
            $rows[$item.file] = $row
            [System.IO.File]::AppendAllText($logPath, (($row | ConvertTo-Json -Depth 40 -Compress) + [Environment]::NewLine), $utf8)
            $processed++
            if (($processed % 25) -eq 0) {
                Write-Host ("B3 {0}/{1}; processed this pass={5}: {2}; {3}; elapsed={4}s" -f $rows.Count, $manifest.Count, $item.file, $row.object_model_assessment, [Math]::Round($clock.Elapsed.TotalSeconds, 1), $processed)
                Write-EditedReports
            }
        }
    } finally {
        try { $Word.DisplayAlerts = $oldAlerts } catch { Write-Warning $_.Exception.Message }
        Write-BatchStage 'write-reports'
        Write-EditedReports
    }
    Write-BatchStage 'complete' "$($rows.Count)/$($manifest.Count) recorded"
    return [pscustomobject]@{ recorded = $rows.Count; total = $manifest.Count; processed_this_run = $processed; json = (Join-Path $OutputDirectory 'edited3-results.json'); markdown = (Join-Path $OutputDirectory 'EDITED3.md'); seconds = [Math]::Round($clock.Elapsed.TotalSeconds, 1) }
}
