function Begin-BReview {
    param([object]$Word,[string]$File)
    . (Join-Path $PSScriptRoot 'edited3-ui.ps1')
    Open-Edited3UiCase -Word $Word -File $File | Out-Null
    Read-Edited3ActiveCase -Word $Word -File $File -ReadoutTag initial -FocusFeature | Out-Null
    @{file=$File;active=$Word.ActiveDocument.FullName;compat=$Word.ActiveDocument.CompatibilityMode} | ConvertTo-Json
}
function Complete-BReview {
    param([object]$Word,[string]$File,[string]$Observation,[Nullable[bool]]$Consistent=$null,[string]$Recovery='none',[string[]]$ExtraScreenshots=@(),[string]$Root='C:\word\real-word-round3-20260907')
    $ErrorActionPreference='Stop'
    . (Join-Path $PSScriptRoot 'edited3-ui.ps1')
    . (Join-Path $PSScriptRoot 'task-a.ps1')
    $stem=[IO.Path]::GetFileNameWithoutExtension($File)
    $screenshots=@('screenshots/b-'+$stem+'-0.jpg')+$ExtraScreenshots
    foreach($evidence in $screenshots){if(-not(Test-Path -LiteralPath (Join-Path $Root $evidence))){throw "Screenshot missing: $evidence"}}
    if([string]::IsNullOrWhiteSpace($Observation)){throw 'Observation is required.'}
    $uiPath=Join-Path $Root 'ui-edited3.json'
    $rows=@()
    if(Test-Path -LiteralPath $uiPath){$parsed=Get-Content -LiteralPath $uiPath -Raw -Encoding UTF8 | ConvertFrom-Json;$rows=@($parsed)}
    if(@($rows | Where-Object file -eq $File).Count){throw "UI record already exists: $File"}
    $initialPath=Join-Path $Root ('_readouts/'+$stem+'-ui-initial.json')
    if(-not(Test-Path -LiteralPath $initialPath)){Read-Edited3ActiveCase -Word $Word -File $File -ReadoutTag initial | Out-Null}
    $initial=Get-Content -LiteralPath $initialPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $activated=$null
    if($initial.op -in 'newchart','chartdata'){$activated=Read-Edited3ActiveCase -Word $Word -File $File -ActivateData -ReadoutTag activated}
    $artifacts=Save-Edited3UiArtifacts -Word $Word -File $File -StateDescription 'Saved after direct Word UI inspection and native chart workbook activation when applicable.'
    $savedHash=SharedHash $artifacts.resaved
    $Word.ActiveDocument.Close(0)
    if((SharedHash $artifacts.resaved)-ne$savedHash){throw 'Resaved DOCX changed during close.'}
    $entry=[ordered]@{file=$File;recovery=$Recovery;observation=$Observation;consistent=$Consistent;screenshot=$screenshots;pdf=$artifacts.pdf;resaved=$artifacts.resaved;resaved_sha256=$savedHash;initialReadout=$initialPath;activationReadout=$(if($activated){Join-Path $Root ('_readouts/'+$stem+'-ui-activated.json')}else{$null});method='Native Word visible read-only open with DisplayAlerts=-1, current window screenshot inspected; separate COM readout; Word SaveAs2 then PDF; closed without another save.';date=(Get-Date).ToString('o')}
    $rows+=,[pscustomobject]$entry
    ConvertTo-Json -InputObject $rows -Depth 35 | Set-Content -LiteralPath $uiPath -Encoding UTF8
    @{file=$File;uiRows=$rows.Count;resaved=$artifacts.resaved;chartActivation=$(if($activated){@($activated.charts | ForEach-Object activation)}else{@()})} | ConvertTo-Json
}
function Next-BReview {
    param([object]$Word,[string]$Observation,[Nullable[bool]]$Consistent=$null)
    if($Observation){
        $file=[string]$Word.ActiveDocument.Name
        Complete-BReview -Word $Word -File $file -Observation $Observation -Consistent $Consistent
    }
    $root=Split-Path $PSScriptRoot
    $plan=Get-Content (Join-Path $PSScriptRoot 'edited3-plan.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    $uiParsed=Get-Content (Join-Path $root 'ui-edited3.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    $done=@($uiParsed | ForEach-Object file)
    $next=$plan.selected | Where-Object {$_.file -notin $done} | Select-Object -First 1
    if($next){Begin-BReview -Word $Word -File $next.file; $next | Select-Object file,op,expect | ConvertTo-Json}
    else{'ALL_PLANNED_UI_REVIEWED'}
}
