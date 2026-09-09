param(
    [Parameter(Mandatory=$true)][string]$Case,
    [string]$Root = 'C:\code\rsWordParser\real-word-corpus-20260906',
    [switch]$TrySeriesFormula
)
$ErrorActionPreference='Stop'
trap { Write-Output $_.ScriptStackTrace; Write-Output $_.InvocationInfo.PositionMessage; throw }

function Read-SeriesState($Chart) {
    $items=@()
    try {
        for($index=1;$index -le $Chart.SeriesCollection().Count;$index++) {
            $series=$Chart.SeriesCollection($index)
            $item=[ordered]@{index=$index}
            foreach($property in @('Name','Formula','FormulaLocal','XValues','Values')) {
                try {$item[$property]=$series.$property} catch {$item[$property+'Error']=$_.Exception.Message}
            }
            $items+=,$item
        }
    } catch {$items+=,@{collectionError=$_.Exception.Message}}
    return $items
}

$word=[Runtime.InteropServices.Marshal]::GetActiveObject('Word.Application')
$doc=$null
foreach($candidate in $word.Documents) {
    try {if($candidate.Variables.Item('CorpusUICase').Value -eq $Case){$doc=$candidate;break}} catch {}
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
$sheetName=[string]$sheet.Name
$used=$sheet.UsedRange
$lastRow=[int]($used.Row+$used.Rows.Count-1)
$lastColumn=[int]($used.Column+$used.Columns.Count-1)
if($lastRow -gt 100 -or $lastColumn -gt 10){throw 'Unexpected default chart dataset dimensions.'}
$hierarchical=$Case -match 'sunburst|treemap'
$lastRow=[Math]::Max(4,$lastRow)
$lastColumn=[Math]::Max(3,$lastColumn)
$valueColumn=if($hierarchical){$lastColumn}else{2}
$beforeSeries=@(Read-SeriesState $chart)
$original=@()
for($row=1;$row -le $lastRow;$row++) {
    $cells=@()
    for($column=1;$column -le $lastColumn;$column++){$cells+=$sheet.Cells.Item($row,$column).Value2}
    $original+=,$cells
}

# Retain native worksheet/range references; blank trailing cells through Value2.
$values=New-Object 'object[,]' $lastRow,$lastColumn
$values[0,0]='Category'
if($hierarchical) {
    $values[0,1]='Subcategory';$values[0,($valueColumn-1)]='Value'
    for($row=1;$row -le 3;$row++) {
        $values[$row,0]=if($row -le 2){'Group A'}else{'Group B'}
        $values[$row,1]="Item $row"
        $values[$row,($valueColumn-1)]=[double]($row*10)
    }
} else {
    $values[0,1]='Series 1';$values[0,2]='Series 2'
    for($row=1;$row -le 3;$row++) {
        $values[$row,0]="Category $row";$values[$row,1]=[double]($row*10);$values[$row,2]=[double]($row*10+5)
    }
}
$target=$sheet.Range($sheet.Cells.Item(1,1),$sheet.Cells.Item($lastRow,$lastColumn))
$target.Value2=$values
$formulaAttempts=@()
if($TrySeriesFormula) {
    $escapedSheet="'"+$sheetName.Replace("'","''")+"'"
    $valueLetter=[string][char](64+$valueColumn)
    $categoryLast=if($hierarchical){[string][char](63+$valueColumn)}else{'A'}
    for($index=1;$index -le $chart.SeriesCollection().Count;$index++) {
        $series=$chart.SeriesCollection($index)
        $seriesValueLetter=if($hierarchical){$valueLetter}else{[string][char](65+$index)}
        $formula='=SERIES({0}!${1}$1,{0}!$A$2:${2}$4,{0}!${1}$2:${1}$4,{3})' -f $escapedSheet,$seriesValueLetter,$categoryLast,$index
        $attempt=[ordered]@{index=$index;requested=$formula}
        try {$series.Formula=$formula;$attempt.readback=$series.Formula;$attempt.applied=$true} catch {$attempt.applied=$false;$attempt.error=$_.Exception.Message}
        $formulaAttempts+=,$attempt
    }
}
try {$sheet.Calculate()} catch {}
$afterSeries=@(Read-SeriesState $chart)
$observed=@()
for($row=1;$row -le $lastRow;$row++) {
    $cells=@()
    for($column=1;$column -le $lastColumn;$column++){$cells+=$sheet.Cells.Item($row,$column).Value2}
    $observed+=,$cells
}
$result=[ordered]@{
    case=$Case
    method='Word UI Insert Chart, then edit native embedded worksheet without renaming, deleting or replacing source ranges'
    hierarchical=$hierarchical
    worksheetName=$sheetName
    valueColumn=$valueColumn
    preservedRange=$target.Address()
    originalWorksheet=$original
    worksheet=$observed
    seriesBefore=$beforeSeries
    seriesAfterWrite=$afterSeries
    formulaAttempts=$formulaAttempts
}
$book.Close($true)
try {$chart.Refresh()}catch{$result.refreshError=$_.Exception.Message}
$chart.HasTitle=$true
$chart.ChartTitle.Text=([string][char]0x9500)+([string][char]0x552E)+([string][char]0x7EDF)+([string][char]0x8BA1)
$result.title=$chart.ChartTitle.Text
$result.seriesAfterClose=@(Read-SeriesState $chart)
try {$result.chartType=$chart.ChartType} catch {$result.chartTypeError=$_.Exception.Message}
$doc.Activate()
$doc.Range(0,0).Select()
$result | ConvertTo-Json -Depth 16 | Set-Content -LiteralPath (Join-Path $Root ('_scripts/'+($Case -split '/')[-1]+'-preserved-data.json')) -Encoding utf8
$result | ConvertTo-Json -Depth 16
