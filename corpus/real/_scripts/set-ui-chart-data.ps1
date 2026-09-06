param(
    [Parameter(Mandatory=$true)][string]$Case,
    [string]$Root = 'C:\code\rsWordParser\real-word-corpus-20260906'
)
$ErrorActionPreference='Stop'
trap { Write-Output $_.ScriptStackTrace; Write-Output $_.InvocationInfo.PositionMessage; throw }
$word=[Runtime.InteropServices.Marshal]::GetActiveObject('Word.Application')
$doc=$null
foreach($candidate in $word.Documents) {
    try { if($candidate.Variables.Item('CorpusUICase').Value -eq $Case){$doc=$candidate;break} } catch {}
}
if($null -eq $doc -or $doc.Path -ne ''){throw 'Expected this session''s unsaved UI chart document.'}
$chart=$null
foreach($shape in $doc.Shapes){if($shape.Type -eq 3){$chart=$shape.Chart;break}}
if($null -eq $chart){foreach($shape in $doc.InlineShapes){if($shape.HasChart -eq -1){$chart=$shape.Chart;break}}}
if($null -eq $chart){throw 'No native chart was created by the Word UI.'}
$book=$chart.ChartData.Workbook
if($null -eq $book){$chart.ChartData.Activate();$book=$chart.ChartData.Workbook}
if($null -eq $book){throw 'The actual chart data workbook is unavailable.'}
$sheet=$book.Worksheets.Item(1)
$sheet.Cells.Clear()
$sheet.Name='ChartData'
$hierarchical=$Case -match 'sunburst|treemap'
$values=New-Object 'object[,]' 4,3
if($hierarchical) {
    $values[0,0]='Category';$values[0,1]='Subcategory';$values[0,2]='Value'
    $values[1,0]='Group A';$values[1,1]='Item 1';$values[1,2]=[double]10
    $values[2,0]='Group A';$values[2,1]='Item 2';$values[2,2]=[double]20
    $values[3,0]='Group B';$values[3,1]='Item 3';$values[3,2]=[double]30
} else {
    $values[0,0]='Category';$values[0,1]='Series 1';$values[0,2]='Series 2'
    for($row=1;$row -le 3;$row++){
        $values[$row,0]="Category $row";$values[$row,1]=[double]($row*10);$values[$row,2]=[double]($row*10+5)
    }
}
$sheet.Range('A1:C4').Value2=$values
$sourceDataError=$null
try {$chart.SetSourceData('ChartData!$A$1:$C$4',2)}catch{$sourceDataError=$_.Exception.Message}
$chart.HasTitle=$true
$chart.ChartTitle.Text='销售统计'
$observed=@()
for($row=1;$row -le 4;$row++){
    $cells=@()
    for($column=1;$column -le 3;$column++){$cells+=[string]$sheet.Cells.Item($row,$column).Text}
    $observed+=,$cells
}
$result=[ordered]@{case=$Case;method='Word UI Insert Chart, then actual embedded Excel workbook edited through COM';hierarchical=$hierarchical;worksheet=$observed;title=$chart.ChartTitle.Text;sourceDataError=$sourceDataError;sourceRangeRequiresUi=($null -ne $sourceDataError)}
try {$result.chartType=$chart.ChartType} catch {$result.chartTypeError=$_.Exception.Message}
try {
    $result.series=@()
    for($index=1;$index -le $chart.SeriesCollection().Count;$index++){
        $series=$chart.SeriesCollection($index)
        $result.series+=@{name=$series.Name;categories=@($series.XValues);values=@($series.Values)}
    }
} catch {$result.seriesReadError=$_.Exception.Message}
$book.Close($true)
try {$chart.Refresh()}catch{$result.refreshError=$_.Exception.Message}
$doc.Activate()
$doc.Range(0,0).Select()
$result | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $Root ('_scripts/'+($Case -split '/')[-1]+'-data.json')) -Encoding utf8
$result | ConvertTo-Json -Depth 12
