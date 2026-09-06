#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$OutputRoot = (Split-Path -Parent $PSScriptRoot),
    [string[]]$Only = @(),
    [switch]$Visible,
    [switch]$SkipExisting
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# All document bytes are emitted by Word. No package or XML editing is performed.
$specs = @(
    @{ Name = 'chart-column'; Type = 51; Kind = 'clustered column' },
    @{ Name = 'chart-bar'; Type = 57; Kind = 'clustered bar' },
    @{ Name = 'chart-stacked'; Type = 52; Kind = 'stacked column' },
    @{ Name = 'chart-percent-stacked'; Type = 53; Kind = '100 percent stacked column' },
    @{ Name = 'chart-line'; Type = 65; Kind = 'line with markers' },
    @{ Name = 'chart-line-plain'; Type = 4; Kind = 'line without markers' },
    @{ Name = 'chart-pie'; Type = 5; Kind = 'pie'; SingleSeries = $true },
    @{ Name = 'chart-doughnut'; Type = -4120; Kind = 'doughnut'; HoleSize = 30 },
    @{ Name = 'chart-area'; Type = 1; Kind = 'area' },
    @{ Name = 'chart-scatter'; Type = -4169; Kind = 'scatter with markers only'; XY = $true },
    @{ Name = 'chart-scatter-lines'; Type = 72; Kind = 'scatter with smooth lines and markers'; XY = $true },
    @{ Name = 'chart-bubble'; Type = 15; Kind = 'bubble'; XY = $true; Bubble = $true },
    @{ Name = 'chart-combo'; Type = 51; Kind = 'column plus line on secondary axis'; Combo = $true },
    @{ Name = 'chart-3d'; Type = 54; Kind = '3D clustered column' },
    @{ Name = 'chart-dates'; Type = 4; Kind = 'line with date categories'; Dates = $true },
    @{ Name = 'chart-style'; Type = 51; Kind = 'clustered column'; Style = 14; Color = 14; Palette = 'monochromatic accent 1' },
    @{ Name = 'chart-style-gray'; Type = 51; Kind = 'clustered column'; Style = 14; Color = 26; Palette = 'grayscale'; Gray = $true },
    @{ Name = 'chart-point-color'; Type = 5; Kind = 'pie with red second sector'; SingleSeries = $true; RedPoint = $true },
    @{ Name = 'chart-legend'; Type = 51; Kind = 'clustered column'; LegendTop = $true },
    @{ Name = 'chart-no-legend'; Type = 51; Kind = 'clustered column'; NoLegend = $true },
    @{ Name = 'chart-no-title'; Type = 51; Kind = 'clustered column'; NoTitle = $true },
    @{ Name = 'chart-floating'; Type = 51; Kind = 'clustered column'; Floating = $true },
    @{ Name = 'chart-in-table'; Type = 51; Kind = 'clustered column'; InTable = $true },
    @{ Name = 'chartex-sunburst'; Type = 120; Kind = 'sunburst'; ChartEx = $true },
    @{ Name = 'chartex-treemap'; Type = 117; Kind = 'treemap'; ChartEx = $true },
    @{ Name = 'chartex-waterfall'; Type = 119; Kind = 'waterfall'; ChartEx = $true },
    @{ Name = 'chartex-histogram'; Type = 118; Kind = 'histogram'; ChartEx = $true },
    @{ Name = 'chartex-boxwhisker'; Type = 121; Kind = 'box and whisker'; ChartEx = $true },
    @{ Name = 'chartex-funnel'; Type = 123; Kind = 'funnel'; ChartEx = $true }
)

if ($Only.Count -gt 0) {
    $unknown = @($Only | Where-Object { $_ -notin $specs.Name })
    if ($unknown.Count -gt 0) { throw ('Unknown chart names: ' + ($unknown -join ', ')) }
    $specs = @($specs | Where-Object { $_.Name -in $Only })
}

function Get-UniquePath {
    param([string]$Directory, [string]$BaseName, [string]$Extension)
    $candidate = Join-Path $Directory ($BaseName + $Extension)
    $suffix = 2
    while (Test-Path -LiteralPath $candidate) {
        $candidate = Join-Path $Directory ($BaseName + '-' + $suffix + $Extension)
        $suffix++
    }
    return $candidate
}

