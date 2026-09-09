param(
    [Parameter(Mandatory=$true)][string]$Case,
    [string]$Root = 'C:\code\rsWordParser\real-word-corpus-20260906'
)
$ErrorActionPreference='Stop'
$word=[Runtime.InteropServices.Marshal]::GetActiveObject('Word.Application')
$doc=$null
foreach($candidate in $word.Documents) {
    if($candidate.Path -ne ''){continue}
    try {if($candidate.Variables.Item('CorpusUICase').Value -eq $Case){$doc=$candidate;break}} catch {}
}
if($null -eq $doc){throw 'Expected a tagged, new unsaved document created from the native sunburst template.'}
$chart=$null
foreach($shape in $doc.Shapes){if($shape.Type -eq 3){$chart=$shape.Chart;break}}
if($null -eq $chart){foreach($shape in $doc.InlineShapes){if($shape.HasChart -eq -1){$chart=$shape.Chart;break}}}
if($null -eq $chart){throw 'No native chart was found.'}
$logPath=Join-Path $Root ('_scripts/chartex-tail-row-deletion-attempt-'+[DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff')+'.json')
$result=[ordered]@{case=$Case;method='Delete genuinely empty worksheet rows 5:17 through native Excel COM; no series access or document save';stage='acquire native chart workbook';status='started'}
try {
    $book=$null
    try {$book=$chart.ChartData.Workbook} catch {}
    if($null -eq $book){$chart.ChartData.Activate();$book=$chart.ChartData.Workbook}
    if($null -eq $book){throw 'The native embedded workbook is unavailable.'}
    $sheet=$book.Worksheets.Item(1)
    $result.worksheetName=$sheet.Name
    $result.usedRangeBefore=$sheet.UsedRange.Address()
    $used=$sheet.UsedRange
    $lastColumn=[int]($used.Column+$used.Columns.Count-1)
    if($lastColumn -gt 10){throw 'Unexpected native template column count.'}
    $data=@()
    for($row=1;$row -le 4;$row++) {
        $cells=@()
        for($column=1;$column -le $lastColumn;$column++){$cells+=$sheet.Cells.Item($row,$column).Value2}
        $data+=,$cells
    }
    $result.firstFourRows=$data
    for($row=5;$row -le 17;$row++) {
        for($column=1;$column -le $lastColumn;$column++) {
            $value=$sheet.Cells.Item($row,$column).Value2
            $formula=$sheet.Cells.Item($row,$column).Formula
            if(($null -ne $value -and [string]$value -ne '') -or ($null -ne $formula -and [string]$formula -ne '')) {
                throw ('Refusing to delete nonempty worksheet cell at row '+$row+', column '+$column)
            }
        }
    }
    $result.stage='delete empty entire rows 5:17'
    $sheet.Range('5:17').EntireRow.Delete() | Out-Null
    $result.rowsDeleted='5:17'
    $result.usedRangeAfter=$sheet.UsedRange.Address()
    $result.stage='submit embedded workbook changes'
    $book.Close($true)
    $doc.Activate()
    $doc.Range(0,0).Select()
    $result.status='worksheet-rows-deleted-chart-reference-update-unverified'
    $result.nextCheck='Inspect actual Word chart; save this new document once under a new variant name, then inspect copied chartEx formulas and point counts.'
} catch {
    $result.status='failed'
    $result.error=[ordered]@{message=$_.Exception.Message;hresult=('0x{0:X8}' -f $_.Exception.HResult);scriptStack=$_.ScriptStackTrace}
    throw
} finally {
    $result.finishedUtc=[DateTime]::UtcNow.ToString('o')
    $result | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $logPath -Encoding utf8
    $result | ConvertTo-Json -Depth 12
}
