function Begin-CReview {
    param([object]$Word,[string]$File)
    $p=Join-Path 'C:\word\round3-work-20260907\real-word-round3-inputs\fixtures' $File
    $Word.DisplayAlerts=-1
    $d=$Word.Documents.Open($p,$false,$true,$false)
    $d.Activate()
    $r=& (Join-Path $PSScriptRoot 'task-c-read.ps1') -Word $Word -InputPath $p
    $r.rows | Select-Object sentence,desktopWord,comparisonWithRound2,error | ConvertTo-Json -Depth 4
}
function Add-CObservation {
    param([string]$File,[string]$Observation,[string]$Stem,[string]$Sentence,[string]$Kind='page')
    $p=Join-Path $PSScriptRoot 'task-c-ui.json'
    $rows=@()
    if(Test-Path -LiteralPath $p){$parsed=Get-Content -LiteralPath $p -Raw -Encoding UTF8 | ConvertFrom-Json;$rows=@($parsed)}
    $e='screenshots/'+$Stem+'-0.jpg'
    if(-not(Test-Path -LiteralPath (Join-Path (Split-Path $PSScriptRoot) $e))){throw "Missing $e"}
    $rows+=,[pscustomobject]@{file=$File;sentence=$Sentence;observation=$Observation;evidence=@(@{path=$e;kind=$Kind})}
    ConvertTo-Json -InputObject $rows -Depth 10 | Set-Content -LiteralPath $p -Encoding UTF8
    @{file=$File;uiRows=$rows.Count}
}