function Get-ComProperty {
    param([scriptblock]$Read, [string]$Label, [System.Collections.Generic.List[string]]$Warnings)
    try { return (& $Read) }
    catch { $Warnings.Add($Label + ': ' + $_.Exception.Message); return $null }
}

function Write-RunLog {
    $run.UpdatedUtc = [DateTime]::UtcNow.ToString('o')
    $run | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $logPath -Encoding UTF8
}

function Get-ActivatedWorkbook {
    param($Chart, [System.Collections.Generic.List[string]]$Warnings)
    $lastError = 'Workbook was null.'
    for ($attempt = 1; $attempt -le 20; $attempt++) {
        try {
            [void]$Chart.ChartData.Activate()
            Start-Sleep -Milliseconds 500
            $candidate = $Chart.ChartData.Workbook
            if ($null -ne $candidate -and $candidate.Worksheets.Count -gt 0) {
                if ($attempt -gt 1) { $Warnings.Add('ChartData.Workbook became available after activation attempt ' + $attempt) }
                return $candidate
            }
        }
        catch { $lastError = $_.Exception.Message }
        Start-Sleep -Milliseconds 500
    }
    throw ('ChartData workbook did not become available after 20 activation attempts: ' + $lastError)
}

function Read-ZipXml {
    param($Archive, [string]$Name)
    $part = $Archive.GetEntry($Name)
    if ($null -eq $part) { throw ('Missing package part: ' + $Name) }
    $stream = $part.Open()
    try {
        $xml = [xml]::new()
        $xml.Load($stream)
        return $xml
    }
    finally { $stream.Dispose() }
}

function Test-ChartPackage {
    param([string]$Path, [hashtable]$Spec)
    $archive = [IO.Compression.ZipFile]::OpenRead($Path)
    try {
        $names = @($archive.Entries | ForEach-Object { $_.FullName })
        $pattern = $(if ($Spec.ChartEx) { '^word/charts/chartEx[0-9]+\.xml$' } else { '^word/charts/chart[0-9]+\.xml$' })
        $chartParts = @($names | Where-Object { $_ -match $pattern })
        $workbooks = @($names | Where-Object { $_ -match '^word/embeddings/.*\.xlsx$' })
        $failures = [System.Collections.Generic.List[string]]::new()
        if ($chartParts.Count -ne 1) { $failures.Add('Expected exactly one matching chart part.') }
        if ($workbooks.Count -ne 1) { $failures.Add('Expected exactly one embedded xlsx workbook.') }
        $externalData = $false
        $chartRelationships = $false
        foreach ($partName in $chartParts) {
            $chartXml = Read-ZipXml $archive $partName
            $externalData = $null -ne $chartXml.SelectSingleNode('//*[local-name()="externalData"]')
            $relsName = 'word/charts/_rels/' + [IO.Path]::GetFileName($partName) + '.rels'
            $chartRelationships = $names -contains $relsName
            if (-not $externalData) { $failures.Add('Chart part has no externalData element.') }
            if (-not $chartRelationships) { $failures.Add('Chart relationships part is missing.') }
            if (-not $Spec.ChartEx) {
                $cachedSeries = @($chartXml.SelectNodes('//*[local-name()="ser"]'))
                $expectedCount = $(if ($Spec.SingleSeries) { 1 } else { 2 })
                if ($cachedSeries.Count -ne $expectedCount) { $failures.Add('Saved chart has an unexpected series count.') }
                for ($seriesIndex = 0; $seriesIndex -lt $cachedSeries.Count; $seriesIndex++) {
                    $cacheValues = @($cachedSeries[$seriesIndex].SelectNodes('./*[local-name()="val" or local-name()="yVal"]//*[local-name()="pt"]/*[local-name()="v"]'))
                    if ($cacheValues.Count -ne 3) { $failures.Add('Saved chart series does not have three cached values.'); continue }
                    for ($valueIndex = 0; $valueIndex -lt 3; $valueIndex++) {
                        $expectedValue = ($valueIndex + 1) * 10 + $seriesIndex * 5
                        if ([double]$cacheValues[$valueIndex].InnerText -ne $expectedValue) {
                            $failures.Add('Saved chart cache differs from the requested numeric data.')
                        }
                    }
                }
            }
        }
        $documentXml = Read-ZipXml $archive 'word/document.xml'
        $chartReference = $null -ne $documentXml.SelectSingleNode('//*[local-name()="chart" and @*[local-name()="id"]]')
        if (-not $chartReference) { $failures.Add('Document has no chart relationship reference.') }
        $fallbackImage = $false
        if ($Spec.ChartEx) {
            $fallbackImage = $null -ne $documentXml.SelectSingleNode('//*[local-name()="AlternateContent"]/*[local-name()="Fallback"]//*[local-name()="blip"]')
            if (-not $fallbackImage) { $failures.Add('Modern chart has no AlternateContent fallback image.') }
        }
        return [ordered]@{
            Method = 'ZipFile.OpenRead; XML parsed read-only; original package never rewritten'
            Passed = $failures.Count -eq 0
            ChartParts = $chartParts
            EmbeddedWorkbooks = $workbooks
            ExternalData = $externalData
            ChartRelationships = $chartRelationships
            ChartReference = $chartReference
            FallbackImage = $fallbackImage
            Failures = @($failures)
        }
    }
    finally { $archive.Dispose() }
}

