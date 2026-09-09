param([string]$Root = 'C:\word\real-word-round2-20260907',[switch]$Attach)
$ErrorActionPreference = 'Stop'
$control = Join-Path $Root '_control'
New-Item -ItemType Directory -Path $control -Force | Out-Null
$word = if($Attach){[Runtime.InteropServices.Marshal]::GetActiveObject('Word.Application')}else{New-Object -ComObject Word.Application}
$word.Visible = $true
$word.DisplayAlerts = -1
$word.Options.SaveNormalPrompt = $false
$word.Options.UpdateLinksAtOpen = $false
$envInfo = [ordered]@{
    version = $word.Version
    build = $word.Build
    product = (Get-Item 'C:\Program Files\Microsoft Office\Root\Office16\WINWORD.EXE').VersionInfo | Select-Object ProductName,ProductVersion,FileVersion
    windows = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' | Select-Object ProductName,DisplayVersion,CurrentBuild,UBR
    office = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Office\ClickToRun\Configuration' | Select-Object ProductReleaseIds,VersionToReport,Platform
    date = (Get-Date).ToString('o')
    hwnd = $word.Hwnd
}
if(-not $Attach){$envInfo | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $Root 'environment.json') -Encoding utf8}
Write-Output 'WORD_WORKER_READY'
try {
    while ($true) {
        $request = Join-Path $control 'request.json'
        if (-not (Test-Path $request)) { Start-Sleep -Milliseconds 250; continue }
        $job = Get-Content -LiteralPath $request -Raw | ConvertFrom-Json
        Remove-Item -LiteralPath $request
        if ($job.action -eq 'stop') { break }
        try {
            $result = & ([scriptblock]::Create($job.script))
            @{ id=$job.id; ok=$true; result=$result } | ConvertTo-Json -Depth 35 | Set-Content (Join-Path $control ($job.id + '.json')) -Encoding utf8
        } catch {
            @{ id=$job.id; ok=$false; error=$_.ToString(); stack=$_.ScriptStackTrace } | ConvertTo-Json -Depth 10 | Set-Content (Join-Path $control ($job.id + '.json')) -Encoding utf8
        }
        Write-Output ('JOB_FINISHED ' + $job.id)
    }
} finally {
    if ($word.Documents.Count -eq 0) { $word.Quit() }
    [void][Runtime.InteropServices.Marshal]::ReleaseComObject($word)
}
