# Shared read-only Word collectors for round 3. The calling function owns $Word.
# No collector saves documents or quits Word/Excel.

function Get-E3RawError($Record) {
    return [ordered]@{
        message = $Record.Exception.Message
        exception = $Record.Exception.ToString()
        error_record = ($Record | Out-String).TrimEnd()
        hresult = $Record.Exception.HResult
        category = [string]$Record.CategoryInfo
        fully_qualified_error_id = $Record.FullyQualifiedErrorId
    }
}

function Add-E3ReadError($Target, [string] $Stage, $Record) {
    [void]$Target.errors.Add([ordered]@{ stage = $Stage; error = (Get-E3RawError $Record) })
}

function Open-E3BatchDocument([string] $Path, [bool] $Visible = $false) {
    $missing = [Type]::Missing
    return $Word.Documents.Open($Path, $false, $true, $false, $missing, $missing, $false, $missing, $missing, $missing, $missing, $Visible)
}

function Get-E3DocumentMetrics($Document) {
    $metrics = [ordered]@{
        compat = $null
        paragraphs = $null
        tables = $null
        comments = $null
        inline_shapes = $null
        shapes = $null
        first_table_rows = $null
        first_table_cells = $null
        errors = (New-Object System.Collections.ArrayList)
    }
    foreach ($property in @('compat', 'paragraphs', 'tables', 'comments', 'inline_shapes', 'shapes')) {
        try {
            $collection = $null
            $value = $null
            switch ($property) {
                'compat' { $value = $Document.CompatibilityMode }
                'paragraphs' { $collection = $Document.Paragraphs }
                'tables' { $collection = $Document.Tables }
                'comments' { $collection = $Document.Comments }
                'inline_shapes' { $collection = $Document.InlineShapes }
                'shapes' { $collection = $Document.Shapes }
            }
            if ($property -ne 'compat') {
                if ($null -eq $collection) { throw "Word returned no collection for $property; the property getter may have failed." }
                $value = $collection.Count
            }
            if ($null -eq $value) { throw "Word returned no value for $property; the property getter may have failed." }
            $metrics[$property] = [int]$value
        } catch { Add-E3ReadError $metrics "metrics.$property" $_ }
    }
    if ($metrics.tables -gt 0) {
        try {
            $tableRows = $Document.Tables.Item(1).Rows
            if ($null -eq $tableRows) { throw 'Word returned no Rows collection; vertically merged cells can make row access unavailable.' }
            $count = $tableRows.Count
            if ($null -eq $count) { throw 'Word returned no Rows.Count value.' }
            $metrics.first_table_rows = [int]$count
        } catch { Add-E3ReadError $metrics 'metrics.first_table_rows' $_ }
        try {
            $tableCells = $Document.Tables.Item(1).Range.Cells
            if ($null -eq $tableCells) { throw 'Word returned no Cells collection for the first table.' }
            $count = $tableCells.Count
            if ($null -eq $count) { throw 'Word returned no Cells.Count value.' }
            $metrics.first_table_cells = [int]$count
        } catch { Add-E3ReadError $metrics 'metrics.first_table_cells' $_ }
    }
    return $metrics
}

function Convert-E3ChartValues($Values) {
    $result = New-Object System.Collections.ArrayList
    foreach ($value in @($Values)) {
        if ($null -eq $value) { [void]$result.Add($null) }
        elseif ($value -is [ValueType]) { [void]$result.Add($value) }
        else { [void]$result.Add([string]$value) }
    }
    return ,$result.ToArray()
}

function Get-E3OneChartReadings($Chart) {
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
    } catch { Add-E3ReadError $reading 'chart.title' $_ }
    try {
        $name = $Chart.SeriesCollection(1).Name
        if ($null -eq $name) { throw 'Chart first-series Name returned null.' }
        $reading.series_name = [string]$name
    } catch { Add-E3ReadError $reading 'chart.series_name' $_ }
    try {
        $values = $Chart.SeriesCollection(1).Values
        if ($null -eq $values) { throw 'Chart first-series Values returned null.' }
        $reading.first_series_values = Convert-E3ChartValues $values
    } catch { Add-E3ReadError $reading 'chart.first_series_values' $_ }
    return $reading
}

function Get-E3DocumentCharts($Document, [bool] $ActivateData) {
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
            [void]$result.Add([ordered]@{ collection = $kind; index = $null; error = (Get-E3RawError $_) })
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
                    catch { $detectionError = Get-E3RawError $_ }
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
                    before = (Get-E3OneChartReadings $chart)
                    activation = 'not_requested'
                    activation_error = $null
                    workbook_name = $null
                    workbook_first_sheet = $null
                    workbook_used_range_values = $null
                    workbook_read_error = $null
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
                        try {
                            $sheet = $workbook.Worksheets.Item(1)
                            $entry.workbook_first_sheet = [string]$sheet.Name
                            $cells = $sheet.UsedRange.Value2
                            $grid = New-Object System.Collections.ArrayList
                            if ($cells -is [Array] -and $cells.Rank -eq 2) {
                                for ($r = $cells.GetLowerBound(0); $r -le $cells.GetUpperBound(0); $r++) {
                                    $values = New-Object System.Collections.ArrayList
                                    for ($c = $cells.GetLowerBound(1); $c -le $cells.GetUpperBound(1); $c++) { [void]$values.Add($cells.GetValue($r, $c)) }
                                    [void]$grid.Add($values.ToArray())
                                }
                                $entry.workbook_used_range_values = $grid.ToArray()
                            } else { $entry.workbook_used_range_values = @($cells) }
                        } catch { $entry.workbook_read_error = Get-E3RawError $_ }
                    } catch {
                        $entry.activation = 'error'
                        $entry.activation_error = Get-E3RawError $_
                    } finally {
                        if ($null -ne $workbook) {
                            try {
                                Write-BatchStage 'chartdata-close-workbook' "$kind[$i]"
                                [void]$workbook.Close($false)
                            } catch { $entry.workbook_close_error = Get-E3RawError $_ }
                        }
                    }
                    Write-BatchStage 'chartdata-reread' "$kind[$i]"
                    $entry.after = Get-E3OneChartReadings $chart
                    if ($null -ne $entry.before.first_series_values -and $null -ne $entry.after.first_series_values) {
                        $entry.values_changed_after_activate = (($entry.before.first_series_values | ConvertTo-Json -Compress) -ne ($entry.after.first_series_values | ConvertTo-Json -Compress))
                    }
                }
                [void]$result.Add($entry)
            } catch {
                [void]$result.Add([ordered]@{ collection = $kind; index = $i; error = (Get-E3RawError $_) })
            }
        }
    }
    return ,$result.ToArray()
}