function Set-ChartWorkbook {
    param($Chart, $Sheet, [hashtable]$Spec)
    [void]$Sheet.Cells.Clear()
    $Sheet.Cells.Item(1, 1).Value2 = 'Category'
    $Sheet.Cells.Item(1, 2).Value2 = 'Series 1'
    $Sheet.Cells.Item(1, 3).Value2 = 'Series 2'
    for ($row = 2; $row -le 4; $row++) {
        $index = $row - 1
        if ($Spec.Dates) {
            $date = [DateTime]::new(2024, $index, 1)
            $Sheet.Cells.Item($row, 1).Value2 = [double]$date.ToOADate()
        }
        elseif ($Spec.XY) { $Sheet.Cells.Item($row, 1).Value2 = [double]$index }
        else { $Sheet.Cells.Item($row, 1).Value2 = 'Category ' + $index }
        $Sheet.Cells.Item($row, 2).Value2 = [double]($index * 10)
        $Sheet.Cells.Item($row, 3).Value2 = [double]($index * 10 + 5)
    }
    if ($Spec.Dates) { $Sheet.Range('A2:A4').NumberFormat = 'm/d/yyyy' }

    if ($Spec.SingleSeries) {
        [void]$Sheet.Range('C1:C4').Clear()
        $sourceRange = $Sheet.Range('A1:B4')
    }
    else { $sourceRange = $Sheet.Range('A1:C4') }
    # Word rejects Excel's workbook-qualified address here; the sheet is local.
    $sourceAddress = $Sheet.Name + '!' + $sourceRange.Address($true, $true, 1, $false)
    [void]$Chart.SetSourceData([string]$sourceAddress, 2)

    if ($Spec.XY) {
        $seriesCollection = $Chart.SeriesCollection()
        while ($seriesCollection.Count -gt 0) { [void]$seriesCollection.Item(1).Delete() }
        if ($Spec.Bubble) {
            $Sheet.Cells.Item(1, 4).Value2 = 'Size 1'
            $Sheet.Cells.Item(1, 5).Value2 = 'Size 2'
            for ($row = 2; $row -le 4; $row++) {
                $Sheet.Cells.Item($row, 4).Value2 = [double](($row - 1) * 3)
                $Sheet.Cells.Item($row, 5).Value2 = [double](($row - 1) * 3 + 1)
            }
        }
        for ($index = 1; $index -le 2; $index++) {
            $series = $seriesCollection.NewSeries()
            $column = if ($index -eq 1) { 'B' } else { 'C' }
            $seriesSheet = '=' + $Sheet.Name + '!'
            $series.Name = $seriesSheet + '$' + $column + '$1'
            $series.XValues = $seriesSheet + '$A$2:$A$4'
            $series.Values = $seriesSheet + '$' + $column + '$2:$' + $column + '$4'
            if ($Spec.Bubble) {
                $sizeColumn = if ($index -eq 1) { 'D' } else { 'E' }
                $series.BubbleSizes = $seriesSheet + '$' + $sizeColumn + '$2:$' + $sizeColumn + '$4'
            }
        }
    }
    [void]$Sheet.Application.Calculate()
}

