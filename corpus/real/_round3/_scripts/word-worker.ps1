param([string]$Root='C:\word\real-word-round3-20260907')
$ErrorActionPreference='Stop'
$control=Join-Path $Root '_control'
[void][IO.Directory]::CreateDirectory($control)
$word=New-Object -ComObject Word.Application
if ($word.Documents.Count -ne 0) { throw 'New Word instance contains existing documents.' }
$original=[ordered]@{UserName=[string]$word.UserName;UserInitials=[string]$word.UserInitials;UseLocalUserInfo=[bool]$word.Options.UseLocalUserInfo;DisplayAlerts=$word.DisplayAlerts;SaveNormalPrompt=[bool]$word.Options.SaveNormalPrompt;UpdateLinksAtOpen=[bool]$word.Options.UpdateLinksAtOpen;UpdateFieldsAtPrint=[bool]$word.Options.UpdateFieldsAtPrint;UpdateLinksAtPrint=[bool]$word.Options.UpdateLinksAtPrint}
$word.Visible=$true
$word.DisplayAlerts=0
$word.Options.SaveNormalPrompt=$false
$word.Options.UpdateLinksAtOpen=$false
$word.Options.UpdateFieldsAtPrint=$false
$word.Options.UpdateLinksAtPrint=$false
$word.Options.UseLocalUserInfo=$true
$environment=[ordered]@{date=(Get-Date).ToString('o');version=[string]$word.Version;build=[string]$word.Build;product=(Get-Item 'C:\Program Files\Microsoft Office\Root\Office16\WINWORD.EXE').VersionInfo | Select-Object ProductName,ProductVersion,FileVersion;windows=Get-CimInstance Win32_OperatingSystem | Select-Object Caption,Version,BuildNumber,OSArchitecture;office=Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Office\ClickToRun\Configuration' | Select-Object ProductReleaseIds,VersionToReport,Platform;originalSettings=$original;hwnd=$word.Hwnd}
$environment | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $Root 'environment.json') -Encoding UTF8
Write-Output 'WORD_WORKER_READY'
try {
    while($true) {
        $request=Join-Path $control 'request.json'
        if (-not(Test-Path -LiteralPath $request)) { Start-Sleep -Milliseconds 200; continue }
        $job=Get-Content -LiteralPath $request -Raw -Encoding UTF8 | ConvertFrom-Json
        Remove-Item -LiteralPath $request
        if($job.action -eq 'stop'){break}
        try { $result=& ([scriptblock]::Create($job.script)); $response=@{id=$job.id;ok=$true;result=$result} }
        catch { $response=@{id=$job.id;ok=$false;error=$_.ToString();stack=$_.ScriptStackTrace} }
        $response | ConvertTo-Json -Depth 50 | Set-Content -LiteralPath (Join-Path $control ($job.id+'.json')) -Encoding UTF8
        Write-Output ('JOB_FINISHED '+$job.id)
    }
} finally {
    $word.UserName=$original.UserName
    $word.UserInitials=$original.UserInitials
    $word.Options.UseLocalUserInfo=$original.UseLocalUserInfo
    $word.DisplayAlerts=$original.DisplayAlerts
    foreach($name in 'SaveNormalPrompt','UpdateLinksAtOpen','UpdateFieldsAtPrint','UpdateLinksAtPrint'){$word.Options.$name=$original[$name]}
    @{finished=(Get-Date).ToString('o');userName=$word.UserName;useLocalUserInfo=$word.Options.UseLocalUserInfo;remainingDocuments=$word.Documents.Count;originalSettings=$original} | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $Root '_readouts/word-worker-finish.json') -Encoding UTF8
    if($word.Documents.Count -eq 0){$word.Quit()}
    [void][Runtime.InteropServices.Marshal]::ReleaseComObject($word)
}
