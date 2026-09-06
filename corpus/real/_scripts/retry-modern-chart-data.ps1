param(
    [Parameter(Mandatory=$true)]
    [ValidateSet('chartex-waterfall','chartex-histogram','chartex-boxwhisker','chartex-funnel')]
    [string[]]$Only,
    [string]$Root = 'C:\code\rsWordParser\real-word-corpus-20260906'
)
$ErrorActionPreference='Stop'
$helperPath=Join-Path $PSScriptRoot 'prepare-modern-chart-dataset.ps1'
if(-not(Test-Path -LiteralPath $helperPath -PathType Leaf)){throw 'The modern dataset helper is unavailable.'}
$chartDirectory=Join-Path $Root 'chart'
$previewDirectory=Join-Path $Root '_previews/chart'
$scriptDirectory=Join-Path $Root '_scripts'
New-Item -ItemType Directory -Path $chartDirectory,$previewDirectory,$scriptDirectory -Force | Out-Null
$logPath=Join-Path $scriptDirectory ('modern-chart-data-retry-'+[DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff')+'.json')
$entries=[System.Collections.Generic.List[object]]::new()
$run=[ordered]@{
    startedUtc=[DateTime]::UtcNow.ToString('o')
    method='Native Documents.Add from each existing modern chart; dataset helper with empty-tail deletion; SaveAs2 once and Word PDF export'
    validation='No modern chart series access. All saved variants require offline package and PDF review.'
    entries=$entries
}
function Write-RunLog {
    $run | ConvertTo-Json -Depth 24 | Set-Content -LiteralPath $logPath -Encoding utf8
}
$word=[Runtime.InteropServices.Marshal]::GetActiveObject('Word.Application')
$word.Visible=$true
$run.wordVersion=$word.Version
$run.wordBuild=$word.Build
$title=([string][char]0x9500)+([string][char]0x552E)+([string][char]0x7EDF)+([string][char]0x8BA1)
foreach($name in $Only) {
    $source=Join-Path $chartDirectory ($name+'.docx')
    $entry=[ordered]@{requestedCase=('chart/'+$name);template=('chart/'+$name+'.docx');stage='preflight';status='started';savedOnce=$false}
    $entries.Add($entry)
    $doc=$null
    $sourceHash=$null
    try {
        if(-not(Test-Path -LiteralPath $source -PathType Leaf)){throw 'The existing native modern chart template is missing.'}
        $sourceHash=(Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash
        $entry.templateSha256Before=$sourceHash
        $number=2
        do {
            $stem="$name-$number"
            $caseTag='chart/'+$stem
            $destination=Join-Path $chartDirectory ($stem+'.docx')
            $preview=Join-Path $previewDirectory ($stem+'.pdf')
            $tagInUse=$false
            foreach($candidate in $word.Documents) {
                try {if($candidate.Variables.Item('CorpusUICase').Value -eq $caseTag){$tagInUse=$true;break}} catch {}
            }
            $number++
        } while((Test-Path -LiteralPath $destination) -or (Test-Path -LiteralPath $preview) -or $tagInUse)
        $entry.case=$caseTag
        $entry.file='chart/'+$stem+'.docx'
        $entry.preview='_previews/chart/'+$stem+'.pdf'
        $entry.stage='Documents.Add from native modern chart template'
        Write-RunLog
        $doc=$word.Documents.Add([string]$source,$false,0,$true)
        if($doc.Path -ne ''){throw 'Expected a new unsaved document.'}
        try {$doc.Variables.Item('CorpusUICase').Value=$caseTag} catch {$doc.Variables.Add('CorpusUICase',$caseTag) | Out-Null}
        $entry.stage='prepare-modern-chart-dataset with DeleteEmptyTailRows'
        Write-RunLog
        $helperOutput=& $helperPath -Case $caseTag -Root $Root -DeleteEmptyTailRows
        $entry.datasetResult=($helperOutput -join [Environment]::NewLine) | ConvertFrom-Json
        $entry.nativeRebind=$entry.datasetResult.nativeRebind
        $entry.nativeRebindError=$entry.datasetResult.nativeRebindError
        if($entry.datasetResult.status -eq 'failed'){throw 'The native dataset helper failed.'}
        if($doc.Path -ne ''){throw 'The dataset helper unexpectedly saved the document.'}
        $charts=@()
        foreach($shape in $doc.Shapes){if($shape.Type -eq 3){$charts+=,$shape.Chart}}
        foreach($shape in $doc.InlineShapes){if($shape.HasChart -eq -1){$charts+=,$shape.Chart}}
        if($charts.Count -ne 1){throw 'Expected one native modern chart after the dataset update.'}
        $chart=$charts[0]
        $chart.HasTitle=$true
        $chart.ChartTitle.Text=$title
        $entry.stage='SaveAs2 format 12 once'
        Write-RunLog
        $doc.SaveAs2([string]$destination,12)
        $entry.savedOnce=$true
        $entry.bytes=(Get-Item -LiteralPath $destination).Length
        $entry.stage='Word PDF export'
        Write-RunLog
        $doc.ExportAsFixedFormat([string]$preview,17,$false)
        $entry.status='saved-needs-offline-package-and-pdf-review'
    } catch {
        $entry.status=if($entry.savedOnce){'saved-but-incomplete'}else{'failed'}
        $entry.error=[ordered]@{message=$_.Exception.Message;hresult=('0x{0:X8}' -f $_.Exception.HResult);scriptStack=$_.ScriptStackTrace}
    } finally {
        if($null -ne $doc){try {$doc.Close(0)} catch {$entry.closeError=$_.Exception.Message}}
        if($null -ne $sourceHash) {
            $entry.templateSha256After=(Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash
            $entry.templateUnchanged=$entry.templateSha256After -eq $sourceHash
            if(-not $entry.templateUnchanged){$entry.status='template-hash-changed'}
        }
        $entry.finishedUtc=[DateTime]::UtcNow.ToString('o')
        Write-RunLog
    }
    $entry | ConvertTo-Json -Depth 24
    if($entry.closeError -or $entry.status -eq 'template-hash-changed'){throw 'Stopped after document-close failure or unexpected template modification.'}
}
$run.finishedUtc=[DateTime]::UtcNow.ToString('o')
Write-RunLog
Write-Output ('Log: '+$logPath)