function Get-ChartSnapshot {
    param($Chart, [hashtable]$Spec, [System.Collections.Generic.List[string]]$Warnings)
    $snapshot = [ordered]@{
        ChartType = Get-ComProperty { [int]$Chart.ChartType } 'ChartType' $Warnings
        ChartStyle = Get-ComProperty { [int]$Chart.ChartStyle } 'ChartStyle' $Warnings
        ChartColor = Get-ComProperty { [int]$Chart.ChartColor } 'ChartColor' $Warnings
        HasTitle = Get-ComProperty { [bool]$Chart.HasTitle } 'HasTitle' $Warnings
        Title = $null
        HasLegend = Get-ComProperty { [bool]$Chart.HasLegend } 'HasLegend' $Warnings
        LegendPosition = $null
        Series = @()
    }
    if ($snapshot.HasTitle) {
        $snapshot.Title = Get-ComProperty { [string]$Chart.ChartTitle.Text } 'Title' $Warnings
    }
    if ($snapshot.HasLegend) {
        $snapshot.LegendPosition = Get-ComProperty { [int]$Chart.Legend.Position } 'LegendPosition' $Warnings
    }
    $count = Get-ComProperty { [int]$Chart.SeriesCollection().Count } 'SeriesCount' $Warnings
    for ($index = 1; $index -le $count; $index++) {
        $series = $Chart.SeriesCollection($index)
        $item = [ordered]@{
            Name = Get-ComProperty { [string]$series.Name } ('Series ' + $index + ' name') $Warnings
            Values = @(Get-ComProperty { $series.Values } ('Series ' + $index + ' values') $Warnings)
            XValues = @(Get-ComProperty { $series.XValues } ('Series ' + $index + ' categories') $Warnings)
            ChartType = Get-ComProperty { [int]$series.ChartType } ('Series ' + $index + ' ChartType') $Warnings
            AxisGroup = Get-ComProperty { [int]$series.AxisGroup } ('Series ' + $index + ' axis') $Warnings
            FillRgb = Get-ComProperty { [int]$series.Format.Fill.ForeColor.RGB } ('Series ' + $index + ' fill') $Warnings
        }
        if ($Spec.XY -and -not $Spec.Bubble) {
            $item.MarkerStyle = Get-ComProperty { [int]$series.MarkerStyle } ('Series ' + $index + ' marker') $Warnings
            $item.Smooth = Get-ComProperty { [bool]$series.Smooth } ('Series ' + $index + ' smooth') $Warnings
        }
        if ($Spec.Bubble) {
            $item.BubbleSizes = Get-ComProperty { $series.BubbleSizes } ('Series ' + $index + ' bubble sizes') $Warnings
        }
        $snapshot.Series += $item
    }
    if ($Spec.HoleSize) {
        $snapshot.DoughnutHoleSize = Get-ComProperty { [int]$Chart.ChartGroups(1).DoughnutHoleSize } 'DoughnutHoleSize' $Warnings
    }
    if ($Spec.RedPoint) {
        $snapshot.SecondPointRgb = Get-ComProperty { [int]$Chart.SeriesCollection(1).Points(2).Format.Fill.ForeColor.RGB } 'SecondPointRgb' $Warnings
    }
    if ($Spec.Dates) {
        $snapshot.DateCategoryType = Get-ComProperty { [int]$Chart.Axes(1, 1).CategoryType } 'DateCategoryType' $Warnings
        $snapshot.DateNumberFormat = Get-ComProperty { [string]$Chart.Axes(1, 1).TickLabels.NumberFormat } 'DateNumberFormat' $Warnings
    }
    return $snapshot
}