function Compare-E3Number($Actual, $Expected) {
    if ($null -eq $Actual -or $null -eq $Expected) { return $null }
    return ([int]$Actual -eq [int]$Expected)
}

function Get-E3Checks($Document, $Item, $Metrics, $Baseline, $Charts) {
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
                $checks.marker = [ordered]@{ kind = 'last_section_primary_header'; actual = $header; expected = $expected; matches = $header.Trim([char[]]@(13, 7, 10, 32)).Equals($expected, [StringComparison]::Ordinal) }
            }
            'comment' {
                $authors = @()
                for ($c = 1; $c -le $Document.Comments.Count; $c++) { $authors += [string]$Document.Comments.Item($c).Author }
                $expectedCount = if ($null -ne $b -and $null -ne $b.comments) { [int]$b.comments + 1 } else { $null }
                $checks.marker = [ordered]@{ kind = 'comment_author_and_count_plus_one'; count = [int]$Document.Comments.Count; expected_count = $expectedCount; authors = $authors; matches = $(if ($null -eq $expectedCount) { $null } else { $Document.Comments.Count -eq $expectedCount -and $authors -contains 'rsword' }) }
            }
            'split' {
                $expected = $null
                if ($null -ne $b -and $null -ne $b.paragraphs) { $expected = [int]$b.paragraphs + 1 }
                $checks.marker = [ordered]@{ kind = 'paragraph_count_plus_one'; baseline = $b.paragraphs; actual = $Metrics.paragraphs; expected = $expected; matches = (Compare-E3Number $Metrics.paragraphs $expected) }
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
    } catch { Add-E3ReadError $checks 'marker' $_ }
    if ($Item.op -in @('newimage', 'ink', 'newchart', 'replaceimage')) {
        $expectedInline = $null
        $expectedFloating = $null
        if ($null -ne $b) {
            if ($null -ne $b.inline_shapes) { $expectedInline = [int]$b.inline_shapes }
            if ($null -ne $b.shapes) { $expectedFloating = [int]$b.shapes }
            if ($Item.op -eq 'newchart' -and $null -ne $expectedInline) { $expectedInline++ }
            if ($Item.op -in @('newimage', 'ink') -and $null -ne $expectedFloating) { $expectedFloating++ }
        }
        $inlineMatches = Compare-E3Number $Metrics.inline_shapes $expectedInline
        $floatingMatches = Compare-E3Number $Metrics.shapes $expectedFloating
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


function Resolve-E3BaselinePath {
    param([string] $RelativePath, [string] $BaselineRoot, [string] $Round2Root)
    if ([IO.Path]::IsPathRooted($RelativePath) -or $RelativePath -match '(^|[\\/])\.\.([\\/]|$)') { throw "Unsafe manifest base: $RelativePath" }
    $candidates = @(@(
        (Join-Path $BaselineRoot $RelativePath),
        (Join-Path $Round2Root $RelativePath)
    ) | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf })
    if (@($candidates).Count -eq 0) { throw "No original baseline exists for: $RelativePath" }
    if (@($candidates).Count -gt 1) { throw "Ambiguous baseline: $RelativePath; resolve explicitly before running." }
    return [string]$candidates[0]
}

function Get-E3ShapeGeometry {
    param([object] $Document)
    $out = New-Object System.Collections.ArrayList
    foreach ($kind in @('InlineShapes', 'Shapes')) {
        $collection = $Document.$kind
        for ($i = 1; $i -le $collection.Count; $i++) {
            $shape = $collection.Item($i)
            $r = [ordered]@{ collection = $kind; index = $i; type = $null; width = $null; height = $null; name = $null; left = $null; top = $null; relative_horizontal = $null; relative_vertical = $null; wrap_type = $null; z_order = $null; errors = (New-Object System.Collections.ArrayList) }
            foreach ($property in @('type', 'width', 'height')) {
                try { $r[$property] = $shape.$property } catch { Add-E3ReadError $r "geometry.$property" $_ }
            }
            if ($kind -eq 'Shapes') {
                foreach ($pair in @(@('name','Name'), @('left','Left'), @('top','Top'), @('relative_horizontal','RelativeHorizontalPosition'), @('relative_vertical','RelativeVerticalPosition'), @('z_order','ZOrderPosition'))) {
                    try { $r[$pair[0]] = $shape.($pair[1]) } catch { Add-E3ReadError $r "geometry.$($pair[0])" $_ }
                }
                try { $r.wrap_type = $shape.WrapFormat.Type } catch { Add-E3ReadError $r 'geometry.wrap_type' $_ }
            }
            [void]$out.Add($r)
        }
    }
    return ,$out.ToArray()
}
