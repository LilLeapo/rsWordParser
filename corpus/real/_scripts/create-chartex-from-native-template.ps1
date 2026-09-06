param(
    [Parameter(Mandatory=$true)]
    [ValidateSet('chartex-treemap','chartex-waterfall','chartex-histogram','chartex-boxwhisker','chartex-funnel')]
    [string[]]$Only,
    [string]$Root = 'C:\code\rsWordParser\real-word-corpus-20260906',
    [switch]$UseClassicSource
)
$ErrorActionPreference='Stop'
$definitions=@{
    'chartex-treemap'=@{type=117;source='chart/chartex-sunburst-2.docx'}
    'chartex-waterfall'=@{type=119;source='chart/chartex-sunburst-2.docx'}
    'chartex-histogram'=@{type=118;source='chart/chartex-sunburst-2.docx'}
    'chartex-boxwhisker'=@{type=121;source='chart/chartex-sunburst-2.docx'}
    'chartex-funnel'=@{type=123;source='chart/chartex-sunburst-2.docx'}
}
if($UseClassicSource) {
    foreach($name in @('chartex-waterfall','chartex-histogram','chartex-boxwhisker','chartex-funnel')) {
        $definitions[$name].source='chart/chart-column.docx'
    }
}
$chartDirectory=Join-Path $Root 'chart'
$previewDirectory=Join-Path $Root '_previews/chart'
$scriptDirectory=Join-Path $Root '_scripts'
New-Item -ItemType Directory -Path $chartDirectory,$previewDirectory,$scriptDirectory -Force | Out-Null
$logPath=Join-Path $scriptDirectory ('chartex-native-template-results-'+[DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff')+'.json')
$entries=[System.Collections.Generic.List[object]]::new()
$run=[ordered]@{
    startedUtc=[DateTime]::UtcNow.ToString('o')
    method='Word Documents.Add(existing native DOCX as template), Chart.ChartType conversion, one SaveAs2 and Word PDF export'
    observationStatus='No chart series/property readback; inspect saved package and actual Word UI or exported PDF separately'
    useClassicSource=[bool]$UseClassicSource
    dataset=$(if($UseClassicSource){'Classic source for non-treemap cases: Category 1/2/3, Series 1 10/20/30, Series 2 15/25/35; treemap retains hierarchical source'}else{'Inherited sunburst-2 worksheet: Group A/A/B, Item 1/2/3, one numeric series 10/20/30 in D2:D4'})
    entries=$entries
}

function Write-RunLog {
    $run | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $logPath -Encoding utf8
}

$word=[Runtime.InteropServices.Marshal]::GetActiveObject('Word.Application')
$word.Visible=$true
$run.wordVersion=$word.Version
$run.wordBuild=$word.Build
$title=([string][char]0x9500)+([string][char]0x552E)+([string][char]0x7EDF)+([string][char]0x8BA1)
$before='before '+([string][char]0x524D)+([string][char]0x6587)
$after='after '+([string][char]0x540E)+([string][char]0x6587)
foreach($name in $Only) {
    $definition=$definitions[$name]
    $source=Join-Path $Root $definition.source
    $entry=[ordered]@{case=('chart/'+$name);source=$definition.source;requestedType=$definition.type;status='creating';stage='preflight';savedOnce=$false}
    $entries.Add($entry)
    $doc=$null
    $sourceHash=$null
    try {
        if(-not(Test-Path -LiteralPath $source -PathType Leaf)){throw 'Native source document does not exist.'}
        $sourceHash=(Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash
        $entry.sourceSha256Before=$sourceHash
        $number=1
        do {
            $stem=if($number -eq 1){$name}else{"$name-$number"}
            $destination=Join-Path $chartDirectory ($stem+'.docx')
            $preview=Join-Path $previewDirectory ($stem+'.pdf')
            $number++
        } while((Test-Path -LiteralPath $destination) -or (Test-Path -LiteralPath $preview))
        $entry.file='chart/'+$stem+'.docx'
        $entry.preview='_previews/chart/'+$stem+'.pdf'
        $entry.stage='Documents.Add from native DOCX template'
        Write-RunLog
        $doc=$word.Documents.Add([string]$source,$false,0,$true)
        if($doc.Path -ne ''){throw 'Documents.Add did not return a new unsaved document.'}
        try {$doc.Variables.Item('CorpusUICase').Value=$entry.case} catch {$doc.Variables.Add('CorpusUICase',$entry.case) | Out-Null}
        if(-not($doc.Content.Text.Contains($before) -and $doc.Content.Text.Contains($after))){throw 'Template did not retain before/after markers.'}
        $chartHosts=@()
        foreach($shape in $doc.InlineShapes){if($shape.HasChart -eq -1){$chartHosts+=,@{kind='inline';object=$shape}}}
        foreach($shape in $doc.Shapes){if($shape.Type -eq 3){$chartHosts+=,@{kind='shape';object=$shape}}}
        if($chartHosts.Count -ne 1){throw 'Expected one native chart in the new document.'}
        $chartHost=$chartHosts[0]
        if($UseClassicSource -and $name -ne 'chartex-treemap' -and $chartHost.kind -eq 'inline') {
            $entry.stage='Convert inline chart to Shape with inline wrap'
            Write-RunLog
            $nativeChartShape=$chartHost.object.ConvertToShape()
            $nativeChartShape.WrapFormat.Type=7
            $chart=$nativeChartShape.Chart
            $entry.hostConversion='InlineShape.ConvertToShape; Shape.WrapFormat.Type=7; no conversion back'
        } else {
            $chart=$chartHost.object.Chart
        }
        $entry.stage='Native Chart.ChartType conversion'
        Write-RunLog
        $chart.ChartType=[int]$definition.type
        $chart.HasTitle=$true
        $chart.ChartTitle.Text=$title
        $entry.typeSetterCompleted=$true
        $doc.Activate()
        $doc.ActiveWindow.View.Type=3
        $doc.Range(0,0).Select()
        $entry.stage='SaveAs2 format 12 once'
        Write-RunLog
        $doc.SaveAs2([string]$destination,12)
        $entry.savedOnce=$true
        $entry.bytes=(Get-Item -LiteralPath $destination).Length
        $entry.stage='Word PDF export'
        Write-RunLog
        $doc.ExportAsFixedFormat([string]$preview,17,$false)
        $entry.status='saved-needs-package-and-visual-checks'
    } catch {
        $entry.status=if($entry.savedOnce){'saved-but-incomplete'}else{'failed'}
        $entry.error=[ordered]@{message=$_.Exception.Message;hresult=('0x{0:X8}' -f $_.Exception.HResult);scriptStack=$_.ScriptStackTrace}
    } finally {
        if($null -ne $doc){
            try {$doc.Close(0)} catch {$entry.closeError=$_.Exception.Message}
        }
        if($null -ne $sourceHash){
            $entry.sourceSha256After=(Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash
            $entry.sourceUnchanged=$entry.sourceSha256After -eq $sourceHash
            if(-not $entry.sourceUnchanged){$entry.status='source-hash-changed'}
        }
        $entry.finishedUtc=[DateTime]::UtcNow.ToString('o')
        Write-RunLog
    }
    $entry | ConvertTo-Json -Depth 20
    if($entry.closeError -or $entry.status -eq 'source-hash-changed'){throw 'Stopped after document-close failure or unexpected source modification.'}
}
$run.finishedUtc=[DateTime]::UtcNow.ToString('o')
Write-RunLog
Write-Output ('Log: '+$logPath)