$OutputRoot = [IO.Path]::GetFullPath($OutputRoot)
$chartDirectory = Join-Path $OutputRoot 'chart'
if ($SkipExisting) {
    $specs = @($specs | Where-Object { -not (Test-Path -LiteralPath (Join-Path $chartDirectory ($_.Name + '.docx'))) })
}
$logDirectory = Join-Path $OutputRoot '_scripts'
$previewDirectory = Join-Path $OutputRoot '_previews/chart'
[void][IO.Directory]::CreateDirectory($chartDirectory)
[void][IO.Directory]::CreateDirectory($logDirectory)
[void][IO.Directory]::CreateDirectory($previewDirectory)
Add-Type -AssemblyName System.IO.Compression.FileSystem
Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class ChartWordProcess { [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId); }'
$logPath = Get-UniquePath $logDirectory ('create-charts-log-' + (Get-Date -Format 'yyyyMMdd-HHmmss')) '.json'
$title = -join ([char[]]@(0x9500, 0x552E, 0x7EDF, 0x8BA1))
$before = 'before ' + (-join ([char[]]@(0x524D, 0x6587)))
$after = 'after ' + (-join ([char[]]@(0x540E, 0x6587)))
$entries = [System.Collections.Generic.List[object]]::new()
$run = [ordered]@{
    Script = 'create-charts.ps1'
    StartedUtc = [DateTime]::UtcNow.ToString('o')
    UpdatedUtc = $null
    CompletedUtc = $null
    OutputRoot = $OutputRoot
    WordVersion = $null
    WordBuild = $null
    WordProductCode = $null
    WordProcessId = $null
    WindowsVersion = [Environment]::OSVersion.VersionString
    PowerShellVersion = $PSVersionTable.PSVersion.ToString()
    SaveFormat = 12
    Method = 'Word COM; Documents.Add; InlineShapes.AddChart2; ChartData.Workbook; SaveAs2 once'
    ObservationStatus = 'COM property readback only. Visual observation and copied-package self-check still required.'
    Entries = $entries
    FatalError = $null
}
$word = $null
try {
    $word = New-Object -ComObject Word.Application
    $word.Visible = [bool]$Visible
    $word.DisplayAlerts = 0
    $word.Options.SaveNormalPrompt = $false
    $run.WordVersion = [string]$word.Version
    $run.WordBuild = [string]$word.Build
    $run.WordProductCode = [string]$word.ProductCode()
    Write-RunLog

    foreach ($spec in $specs) {
        # Explicit defaults allow StrictMode while keeping the sample table concise.
        foreach ($key in @('SingleSeries', 'HoleSize', 'XY', 'Bubble', 'Combo', 'Dates', 'Style', 'Color', 'Palette', 'Gray', 'RedPoint', 'LegendTop', 'NoLegend', 'NoTitle', 'Floating', 'InTable', 'ChartEx')) {
            if (-not $spec.ContainsKey($key)) { $spec[$key] = $null }
        }
        $destination = Get-UniquePath $chartDirectory $spec.Name '.docx'
        $warnings = [System.Collections.Generic.List[string]]::new()
        $entry = [ordered]@{
            RequestedFile = 'chart/' + $spec.Name + '.docx'
            File = 'chart/' + [IO.Path]::GetFileName($destination)
            Requested = $spec.Clone()
            RequestedTitle = $(if ($spec.NoTitle) { $null } else { $title })
            Worksheet = [ordered]@{
                Categories = $(if ($spec.Dates) { @('1/1/2024', '2/1/2024', '3/1/2024') } elseif ($spec.XY) { @(1, 2, 3) } else { @('Category 1', 'Category 2', 'Category 3') })
                Series1 = @{ Name = 'Series 1'; Values = @(10, 20, 30) }
                Series2 = $(if ($spec.SingleSeries) { $null } else { @{ Name = 'Series 2'; Values = @(15, 25, 35) } })
                BubbleSizes = $(if ($spec.Bubble) { @(@(3, 6, 9), @(4, 7, 10)) } else { $null })
            }
            Status = 'creating'
            Stage = 'Documents.Add'
            StartedUtc = [DateTime]::UtcNow.ToString('o')
            FinishedUtc = $null
            Actual = $null
            Warnings = $warnings
            Error = $null
            SavedOnce = $false
            Bytes = $null
            PackageCheck = $null
            PreviewFile = $null
            PreviewStatus = 'not-exported'
        }
        $entries.Add($entry)
        Write-RunLog
        $document = $null
        $workbook = $null
        $sheet = $null
        $chart = $null
        $inline = $null
        $shape = $null
        $table = $null
        try {
            $document = $word.Documents.Add()
            $ownedWordProcess = [uint32]0
            [void][ChartWordProcess]::GetWindowThreadProcessId([IntPtr]$document.ActiveWindow.Hwnd, [ref]$ownedWordProcess)
            $run.WordProcessId = $ownedWordProcess
            [void]$document.SetCompatibilityMode(15)
            $document.Content.Text = $before + "`r`r" + $after + "`r"
            $document.Content.Font.Size = 11
            $range = $document.Paragraphs.Item(2).Range.Duplicate
            [void]$range.Collapse(1)
            if ($spec.InTable) {
                $table = $document.Tables.Add($range, 2, 2)
                $table.AllowAutoFit = $false
                $table.Columns.Width = 220
                $table.Borders.Enable = 1
                $table.Cell(1, 2).Range.Text = 'Cell 2'
                $table.Cell(2, 1).Range.Text = 'Cell 3'
                $table.Cell(2, 2).Range.Text = 'Cell 4'
                $range = $table.Cell(1, 1).Range.Duplicate
                [void]$range.Collapse(1)
            }

            $entry.Stage = 'InlineShapes.AddChart2'
            try { $inline = $document.InlineShapes.AddChart2(-1, [int]$spec.Type, $range, $false) }
            catch {
                if (-not $spec.ChartEx) { throw }
                $warnings.Add('Direct AddChart2 failed: ' + $_.Exception.Message)
                $entry.Stage = 'modern chart fallback: create column then set ChartType'
                Write-RunLog
                $inline = $document.InlineShapes.AddChart2(-1, 51, $range, $false)
                $inline.Chart.ChartType = [int]$spec.Type
            }
            $inline.Width = $(if ($spec.InTable) { 205 } elseif ($spec.Floating) { 285 } else { 420 })
            $inline.Height = $(if ($spec.InTable) { 155 } else { 240 })
            $chart = $inline.Chart
            if ([int]$chart.ChartType -ne [int]$spec.Type) {
                throw ('AddChart2 returned type ' + $chart.ChartType + ' instead of ' + $spec.Type)
            }
            $entry.Stage = 'ChartData.Activate and Workbook'
            Write-RunLog
            $workbook = Get-ActivatedWorkbook $chart $warnings
            if (-not $Visible) {
                $word.Visible = $false
            }
            $sheet = $workbook.Worksheets.Item(1)
            $entry.Stage = 'populate worksheet and SetSourceData'
            Write-RunLog
            Set-ChartWorkbook $chart $sheet $spec
            $entry.Stage = 'close embedded workbook with changes'
            Write-RunLog
            [void]$workbook.Close($true)
            $workbook = $null
            $entry.Stage = 'configure chart properties'
            Write-RunLog
            $chart.HasTitle = -not [bool]$spec.NoTitle
            if (-not $spec.NoTitle) { $chart.ChartTitle.Text = $title }
            $chart.HasLegend = -not [bool]$spec.NoLegend
            if (-not $spec.NoLegend) {
                $chart.Legend.Position = $(if ($spec.LegendTop) { -4160 } else { -4152 })
            }
            if ($spec.Style) { $chart.ChartStyle = [int]$spec.Style }
            $chart.ChartColor = $(if ($spec.Color) { [int]$spec.Color } else { 10 })
            if ($spec.Gray) {
                # Pin the rendered colors as well as selecting the grayscale palette.
                $chart.SeriesCollection(1).Format.Fill.ForeColor.RGB = 0x595959
                $chart.SeriesCollection(2).Format.Fill.ForeColor.RGB = 0xA6A6A6
            }
            if ($spec.HoleSize) { $chart.ChartGroups(1).DoughnutHoleSize = [int]$spec.HoleSize }
            if ($spec.RedPoint) {
                [void]$chart.SeriesCollection(1).Points(2).Format.Fill.Solid()
                $chart.SeriesCollection(1).Points(2).Format.Fill.ForeColor.RGB = 255
            }
            if ($spec.Combo) {
                $chart.SeriesCollection(2).ChartType = 4
                $chart.SeriesCollection(2).AxisGroup = 2
            }
            if ($spec.Dates) {
                $chart.Axes(1, 1).CategoryType = 3
                $chart.Axes(1, 1).BaseUnit = 1
                $chart.Axes(1, 1).MajorUnit = 1
                $chart.Axes(1, 1).MajorUnitScale = 1
                $chart.Axes(1, 1).TickLabels.NumberFormat = 'm/d/yyyy'
                $chart.Axes(1, 1).TickLabels.NumberFormatLinked = $false
            }
            if ($spec.Name -eq 'chart-line' -or $spec.Name -eq 'chart-scatter' -or $spec.Name -eq 'chart-scatter-lines') {
                for ($seriesIndex = 1; $seriesIndex -le 2; $seriesIndex++) {
                    $chart.SeriesCollection($seriesIndex).MarkerStyle = 8
                    $chart.SeriesCollection($seriesIndex).MarkerSize = 6
                }
            }
            if ($spec.Name -eq 'chart-line-plain' -or $spec.Dates -or $spec.Combo) {
                $seriesIndex = $(if ($spec.Combo) { 2 } else { 1 })
                for (; $seriesIndex -le 2; $seriesIndex++) { $chart.SeriesCollection($seriesIndex).MarkerStyle = -4142 }
            }
            if ($spec.Name -eq 'chart-scatter-lines') {
                for ($seriesIndex = 1; $seriesIndex -le 2; $seriesIndex++) { $chart.SeriesCollection($seriesIndex).Smooth = $true }
            }
            [void]$chart.Refresh()
            $entry.Stage = 'read COM properties'
            Write-RunLog
            $entry.Actual = Get-ChartSnapshot $chart $spec $warnings
            if (-not $spec.Combo -and $entry.Actual.ChartType -ne [int]$spec.Type) {
                throw 'The chart type changed while configuring its data.'
            }
            if (-not $spec.ChartEx) {
                $expectedSeriesCount = $(if ($spec.SingleSeries) { 1 } else { 2 })
                if ($entry.Actual.Series.Count -ne $expectedSeriesCount) {
                    throw ('Expected ' + $expectedSeriesCount + ' series, read back ' + $entry.Actual.Series.Count)
                }
                for ($seriesIndex = 0; $seriesIndex -lt $expectedSeriesCount; $seriesIndex++) {
                    $actualSeries = $entry.Actual.Series[$seriesIndex]
                    if ($actualSeries.Name -ne ('Series ' + ($seriesIndex + 1))) {
                        throw ('Unexpected series name: ' + $actualSeries.Name)
                    }
                    if ($actualSeries.Values.Count -ne 3) { throw 'Expected exactly three values per series.' }
                    for ($valueIndex = 0; $valueIndex -lt 3; $valueIndex++) {
                        $expectedValue = ($valueIndex + 1) * 10 + $seriesIndex * 5
                        if ([double]$actualSeries.Values[$valueIndex] -ne $expectedValue) {
                            throw ('Series data readback differs from the requested value ' + $expectedValue)
                        }
                    }
                }
            }
            if ($spec.HoleSize -and $entry.Actual.DoughnutHoleSize -ne $spec.HoleSize) {
                throw 'Doughnut hole size readback is not 30 percent.'
            }
            if ($spec.RedPoint -and $entry.Actual.SecondPointRgb -ne 255) {
                throw 'Second pie sector did not retain its red fill.'
            }
            if ($spec.ChartEx) {
                $warnings.Add('Modern chart source contains three category rows and two numeric columns; confirm how this chart type plots them in Word. COM data is not a visual oracle.')
            }
            if ($spec.Floating) {
                $entry.Stage = 'ConvertToShape and square wrap'
                $shape = $inline.ConvertToShape()
                $shape.WrapFormat.Type = 0
                $shape.RelativeHorizontalPosition = 0
                $shape.Left = -999996
                $shape.RelativeVerticalPosition = 2
                $shape.Top = 0
                $entry.Actual.Layout = @{ Floating = $true; WrapType = [int]$shape.WrapFormat.Type; WidthPt = [double]$shape.Width; HeightPt = [double]$shape.Height; Left = [double]$shape.Left }
            }
            else {
                $entry.Actual.Layout = @{ Floating = $false; InTable = [bool]$spec.InTable; WidthPt = [double]$inline.Width; HeightPt = [double]$inline.Height }
            }

            $entry.Stage = 'SaveAs2 format 12 (first and only original save)'
            Write-RunLog
            [void]$document.SaveAs2([string]$destination, 12)
            $entry.SavedOnce = $true
            $entry.Bytes = (Get-Item -LiteralPath $destination).Length
            $entry.Stage = 'Word ExportAsFixedFormat PDF preview'
            $previewPath = Get-UniquePath $previewDirectory ([IO.Path]::GetFileNameWithoutExtension($destination)) '.pdf'
            [void]$document.ExportAsFixedFormat([string]$previewPath, 17)
            $entry.PreviewFile = '_previews/chart/' + [IO.Path]::GetFileName($previewPath)
            $entry.PreviewStatus = 'Word-exported PDF; not visually verified'
            [void]$document.Close(0)
            $document = $null
            $entry.Stage = 'read-only package self-check'
            $entry.PackageCheck = Test-ChartPackage $destination $spec
            if (-not $entry.PackageCheck.Passed) {
                throw ('Package self-check failed: ' + ($entry.PackageCheck.Failures -join '; '))
            }
            $entry.Status = 'saved-package-check-passed-needs-visual-check'
            $entry.Stage = 'finished'
            Write-Host ($entry.File + ': saved')
        }
        catch {
            $entry.Status = $(if ($entry.SavedOnce) { 'saved-but-incomplete' } else { 'failed' })
            $entry.Error = @{ Message = $_.Exception.Message; HResult = ('0x{0:X8}' -f $_.Exception.HResult); Detail = $_.ToString(); ScriptStackTrace = $_.ScriptStackTrace }
            if (Test-Path -LiteralPath $destination) { $entry.Bytes = (Get-Item -LiteralPath $destination).Length }
            Write-Warning ($entry.File + ' failed at ' + $entry.Stage + ': ' + $_.Exception.Message)
        }
        finally {
            if ($null -ne $workbook) {
                try { [void]$workbook.Close($false) }
                catch { $warnings.Add('Embedded workbook cleanup: ' + $_.Exception.Message) }
            }
            if ($null -ne $document) {
                try { [void]$document.Close(0) }
                catch { $warnings.Add('Document close without saving: ' + $_.Exception.Message) }
            }
            $entry.FinishedUtc = [DateTime]::UtcNow.ToString('o')
            Write-RunLog
            $sheet = $null
            $workbook = $null
            $chart = $null
            $inline = $null
            $shape = $null
            $table = $null
            $range = $null
            $document = $null
            [GC]::Collect()
            [GC]::WaitForPendingFinalizers()
        }
    }
}
catch {
    $run.FatalError = $_.ToString()
    throw
}
finally {
    if ($null -ne $word) {
        try { [void]$word.Quit(0) }
        catch { $run.FatalError = 'Word.Quit: ' + $_.Exception.Message }
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($word)
    }
    $run.CompletedUtc = [DateTime]::UtcNow.ToString('o')
    Write-RunLog
    Write-Host ('COM job log: ' + $logPath)
}

if (@($entries | Where-Object { $_.Status -ne 'saved-package-check-passed-needs-visual-check' }).Count -gt 0) { exit 1 }
