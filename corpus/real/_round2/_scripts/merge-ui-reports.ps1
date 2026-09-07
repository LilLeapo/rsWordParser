# Run after the batch has finished. JSONL remains the original first-layer log.
# Dot-source this file, then call Update-EditedUiReport.

function Update-EditedUiReport {
    [CmdletBinding()]
    param(
        [string] $OutputDirectory = 'C:/word/real-word-round2-20260907',
        [string] $ResultsPath,
        [string] $UiPath,
        [string] $MarkdownPath,
        [int] $ExpectedCount = 944
    )
    $ErrorActionPreference = 'Stop'
    if (-not $ResultsPath) { $ResultsPath = Join-Path $OutputDirectory 'edited-results.json' }
    if (-not $UiPath) { $UiPath = Join-Path $OutputDirectory 'ui-edited.json' }
    if (-not $MarkdownPath) { $MarkdownPath = Join-Path $OutputDirectory 'EDITED.md' }
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    $resultsText = [System.IO.File]::ReadAllText($ResultsPath, [System.Text.Encoding]::UTF8)
    $uiText = [System.IO.File]::ReadAllText($UiPath, [System.Text.Encoding]::UTF8)
    if (-not $resultsText.TrimStart().StartsWith('[')) { throw 'edited-results.json must contain a JSON array.' }
    if (-not $uiText.TrimStart().StartsWith('[')) { throw 'ui-edited.json must contain a JSON array.' }
    $results = @($resultsText | ConvertFrom-Json)
    $uiRows = @($uiText | ConvertFrom-Json)
    # An explicit array cast normalizes both Windows PowerShell 5.1 and PowerShell 7.
    $results = @($results | ForEach-Object { $_ })
    $uiRows = @($uiRows | ForEach-Object { $_ })
    if ($results.Count -ne $ExpectedCount) { throw "Expected $ExpectedCount results, found $($results.Count). Run the complete batch before merging." }
    $byFile = @{}
    foreach ($row in $results) {
        if ([string]::IsNullOrWhiteSpace([string]$row.file)) { throw 'A result row has no file name.' }
        if ($byFile.ContainsKey([string]$row.file)) { throw "Duplicate result file: $($row.file)" }
        $byFile[[string]$row.file] = $row
    }
    $uiByFile = @{}
    foreach ($ui in $uiRows) {
        if ([string]::IsNullOrWhiteSpace([string]$ui.file)) { throw 'A UI row has no file name.' }
        if (-not $byFile.ContainsKey([string]$ui.file)) { throw "UI file has no original result: $($ui.file)" }
        if ($uiByFile.ContainsKey([string]$ui.file)) { throw "Duplicate UI file: $($ui.file)" }
        foreach ($property in @('recovery', 'observation', 'consistent')) {
            if ($null -eq $ui.PSObject.Properties[$property]) { throw "UI row $($ui.file) is missing '$property'. Use null for an unknown value." }
        }
        $uiByFile[[string]$ui.file] = $ui
    }

    function Set-UiProperty($Row, [string] $Name, $Value) {
        $Row | Add-Member -MemberType NoteProperty -Name $Name -Value $Value -Force
    }

    function ConvertTo-Cell($Value) {
        if ($null -eq $Value) { return '-' }
        $text = [string]$Value
        if ([string]::IsNullOrWhiteSpace($text)) { return '-' }
        return ($text.Replace('&', '&amp;').Replace('<', '&lt;').Replace('>', '&gt;').Replace('|', '&#124;').Replace('`', '&#96;') -replace '\r\n|\r|\n', '<br>')
    }

    function Format-Consistency($Value) {
        if ($null -eq $Value) { return 'unknown' }
        if ($Value -is [bool]) { if ($Value) { return 'yes' } else { return 'no' } }
        if ([string]::IsNullOrWhiteSpace([string]$Value)) { return 'unknown' }
        return [string]$Value
    }

    function Get-ArtifactLabels($Ui) {
        $labels = New-Object System.Collections.Generic.List[string]
        foreach ($property in @('screenshot', 'pdf', 'resaved')) {
            foreach ($path in @($Ui.$property)) {
                if (-not [string]::IsNullOrWhiteSpace([string]$path)) { $labels.Add("${property}: $path") }
            }
        }
        return ($labels -join '; ')
    }

    $lines = New-Object System.Collections.Generic.List[string]
    $lines.Add('# B2-b edited documents: Word and UI observations')
    $lines.Add('')
    $visibleRereads = @($results | Where-Object { $_.reread_method -eq 'visible_chart_pass' }).Count
    $lines.Add("Recorded $($results.Count) / $ExpectedCount object-model results; $($uiRows.Count) files have separate UI observations. Initial object-model opens used DisplayAlerts=0, ReadOnly=true and Visible=false; $visibleRereads chart rows were subsequently reread with Visible=true and Document.Activate. An unchecked repair prompt is not evidence that no prompt would appear. Original errors and readings remain in JSONL and the hidden-pass snapshot.")
    $lines.Add('')
    $lines.Add([System.Text.RegularExpressions.Regex]::Unescape('| \u6587\u4EF6 | open | compat | \u6062\u590D\u63D0\u793A | marker / shapes / chart \u7684\u8BFB\u6570 | \u62BD\u68C0\uFF1A\u770B\u5230\u4EC0\u4E48\uFF08\u672A\u62BD\u68C0\u5199 -\uFF09 | \u4E0E\u671F\u671B\u662F\u5426\u4E00\u81F4 |'))
    $lines.Add('| --- | --- | --- | --- | --- | --- | --- |')

    foreach ($row in $results) {
        $ui = $null
        if ($uiByFile.ContainsKey([string]$row.file)) { $ui = $uiByFile[[string]$row.file] }
        Set-UiProperty $row 'ui' $ui
        Set-UiProperty $row 'ui_checked' ($null -ne $ui)
        foreach ($pair in @(
            @('ui_recovery', 'recovery'),
            @('ui_observation', 'observation'),
            @('ui_consistent', 'consistent'),
            @('ui_screenshot', 'screenshot'),
            @('ui_pdf', 'pdf'),
            @('ui_resaved', 'resaved')
        )) {
            $value = $null
            if ($null -ne $ui) { $value = $ui.($pair[1]) }
            Set-UiProperty $row $pair[0] $value
        }
        $reading = [ordered]@{
            paragraphs = $row.metrics.paragraphs
            tables = $row.metrics.tables
            inline_shapes = $row.metrics.inline_shapes
            shapes = $row.metrics.shapes
            first_table_rows = $row.metrics.first_table_rows
            first_table_cells = $row.metrics.first_table_cells
            checks = $row.checks
            charts = $row.charts
        }
        if ($null -ne $row.error) { $reading.error = $row.error }
        if ($null -ne $row.close_error) { $reading.close_error = $row.close_error }
        if ($null -ne $row.metrics -and @($row.metrics.errors).Count -gt 0) { $reading.metric_errors = $row.metrics.errors }
        if ($row.added_chart_assessment) { $reading.added_chart_assessment = $row.added_chart_assessment }
        if ($row.preexisting_chart_limitation) { $reading.preexisting_chart_limitation = $row.preexisting_chart_limitation }
        if ($null -ne $row.derived_chart_correction) { $reading.derived_chart_correction = $row.derived_chart_correction }
        $readingCell = ConvertTo-Cell (ConvertTo-Json -InputObject $reading -Compress -Depth 40)
        $fileCell = ConvertTo-Cell $row.file
        $openCell = ConvertTo-Cell $row.open
        $compatCell = ConvertTo-Cell $row.metrics.compat
        $recoveryCell = 'not checked (alerts suppressed in batch)'
        $observationCell = '-'
        $assessment = "object model: $($row.object_model_assessment); UI: not checked"
        if ($null -ne $ui) {
            $recoveryCell = ConvertTo-Cell $ui.recovery
            $observation = [string]$ui.observation
            $artifacts = Get-ArtifactLabels $ui
            if ($artifacts) {
                if ($observation) { $observation += [Environment]::NewLine }
                $observation += $artifacts
            }
            $observationCell = ConvertTo-Cell $observation
            $assessment = "object model: $($row.object_model_assessment); UI: $(Format-Consistency $ui.consistent)"
        }
        $assessmentCell = ConvertTo-Cell $assessment
        $lines.Add("| $fileCell | $openCell | $compatCell | $recoveryCell | $readingCell | $observationCell | $assessmentCell |")
    }

    # Serialize and validate before changing either report.
    $json = ConvertTo-Json -InputObject $results -Depth 60
    $roundTrip = @($json | ConvertFrom-Json)
    $roundTrip = @($roundTrip | ForEach-Object { $_ })
    if ($roundTrip.Count -ne $ExpectedCount) { throw 'Merged JSON failed its row-count check.' }
    [System.IO.File]::WriteAllText($ResultsPath, $json, $utf8)
    [System.IO.File]::WriteAllLines($MarkdownPath, $lines, $utf8)
    return [pscustomobject]@{ results = $results.Count; ui_observed = $uiRows.Count; ui_unchecked = $results.Count - $uiRows.Count; json = $ResultsPath; markdown = $MarkdownPath }
}
