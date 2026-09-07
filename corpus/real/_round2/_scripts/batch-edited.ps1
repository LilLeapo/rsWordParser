# Dot-source this file, then call Invoke-EditedBatch -Word $word.
# The caller owns the Word application. This script never quits Word or Excel.

function Invoke-EditedBatch {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)] [object] $Word,
        [string] $InputDirectory = 'C:/word/round2-work-20260907/real-word-round2-inputs/_roundtrip/edited',
        [string] $OutputDirectory = 'C:/word/real-word-round2-20260907',
        [string] $BaselineRoot = 'C:/word/round2-work-20260907/baseline/real-word-corpus-20260906',
        [string] $BaselineZip = 'C:/word/real-word-corpus-20260906.zip',
        [string[]] $SkipFiles = @(),
        [string] $SkipReason = 'Word process stopped responding; watchdog terminated the owned Word instance before a result was available.',
        [int] $Limit = 0,
        [switch] $NoResume,
        [switch] $ChartPass
    )

    $ErrorActionPreference = 'Stop'
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    $manifest = @((Get-Content -LiteralPath (Join-Path $InputDirectory 'manifest.json') -Raw -Encoding UTF8 | ConvertFrom-Json) | Where-Object { $_.status -eq 'generated' })
    if ($manifest.Count -ne 944) { throw "Expected 944 generated entries, found $($manifest.Count)." }
    [void][System.IO.Directory]::CreateDirectory($OutputDirectory)
    $logPath = Join-Path $OutputDirectory 'edited-results.jsonl'
    $baselineLogPath = Join-Path $OutputDirectory 'edited-baselines.jsonl'
    $stagePath = Join-Path $OutputDirectory 'edited-current-item.json'
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

    function Get-RawError($Record) {
        return [ordered]@{
            message = $Record.Exception.Message
            exception = $Record.Exception.ToString()
            error_record = ($Record | Out-String).TrimEnd()
            hresult = $Record.Exception.HResult
            category = [string]$Record.CategoryInfo
            fully_qualified_error_id = $Record.FullyQualifiedErrorId
        }
    }

    function Add-ReadError($Target, [string] $Stage, $Record) {
        [void]$Target.errors.Add([ordered]@{ stage = $Stage; error = (Get-RawError $Record) })
    }

    function Open-BatchDocument([string] $Path, [bool] $Visible = $false) {
        $missing = [Type]::Missing
        return $Word.Documents.Open($Path, $false, $true, $false, $missing, $missing, $false, $missing, $missing, $missing, $missing, $Visible)
    }

    function Get-DocumentMetrics($Document) {
        $metrics = [ordered]@{
            compat = $null
            paragraphs = $null
            tables = $null
            inline_shapes = $null
            shapes = $null
            first_table_rows = $null
            first_table_cells = $null
            errors = (New-Object System.Collections.ArrayList)
        }
        foreach ($property in @('compat', 'paragraphs', 'tables', 'inline_shapes', 'shapes')) {
            try {
                $collection = $null
                $value = $null
                switch ($property) {
                    'compat' { $value = $Document.CompatibilityMode }
                    'paragraphs' { $collection = $Document.Paragraphs }
                    'tables' { $collection = $Document.Tables }
                    'inline_shapes' { $collection = $Document.InlineShapes }
                    'shapes' { $collection = $Document.Shapes }
                }
                if ($property -ne 'compat') {
                    if ($null -eq $collection) { throw "Word returned no collection for $property; the property getter may have failed." }
                    $value = $collection.Count
                }
                if ($null -eq $value) { throw "Word returned no value for $property; the property getter may have failed." }
                $metrics[$property] = [int]$value
            } catch { Add-ReadError $metrics "metrics.$property" $_ }
        }
        if ($metrics.tables -gt 0) {
            try {
                $tableRows = $Document.Tables.Item(1).Rows
                if ($null -eq $tableRows) { throw 'Word returned no Rows collection; vertically merged cells can make row access unavailable.' }
                $count = $tableRows.Count
                if ($null -eq $count) { throw 'Word returned no Rows.Count value.' }
                $metrics.first_table_rows = [int]$count
            } catch { Add-ReadError $metrics 'metrics.first_table_rows' $_ }
            try {
                $tableCells = $Document.Tables.Item(1).Range.Cells
                if ($null -eq $tableCells) { throw 'Word returned no Cells collection for the first table.' }
                $count = $tableCells.Count
                if ($null -eq $count) { throw 'Word returned no Cells.Count value.' }
                $metrics.first_table_cells = [int]$count
            } catch { Add-ReadError $metrics 'metrics.first_table_cells' $_ }
        }
        return $metrics
    }

    function Convert-ChartValues($Values) {
        $result = New-Object System.Collections.ArrayList
        foreach ($value in @($Values)) {
            if ($null -eq $value) { [void]$result.Add($null) }
            elseif ($value -is [ValueType]) { [void]$result.Add($value) }
            else { [void]$result.Add([string]$value) }
        }
        return ,$result.ToArray()
    }

    function Get-OneChartReadings($Chart) {
        $reading = [ordered]@{ title = $null; series_name = $null; first_series_values = $null; errors = (New-Object System.Collections.ArrayList) }
        try {
            $hasTitle = $Chart.HasTitle
            if ($null -eq $hasTitle) { throw 'Chart.HasTitle returned null; the chart object is not available to the collector.' }
            if ($hasTitle) {
                $title = $Chart.ChartTitle.Text
                if ($null -eq $title) { throw 'Chart.ChartTitle.Text returned null.' }
                $reading.title = [string]$title
            }
            else { $reading.title = '' }
        } catch { Add-ReadError $reading 'chart.title' $_ }
        try {
            $name = $Chart.SeriesCollection(1).Name
            if ($null -eq $name) { throw 'Chart first-series Name returned null.' }
            $reading.series_name = [string]$name
        } catch { Add-ReadError $reading 'chart.series_name' $_ }
        try {
            $values = $Chart.SeriesCollection(1).Values
            if ($null -eq $values) { throw 'Chart first-series Values returned null.' }
            $reading.first_series_values = Convert-ChartValues $values
        } catch { Add-ReadError $reading 'chart.first_series_values' $_ }
        return $reading
    }

    function Get-DocumentCharts($Document, [bool] $ActivateData) {
        $result = New-Object System.Collections.ArrayList
        foreach ($kind in @('InlineShapes', 'Shapes')) {
            $collection = $null
            try {
                if ($kind -eq 'InlineShapes') { $collection = $Document.InlineShapes }
                else { $collection = $Document.Shapes }
                if ($null -eq $collection) { throw "Word returned no $kind collection." }
                $countValue = $collection.Count
                if ($null -eq $countValue) { throw "Word returned no $kind.Count value." }
                $count = [int]$countValue
            }
            catch {
                [void]$result.Add([ordered]@{ collection = $kind; index = $null; error = (Get-RawError $_) })
                continue
            }
            for ($i = 1; $i -le $count; $i++) {
                $shape = $null
                $chart = $null
                try {
                    $shape = $collection.Item($i)
                    if ($null -eq $shape) { throw "Word returned no $kind.Item($i) object." }
                    $shapeType = $shape.Type
                    $hasChart = $shape.HasChart
                    $typeIsChart = ($kind -eq 'InlineShapes' -and $shapeType -eq 12) -or ($kind -eq 'Shapes' -and $shapeType -eq 3)
                    $detectionError = $null
                    if ($null -eq $hasChart) {
                        if (-not $typeIsChart -and $null -ne $shapeType) { continue }
                        try { throw "Word returned null HasChart for $kind.Item($i), Type=$shapeType; using chart type fallback when available." }
                        catch { $detectionError = Get-RawError $_ }
                        if (-not $typeIsChart) { throw 'Neither HasChart nor a chart shape Type is available.' }
                    } elseif ([int]$hasChart -eq 0 -and -not $typeIsChart) { continue }
                    Write-BatchStage 'chart-read' "$kind[$i]"
                    $chart = $shape.Chart
                    if ($null -eq $chart) { throw "Word returned null Chart for $kind.Item($i), Type=$shapeType, HasChart=$hasChart." }
                    $entry = [ordered]@{
                        collection = $kind
                        index = $i
                        shape_type = $shapeType
                        has_chart = $hasChart
                        detection = $(if ($null -eq $hasChart) { 'shape_type_fallback' } else { 'has_chart' })
                        detection_error = $detectionError
                        before = (Get-OneChartReadings $chart)
                        activation = 'not_requested'
                        activation_error = $null
                        workbook_name = $null
                        workbook_close_error = $null
                        after = $null
                        values_changed_after_activate = $null
                    }
                    if ($ActivateData) {
                        $workbook = $null
                        try {
                            Write-BatchStage 'chartdata-activate' "$kind[$i]"
                            [void]$chart.ChartData.Activate()
                            $entry.activation = 'ok'
                            Write-BatchStage 'chartdata-workbook' "$kind[$i]"
                            $workbook = $chart.ChartData.Workbook
                            if ($null -eq $workbook) { throw 'ChartData.Activate returned without an accessible Workbook.' }
                            $entry.workbook_name = [string]$workbook.Name
                        } catch {
                            $entry.activation = 'error'
                            $entry.activation_error = Get-RawError $_
                        } finally {
                            if ($null -ne $workbook) {
                                try {
                                    Write-BatchStage 'chartdata-close-workbook' "$kind[$i]"
                                    [void]$workbook.Close($false)
                                } catch { $entry.workbook_close_error = Get-RawError $_ }
                            }
                        }
                        Write-BatchStage 'chartdata-reread' "$kind[$i]"
                        $entry.after = Get-OneChartReadings $chart
                        if ($null -ne $entry.before.first_series_values -and $null -ne $entry.after.first_series_values) {
                            $entry.values_changed_after_activate = (($entry.before.first_series_values | ConvertTo-Json -Compress) -ne ($entry.after.first_series_values | ConvertTo-Json -Compress))
                        }
                    }
                    [void]$result.Add($entry)
                } catch {
                    [void]$result.Add([ordered]@{ collection = $kind; index = $i; error = (Get-RawError $_) })
                }
            }
        }
        return ,$result.ToArray()
    }

    function Get-Baseline([string] $RelativePath) {
        $visibleBaseline = $ChartPass -and $RelativePath.StartsWith('chart/')
        if ($baselines.ContainsKey($RelativePath)) {
            if (-not $visibleBaseline -or $baselines[$RelativePath].reread_method -eq 'visible_chart_pass') { return $baselines[$RelativePath] }
            $baselines.Remove($RelativePath)
        }
        $baseline = [ordered]@{ base = $RelativePath; open = 'error'; error = $null; metrics = $null; charts = @(); close_error = $null; reread_method = $(if ($visibleBaseline) { 'visible_chart_pass' } else { $null }) }
        $document = $null
        try {
            $path = Join-Path $BaselineRoot $RelativePath
            if (-not (Test-Path -LiteralPath $path)) { throw "Baseline file not found: $path" }
            Write-BatchStage 'baseline-open' $RelativePath
            $document = Open-BatchDocument $path $visibleBaseline
            $baseline.open = 'ok'
            if ($visibleBaseline) { Write-BatchStage 'baseline-activate' $RelativePath; [void]$document.Activate() }
            Write-BatchStage 'baseline-read' $RelativePath
            $baseline.metrics = Get-DocumentMetrics $document
            if ($RelativePath.StartsWith('chart/')) { $baseline.charts = Get-DocumentCharts $document $false }
        } catch { $baseline.error = Get-RawError $_ }
        finally {
            if ($null -ne $document) {
                try { Write-BatchStage 'baseline-close' $RelativePath; [void]$document.Close(0) }
                catch { $baseline.close_error = Get-RawError $_ }
            }
        }
        $baselines[$RelativePath] = $baseline
        [System.IO.File]::AppendAllText($baselineLogPath, (($baseline | ConvertTo-Json -Depth 30 -Compress) + [Environment]::NewLine), $utf8)
        return $baseline
    }

    function Compare-Number($Actual, $Expected) {
        if ($null -eq $Actual -or $null -eq $Expected) { return $null }
        return ([int]$Actual -eq [int]$Expected)
    }

    function Get-Checks($Document, $Item, $Metrics, $Baseline, $Charts) {
        $checks = [ordered]@{ marker = $null; shape_check = $null; chart_check = $null; table_check = $null; errors = (New-Object System.Collections.ArrayList) }
        $b = $Baseline.metrics
        try {
            switch ($Item.op) {
                'insert' {
                    $firstText = $null
                    for ($p = 1; $p -le $Document.Paragraphs.Count; $p++) {
                        $candidate = [string]$Document.Paragraphs.Item($p).Range.Text
                        if (($candidate -replace '[\x00-\x20\u00A0]', '').Length -gt 0) { $firstText = $candidate; break }
                    }
                    $prefix = 'rsword' + [char]0x270E + ' '
                    $checks.marker = [ordered]@{ kind = 'first_text_paragraph_prefix'; actual = $firstText; expected = $prefix; matches = ($null -ne $firstText -and $firstText.StartsWith($prefix, [StringComparison]::Ordinal)) }
                }
                'header' {
                    $header = [string]$Document.Sections.Last.Headers.Item(1).Range.Text
                    $expected = 'rsword ' + [char]0x9875 + [char]0x7709
                    $checks.marker = [ordered]@{ kind = 'last_section_primary_header'; actual = $header; expected = $expected; matches = $header.Contains($expected) }
                }
                'comment' {
                    $authors = @()
                    for ($c = 1; $c -le $Document.Comments.Count; $c++) { $authors += [string]$Document.Comments.Item($c).Author }
                    $checks.marker = [ordered]@{ kind = 'comment_author'; count = [int]$Document.Comments.Count; authors = $authors; matches = ($Document.Comments.Count -ge 1 -and $authors -contains 'rsword') }
                }
                'split' {
                    $expected = $null
                    if ($null -ne $b -and $null -ne $b.paragraphs) { $expected = [int]$b.paragraphs + 1 }
                    $checks.marker = [ordered]@{ kind = 'paragraph_count_plus_one'; baseline = $b.paragraphs; actual = $Metrics.paragraphs; expected = $expected; matches = (Compare-Number $Metrics.paragraphs $expected) }
                }
                'deleteblock' {
                    $reduced = $null
                    $deltas = [ordered]@{}
                    foreach ($name in @('paragraphs', 'tables', 'inline_shapes', 'shapes')) {
                        $delta = $null
                        if ($null -ne $b -and $null -ne $b.$name -and $null -ne $Metrics.$name) {
                            $delta = [int]$Metrics.$name - [int]$b.$name
                            if ($null -eq $reduced) { $reduced = $false }
                            if ($delta -lt 0) { $reduced = $true }
                        }
                        $deltas[$name] = $delta
                    }
                    $checks.marker = [ordered]@{ kind = 'at_least_one_count_reduced'; deltas = $deltas; matches = $reduced }
                }
            }
        } catch { Add-ReadError $checks 'marker' $_ }
        if ($Item.op -in @('newimage', 'ink', 'newchart', 'replaceimage')) {
            $expectedInline = $null
            $expectedFloating = $null
            if ($null -ne $b) {
                if ($null -ne $b.inline_shapes) { $expectedInline = [int]$b.inline_shapes }
                if ($null -ne $b.shapes) { $expectedFloating = [int]$b.shapes }
                if ($Item.op -eq 'newchart' -and $null -ne $expectedInline) { $expectedInline++ }
                if ($Item.op -in @('newimage', 'ink') -and $null -ne $expectedFloating) { $expectedFloating++ }
            }
            $inlineMatches = Compare-Number $Metrics.inline_shapes $expectedInline
            $floatingMatches = Compare-Number $Metrics.shapes $expectedFloating
            $match = $null
            if ($null -ne $inlineMatches -and $null -ne $floatingMatches) { $match = $inlineMatches -and $floatingMatches }
            $checks.shape_check = [ordered]@{ expected_inline_shapes = $expectedInline; expected_shapes = $expectedFloating; matches = $match }
        }
        if ($Item.op -in @('insertrow', 'mergecells')) {
            $match = $null
            if ($null -ne $b) {
                if ($Item.op -eq 'insertrow' -and $null -ne $b.first_table_rows -and $null -ne $Metrics.first_table_rows) { $match = $Metrics.first_table_rows -eq ([int]$b.first_table_rows + 1) }
                if ($Item.op -eq 'mergecells' -and $null -ne $b.first_table_cells -and $null -ne $Metrics.first_table_cells) { $match = $Metrics.first_table_cells -eq ([int]$b.first_table_cells - 1) }
            }
            $checks.table_check = [ordered]@{ kind = $Item.op; baseline_rows = $b.first_table_rows; baseline_cells = $b.first_table_cells; matches = $match }
        }
        if ($Item.op -in @('newchart', 'chartdata')) {
            $expectedTitle = if ($Item.op -eq 'newchart') { 'rsword ' + [char]0x65B0 + [char]0x56FE + [char]0x8868 } else { [string][char]0x6807 + [char]0x9898 + [char]0x5DF2 + [char]0x6539 + ' rsword' }
            $expectedValues = if ($Item.op -eq 'newchart') { @(3, 1, 2) } else { @(11, 21, 31) }
            $selected = @($Charts | Where-Object { $null -ne $_.before -and $_.before.title -eq $expectedTitle })
            $match = $false
            if ($selected.Count -gt 0) {
                $got = @($selected[0].before.first_series_values)
                $match = $got.Count -eq $expectedValues.Count
                if ($match) { for ($v = 0; $v -lt $got.Count; $v++) { if ($got[$v] -ne $expectedValues[$v]) { $match = $false } } }
            }
            $expectedSeriesName = $null
            if ($Item.op -eq 'chartdata') {
                $expectedSeriesName = [string][char]0x6539 + [char]0x540D + [char]0x7CFB + [char]0x5217
                if ($selected.Count -gt 0 -and $selected[0].before.series_name -ne $expectedSeriesName) { $match = $false }
            }
            $checks.chart_check = [ordered]@{ expected_title = $expectedTitle; expected_series_name = $expectedSeriesName; expected_values = $expectedValues; matches = $match; after_matches_baseline = $null; rollback_to_baseline = $null }
            if ($Item.op -eq 'chartdata' -and $selected.Count -gt 0 -and $null -ne $selected[0].after -and @($Baseline.charts).Count -gt 0) {
                $oldValues = @($Baseline.charts)[0].before.first_series_values
                $afterValues = $selected[0].after.first_series_values
                if ($null -ne $oldValues -and $null -ne $afterValues) {
                    $checks.chart_check.after_matches_baseline = (($oldValues | ConvertTo-Json -Compress) -eq ($afterValues | ConvertTo-Json -Compress))
                    $checks.chart_check.rollback_to_baseline = ($selected[0].values_changed_after_activate -eq $true -and $checks.chart_check.after_matches_baseline)
                }
            }
        }
        return $checks
    }

    function Write-EditedReports {
        $orderedRows = @($manifest | ForEach-Object { if ($rows.ContainsKey($_.file)) { $rows[$_.file] } })
        [System.IO.File]::WriteAllText((Join-Path $OutputDirectory 'edited-results.json'), (ConvertTo-Json -InputObject $orderedRows -Depth 40), $utf8)
        $lines = New-Object System.Collections.Generic.List[string]
        $lines.Add('# B2-b edited documents: Word object-model observations')
        $lines.Add('')
        $visibleCount = @($orderedRows | Where-Object { $_.reread_method -eq 'visible_chart_pass' }).Count
        $lines.Add("Recorded $($orderedRows.Count) / $($manifest.Count) generated documents. DisplayAlerts=0; read-only opens. Initial opens were hidden; $visibleCount chart rows were reread using Visible=true and Document.Activate. Repair dialogs and rendering have not been checked in this object-model report. Object-model failures require a separate UI attempt.")
        $lines.Add('')
        $lines.Add([System.Text.RegularExpressions.Regex]::Unescape('| \u6587\u4EF6 | open | compat | \u6062\u590D\u63D0\u793A | marker / shapes / chart \u7684\u8BFB\u6570 | \u62BD\u68C0\uFF1A\u770B\u5230\u4EC0\u4E48\uFF08\u672A\u62BD\u68C0\u5199 -\uFF09 | \u4E0E\u671F\u671B\u662F\u5426\u4E00\u81F4 |'))
        $lines.Add('| --- | --- | --- | --- | --- | --- | --- |')
        foreach ($row in $orderedRows) {
            $readings = [ordered]@{ paragraphs = $row.metrics.paragraphs; tables = $row.metrics.tables; inline_shapes = $row.metrics.inline_shapes; shapes = $row.metrics.shapes; checks = $row.checks; charts = $row.charts }
            if ($row.open -ne 'ok') { $readings.error = $row.error }
            $text = ($readings | ConvertTo-Json -Depth 30 -Compress) -replace '\|', '&#124;' -replace '\r?\n', ' '
            $compat = if ($null -ne $row.metrics) { [string]$row.metrics.compat } else { '-' }
            $lines.Add("| $($row.file) | $($row.open) | $compat | not checked (alerts suppressed) | $text | - | $($row.object_model_assessment); rendering not checked |")
        }
        [System.IO.File]::WriteAllLines((Join-Path $OutputDirectory 'EDITED.md'), $lines, $utf8)
    }

    if ($NoResume) {
        if (Test-Path -LiteralPath $logPath) { throw 'NoResume refuses to overwrite an existing edited-results.jsonl. Choose a new output directory.' }
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
    if ($ChartPass) {
        if ($rows.Count -ne $manifest.Count) { throw 'ChartPass requires a complete 944-row first pass in edited-results.jsonl.' }
        $hiddenSnapshot = Join-Path $OutputDirectory 'edited-results-hidden-pass.json'
        if (-not (Test-Path -LiteralPath $hiddenSnapshot)) {
            $resultsPath = Join-Path $OutputDirectory 'edited-results.json'
            if (-not (Test-Path -LiteralPath $resultsPath)) { throw 'ChartPass requires the original edited-results.json before it can preserve the hidden pass.' }
            [System.IO.File]::Copy($resultsPath, $hiddenSnapshot, $false)
        }
    }
    if (-not (Test-Path -LiteralPath $BaselineRoot)) {
        Write-BatchStage 'extract-baselines' $BaselineZip
        $extractRoot = Split-Path -Parent $BaselineRoot
        [void][System.IO.Directory]::CreateDirectory($extractRoot)
        Expand-Archive -LiteralPath $BaselineZip -DestinationPath $extractRoot -Force
    }
    $oldAlerts = $Word.DisplayAlerts
    $Word.DisplayAlerts = 0
    $processed = 0
    try {
        foreach ($item in $manifest) {
            $script:editedBatchIndex++
            $script:editedBatchFile = [string]$item.file
            if ($ChartPass) {
                if ($item.op -notin @('newchart', 'chartdata')) { continue }
                if ($rows[$item.file].reread_method -eq 'visible_chart_pass') { continue }
            } elseif ($rows.ContainsKey($item.file)) { continue }
            if ($Limit -gt 0 -and $processed -ge $Limit) { break }
            $document = $null
            $row = [ordered]@{
                file = $item.file
                base = $item.base
                op = $item.op
                expect = $item.expect
                timestamp = [DateTime]::Now.ToString('o')
                reread_method = $(if ($ChartPass) { 'visible_chart_pass' } else { $null })
                open_method = $(if ($ChartPass) { 'ReadOnly=true; Visible=true; Document.Activate; DisplayAlerts=0' } else { 'ReadOnly=true; Visible=false; DisplayAlerts=0' })
                hidden_pass_record = $(if ($ChartPass) { $rows[$item.file] } else { $null })
                open = 'error'
                error = $null
                metrics = $null
                baseline = $null
                checks = $null
                charts = @()
                close_error = $null
                repair_prompt = 'not_checked_alerts_suppressed'
                ui_checked = $false
                ui_observation = $null
                object_model_assessment = 'error'
            }
            try {
                Write-BatchStage 'item-start'
                if ($SkipFiles -contains $item.file) { throw $SkipReason }
                $row.baseline = Get-Baseline $item.base
                Write-BatchStage 'document-open'
                $document = Open-BatchDocument (Join-Path $InputDirectory $item.file) ([bool]$ChartPass)
                $row.open = 'ok'
                if ($ChartPass) { Write-BatchStage 'document-activate'; [void]$document.Activate() }
                Write-BatchStage 'document-metrics'
                $row.metrics = Get-DocumentMetrics $document
                if ($item.op -in @('newchart', 'chartdata')) {
                    $row.charts = Get-DocumentCharts $document ($item.op -eq 'chartdata')
                    if (@($row.charts).Count -eq 0) {
                        try { throw 'No chart objects were collected from a chart-edit document; chart validation is incomplete.' }
                        catch { $row.charts = @([ordered]@{ collection = 'document'; index = $null; error = (Get-RawError $_) }) }
                    }
                }
                Write-BatchStage 'document-checks'
                $row.checks = Get-Checks $document $item $row.metrics $row.baseline $row.charts
                $activeChecks = @($row.checks.marker, $row.checks.shape_check, $row.checks.chart_check, $row.checks.table_check | Where-Object { $null -ne $_ })
                $failedChecks = @($activeChecks | Where-Object { $null -ne $_.matches -and $_.matches -eq $false })
                $unknownChecks = @($activeChecks | Where-Object { $null -eq $_.matches })
                $chartErrors = @($row.charts | Where-Object {
                    $null -ne $_.error -or $null -ne $_.activation_error -or $null -ne $_.workbook_close_error -or
                    ($null -ne $_.before -and $_.before.errors.Count -gt 0) -or
                    ($null -ne $_.after -and $_.after.errors.Count -gt 0)
                })
                $row.object_model_assessment = 'pass'
                if ($row.metrics.compat -ne 15 -or $failedChecks.Count -gt 0) { $row.object_model_assessment = 'mismatch' }
                if ($unknownChecks.Count -gt 0 -or $row.metrics.errors.Count -gt 0 -or $row.checks.errors.Count -gt 0 -or $chartErrors.Count -gt 0 -or $row.baseline.open -ne 'ok') { $row.object_model_assessment = 'incomplete' }
            } catch {
                $row.error = Get-RawError $_
                $row.object_model_assessment = 'error'
            } finally {
                if ($null -ne $document) {
                    try { Write-BatchStage 'document-close'; [void]$document.Close(0) }
                    catch { $row.close_error = Get-RawError $_; $row.object_model_assessment = 'incomplete' }
                }
            }
            Write-BatchStage 'record-result'
            $rows[$item.file] = $row
            [System.IO.File]::AppendAllText($logPath, (($row | ConvertTo-Json -Depth 40 -Compress) + [Environment]::NewLine), $utf8)
            $processed++
            if (($processed % 25) -eq 0) {
                Write-Host ("B2-b {0}/{1}; processed this pass={5}: {2}; {3}; elapsed={4}s" -f $rows.Count, $manifest.Count, $item.file, $row.object_model_assessment, [Math]::Round($clock.Elapsed.TotalSeconds, 1), $processed)
                Write-EditedReports
            }
        }
    } finally {
        Write-BatchStage 'write-reports'
        Write-EditedReports
        try { $Word.DisplayAlerts = $oldAlerts } catch { Write-Warning $_.Exception.Message }
    }
    Write-BatchStage 'complete' "$($rows.Count)/$($manifest.Count) recorded"
    return [pscustomobject]@{ recorded = $rows.Count; total = $manifest.Count; processed_this_run = $processed; json = (Join-Path $OutputDirectory 'edited-results.json'); markdown = (Join-Path $OutputDirectory 'EDITED.md'); seconds = [Math]::Round($clock.Elapsed.TotalSeconds, 1) }
}
