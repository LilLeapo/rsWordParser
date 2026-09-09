param(
    [Parameter(Mandatory=$true)]
    [ValidatePattern('^chart/chartex-(waterfall|funnel|histogram|boxwhisker)(-\d+)?$')]
    [string]$Case,
    [string]$Root = 'C:\code\rsWordParser\real-word-corpus-20260906',
    [switch]$DeleteEmptyTailRows,
    [switch]$OpenDataWindow,
    [switch]$SkipNativeRebind
)
$ErrorActionPreference='Stop'
$word=[Runtime.InteropServices.Marshal]::GetActiveObject('Word.Application')
$documents=@()
foreach($candidate in $word.Documents) {
    if($candidate.Path -ne ''){continue}
    try {if($candidate.Variables.Item('CorpusUICase').Value -eq $Case){$documents+=,$candidate}} catch {}
}
if($documents.Count -ne 1){throw 'Expected exactly one tagged, unsaved native modern chart document.'}
$doc=$documents[0]
$charts=@()
foreach($shape in $doc.Shapes){if($shape.Type -eq 3){$charts+=,$shape.Chart}}
foreach($shape in $doc.InlineShapes){if($shape.HasChart -eq -1){$charts+=,$shape.Chart}}
if($charts.Count -ne 1){throw 'Expected exactly one native modern chart.'}
$chart=$charts[0]
$twoSeries=$Case -match 'histogram|boxwhisker'
$lastDataColumn=if($twoSeries){'E'}else{'D'}
$sourceAddress='$C$1:$'+$lastDataColumn+'$4'
$logPath=Join-Path $Root ('_scripts/modern-chart-dataset-attempt-'+[DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff')+'.json')
$result=[ordered]@{
    case=$Case
    method='Native embedded worksheet editing and Chart.SetSourceData attempt; no chart series access or document save'
    intendedData=$(if($twoSeries){'C: Category 1/2/3; D: Series 1=10/20/30; E: Series 2=15/25/35'}else{'C: Category 1/2/3; D: Series 1=10/20/30'})
    sourceAddress=$sourceAddress
    requiresSavedPackageVerification=$true
    stage='activate native chart workbook'
    status='started'
}
try {
    $book=$null
    try {$book=$chart.ChartData.Workbook} catch {}
    if($null -eq $book){$chart.ChartData.Activate();$book=$chart.ChartData.Workbook}
    if($null -eq $book){throw 'The actual embedded workbook is unavailable.'}
    $sheet=$book.Worksheets.Item(1)
    $result.worksheetName=$sheet.Name
    $result.usedRangeBefore=$sheet.UsedRange.Address()
    $lastUsedColumn=[int]($sheet.UsedRange.Column+$sheet.UsedRange.Columns.Count-1)
    if($lastUsedColumn -gt 10){throw 'Unexpected template worksheet width.'}
    $result.firstFourRowsBefore=$sheet.Range('A1:E4').Value2

    # Keep D as the existing native value column if rebinding is unavailable.
    $values=New-Object 'object[,]' 4,5
    $values[0,2]='Category';$values[0,3]='Series 1'
    if($twoSeries){$values[0,4]='Series 2'}
    for($row=1;$row -le 3;$row++) {
        $values[$row,2]="Category $row"
        $values[$row,3]=[double]($row*10)
        if($twoSeries){$values[$row,4]=[double]($row*10+5)}
    }
    $sheet.Range('A1:E4').Value2=$values
    $result.firstFourRowsAfter=$sheet.Range('A1:E4').Value2
    if($DeleteEmptyTailRows) {
        $result.stage='verify and delete empty rows 5:17'
        for($row=5;$row -le 17;$row++) {
            for($column=1;$column -le [Math]::Max(5,$lastUsedColumn);$column++) {
                $cell=$sheet.Cells.Item($row,$column)
                $value=$cell.Value2
                if(($null -ne $value -and [string]$value -ne '') -or $cell.HasFormula) {
                    throw ('Refusing to delete a nonempty cell at row '+$row+', column '+$column)
                }
            }
        }
        $sheet.Range('5:17').EntireRow.Delete() | Out-Null
        $result.rowsDeleted='5:17'
        $result.deletionCaveat='Worksheet row deletion does not itself prove that external ChartEx references shrink.'
    }
    $sourceRange=$sheet.Range($sourceAddress)
    $sheetNameQuoted="'"+([string]$sheet.Name).Replace("'","''")+"'"
    $nativeSource=$sheetNameQuoted+'!'+$sourceAddress
    $result.requestedNativeSource=$nativeSource
    $result.nativeRebind='skipped'
    if(-not $SkipNativeRebind) {
        $result.stage='native SetSourceData attempt'
        try {
            $chart.SetSourceData([string]$nativeSource,2)
            $result.nativeRebind='setter-completed-unverified'
        } catch {
            $result.nativeRebind='rejected'
            $result.nativeRebindError=[ordered]@{message=$_.Exception.Message;hresult=('0x{0:X8}' -f $_.Exception.HResult)}
        }
    }
    $result.usedRangeAfter=$sheet.UsedRange.Address()
    if($OpenDataWindow) {
        $result.stage='show actual native workbook for root UI'
        try {
            $chart.ChartData.ActivateChartDataWindow()
            $result.dataWindowMethod='ChartData.ActivateChartDataWindow'
        } catch {
            $result.dataWindowMethodError=$_.Exception.Message
            $book.Application.Visible=$true
            $result.dataWindowMethod='Workbook.Application.Visible fallback'
        }
        $book.Activate()
        $sheet.Activate()
        $sourceRange.Select()
        $result.workbookLeftOpen=$true
        $result.nextStep='Root must inspect the actual selected C1:D4 or C1:E4 source range in Excel and use native chart source-selection UI if SetSourceData was rejected. Close the workbook with changes, then save only this new document as a fresh variant.'
    } else {
        $result.stage='submit native workbook edits'
        $book.Close($true)
        $result.workbookLeftOpen=$false
        $doc.Activate()
        $doc.Range(0,0).Select()
        $result.nextStep='Root must inspect Word and save this new document as a fresh variant; copied-package checks must establish actual category count and plotted series. A rejected rebind does not create a second series merely because E cells exist.'
    }
    $result.status='dataset-prepared-chart-reference-and-series-count-unverified'
} catch {
    $result.status='failed'
    $result.error=[ordered]@{message=$_.Exception.Message;hresult=('0x{0:X8}' -f $_.Exception.HResult);scriptStack=$_.ScriptStackTrace}
    throw
} finally {
    $result.finishedUtc=[DateTime]::UtcNow.ToString('o')
    $result | ConvertTo-Json -Depth 16 | Set-Content -LiteralPath $logPath -Encoding utf8
    $result | ConvertTo-Json -Depth 16
}
