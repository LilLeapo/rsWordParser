param(
    [Parameter(Mandatory=$true)][ValidateSet('Open','Inspect','EditData','CloseData','Resave','Close','About')][string]$Action,
    [string]$Name,
    [int]$ChartIndex = 1,
    [switch]$RecoveredActive,
    [string]$Root = 'C:\code\rsWordParser\real-word-corpus-20260906'
)
$ErrorActionPreference = 'Stop'
$app = [Runtime.InteropServices.Marshal]::GetActiveObject('Word.Application')
if ($Action -eq 'About') {
    [ordered]@{ Name=$app.Name; Version=$app.Version; Build=$app.Build; OperatingSystem=$app.OperatingSystem } | ConvertTo-Json
    exit
}
$path = Join-Path (Join-Path $Root '_roundtrip') $Name
$doc = $null
foreach ($candidate in $app.Documents) {
    if ($candidate.FullName -eq $path) { $doc = $candidate; break }
}
if ($RecoveredActive) {
    $doc = $app.ActiveDocument
    if ($doc.Path -ne '') { throw 'RecoveredActive requires an unsaved recovery document.' }
}
if ($Action -eq 'Open') {
    if ($null -eq $doc) { $doc = $app.Documents.Open($path,$false,$true,$false) }
    $doc.Activate()
    $app.Visible = $true
    $doc.ActiveWindow.View.Type = 3
    $doc.ActiveWindow.View.Zoom.Percentage = 100
} elseif ($null -eq $doc) { throw "Document is not open: $path" }
if ($Action -eq 'Resave') {
    $output = Join-Path (Split-Path $path) (([IO.Path]::GetFileNameWithoutExtension($path)) + '-resaved-by-word.docx')
    if (Test-Path -LiteralPath $output) { throw "Refusing to overwrite: $output" }
    $doc.SaveAs2([string]$output,12)
    [ordered]@{ action=$Action; output=$output; bytes=(Get-Item -LiteralPath $output).Length } | ConvertTo-Json
    exit
}
if ($Action -eq 'Close') { $doc.Close(0); exit }
if ($Action -eq 'CloseData') { $doc.InlineShapes.Item($ChartIndex).Chart.ChartData.Workbook.Close($false); exit }
if ($Action -eq 'EditData') {
    $chart = $doc.InlineShapes.Item($ChartIndex).Chart
    $chart.ChartData.Activate()
    $workbook = $chart.ChartData.Workbook
    $sheet = $workbook.Worksheets.Item(1)
    $values = @()
    for ($row = 1; $row -le 4; $row++) {
        $cells = @()
        for ($column = 1; $column -le 3; $column++) { $cells += $sheet.Cells.Item($row,$column).Text }
        $values += ,$cells
    }
    [ordered]@{ action=$Action; workbook=$workbook.Name; values=$values } | ConvertTo-Json -Depth 8
    exit
}
$shapes = @()
foreach ($shape in $doc.InlineShapes) {
    $info = [ordered]@{ placement='inline'; type=$shape.Type; width=$shape.Width; height=$shape.Height; hasChart=$shape.HasChart; alt=$shape.AlternativeText }
    if ($shape.HasChart -eq -1) {
        $chart = $shape.Chart
        $info.chartType = $chart.ChartType
        $info.title = if ($chart.HasTitle) { $chart.ChartTitle.Text } else { $null }
        $info.series = @()
        for ($index = 1; $index -le $chart.SeriesCollection().Count; $index++) {
            $series = $chart.SeriesCollection($index)
            $info.series += [ordered]@{ name=$series.Name; categories=@($series.XValues); values=@($series.Values) }
        }
    }
    $shapes += $info
}
foreach ($shape in $doc.Shapes) {
    $shapes += [ordered]@{ placement='floating'; type=$shape.Type; name=$shape.Name; left=$shape.Left; top=$shape.Top; width=$shape.Width; height=$shape.Height; wrap=$shape.WrapFormat.Type; behind=$shape.WrapFormat.AllowOverlap; alt=$shape.AlternativeText }
}
[ordered]@{ action=$Action; path=$doc.FullName; readOnly=$doc.ReadOnly; compatibilityMode=$doc.CompatibilityMode; text=$doc.Content.Text; pages=$doc.ComputeStatistics(2); shapes=$shapes } | ConvertTo-Json -Depth 12
