param(
    [string] $Root = 'C:\word\real-word-round3-20260907',
    [string] $DocumentPath = 'C:\word\round3-work-20260907\ink-to-shape-working.docx',
    [switch] $QuitWhenNoDocuments
)

$ErrorActionPreference = 'Stop'
$utf8 = New-Object Text.UTF8Encoding($false)
$control = Join-Path $Root '_control'
$readouts = Join-Path $Root '_readouts'
[void][IO.Directory]::CreateDirectory($control)
[void][IO.Directory]::CreateDirectory($readouts)
[string]$targetPath = [IO.Path]::GetFullPath($DocumentPath)
if (-not (Test-Path -LiteralPath $targetPath -PathType Leaf)) { throw "Known working document does not exist: $targetPath" }
$environmentPath = Join-Path $Root 'environment.json'
$environmentHash = (Get-FileHash -LiteralPath $environmentPath -Algorithm SHA256).Hash
$environment = Get-Content -LiteralPath $environmentPath -Raw -Encoding UTF8 | ConvertFrom-Json
$original = $environment.originalSettings
$settingNames = @('UserName', 'UserInitials', 'UseLocalUserInfo', 'DisplayAlerts', 'SaveNormalPrompt', 'UpdateLinksAtOpen', 'UpdateFieldsAtPrint', 'UpdateLinksAtPrint')
foreach ($name in $settingNames) {
    if ($null -eq $original.PSObject.Properties[$name]) { throw "Original environment setting is missing: $name" }
}

function Write-ResumeJson([string] $Path, $Value) {
    [IO.File]::WriteAllText($Path, (ConvertTo-Json -InputObject $Value -Depth 50), $utf8)
}

function Assert-ResumeJsonSafe($Value, [int] $Depth = 0) {
    if ($null -eq $Value) { return }
    if ($Depth -gt 45) { throw 'Job output exceeds the supported JSON nesting depth.' }
    if ([Runtime.InteropServices.Marshal]::IsComObject($Value)) { throw 'A job emitted a COM object. Return scalar readings or JSON data, never a Word document or collection.' }
    if ($Value -is [Collections.IDictionary]) {
        foreach ($key in $Value.Keys) { Assert-ResumeJsonSafe $Value[$key] ($Depth + 1) }
    } elseif ($Value -is [Array] -or $Value -is [Collections.IList]) {
        foreach ($entry in $Value) { Assert-ResumeJsonSafe $entry ($Depth + 1) }
    } elseif ($Value -is [Management.Automation.PSCustomObject]) {
        foreach ($property in $Value.PSObject.Properties) { Assert-ResumeJsonSafe $property.Value ($Depth + 1) }
    }
}

$document = $null
$word = $null
$options = $null
$validated = $false
$stopJobId = $null
$restoration = $null
try {
    Write-ResumeJson (Join-Path $control 'worker-resume-state.json') ([ordered]@{ stage = 'binding_known_document'; pid = $PID; document = $targetPath; timestamp = [DateTime]::Now.ToString('o') })
    # Assign COM return values directly; never return them through helper pipelines.
    $document = [Runtime.InteropServices.Marshal]::BindToMoniker($targetPath)
    if ($null -eq $document) { throw 'BindToMoniker returned no document.' }
    [string]$boundPath = [IO.Path]::GetFullPath([string]$document.FullName)
    if (-not $boundPath.Equals($targetPath, [StringComparison]::OrdinalIgnoreCase)) { throw "Bound document path mismatch: $boundPath" }
    $word = $document.Application
    if ($null -eq $word) { throw 'The validated document returned no Word application.' }
    $validated = $true
    $options = $word.Options
    $options.UseLocalUserInfo = $true
    Write-ResumeJson (Join-Path $control 'worker-resume-state.json') ([ordered]@{ stage = 'ready'; pid = $PID; document = $boundPath; timestamp = [DateTime]::Now.ToString('o'); environment_sha256 = $environmentHash; originalSettingsSource = $environmentPath })
    Write-Output 'WORD_WORKER_RESUMED_READY'
    while ($true) {
        $request = Join-Path $control 'request.json'
        if (-not (Test-Path -LiteralPath $request)) { Start-Sleep -Milliseconds 200; continue }
        $job = Get-Content -LiteralPath $request -Raw -Encoding UTF8 | ConvertFrom-Json
        if ([string]$job.id -notmatch '^[A-Za-z0-9_.-]+$') { throw 'Invalid worker job identifier.' }
        Write-ResumeJson (Join-Path $control ($job.id + '.started.json')) ([ordered]@{ id = $job.id; action = $job.action; script = $job.script; pid = $PID; timestamp = [DateTime]::Now.ToString('o') })
        Write-ResumeJson (Join-Path $control 'worker-resume-state.json') ([ordered]@{ stage = 'job_starting'; id = $job.id; pid = $PID; timestamp = [DateTime]::Now.ToString('o') })
        Write-Output ('JOB_STARTING ' + $job.id)
        Remove-Item -LiteralPath $request
        if ($job.action -eq 'stop') { $stopJobId = [string]$job.id; break }
        try {
            $result = & ([scriptblock]::Create([string]$job.script))
            Assert-ResumeJsonSafe $result
            $response = @{ id = $job.id; ok = $true; result = $result }
        } catch {
            $response = @{ id = $job.id; ok = $false; error = $_.ToString(); stack = $_.ScriptStackTrace }
        }
        Write-ResumeJson (Join-Path $control ($job.id + '.json')) $response
        Write-ResumeJson (Join-Path $control 'worker-resume-state.json') ([ordered]@{ stage = 'job_finished'; id = $job.id; ok = $response.ok; pid = $PID; timestamp = [DateTime]::Now.ToString('o') })
        Write-Output ('JOB_FINISHED ' + $job.id)
    }
} finally {
    if ($validated) {
        $restorationPath = Join-Path $readouts 'settings-restored.json'
        $restoration = [ordered]@{
            started = [DateTime]::Now.ToString('o')
            completed = $null
            workerPid = $PID
            boundWorkingDocument = $targetPath
            method = 'Each original environment setting assigned independently and read back from the validated Word application.'
            originalSettings = $original
            actualSettings = [ordered]@{}
            checks = (New-Object Collections.ArrayList)
            pendingSetting = $null
            environmentSha256Before = $environmentHash
            environmentSha256After = $null
            environmentUnchanged = $false
            allSettingsRestored = $false
            remainingDocuments = $null
            cleanupErrors = (New-Object Collections.ArrayList)
            quitRequested = [bool]$QuitWhenNoDocuments
            wordQuit = $false
        }
        Write-ResumeJson $restorationPath $restoration
        foreach ($name in $settingNames) {
            $restoration.pendingSetting = $name
            Write-ResumeJson $restorationPath $restoration
            $expected = $original.$name
            $check = [ordered]@{ name = $name; expected = $expected; actual = $null; setError = $null; readError = $null; matches = $false }
            try {
                switch ($name) {
                    'UserName' { $word.UserName = [string]$expected }
                    'UserInitials' { $word.UserInitials = [string]$expected }
                    'DisplayAlerts' { $word.DisplayAlerts = [int]$expected }
                    default { $options.$name = [bool]$expected }
                }
            } catch { $check.setError = $_.ToString() }
            try {
                $actual = switch ($name) {
                    'UserName' { $word.UserName }
                    'UserInitials' { $word.UserInitials }
                    'DisplayAlerts' { $word.DisplayAlerts }
                    default { $options.$name }
                }
                if ($null -eq $actual) { throw "Word returned no restored-setting readout: $name" }
                $check.actual = $actual
                $restoration.actualSettings[$name] = $actual
                $check.matches = $actual -ceq $expected
            } catch { $check.readError = $_.ToString() }
            [void]$restoration.checks.Add($check)
            Write-ResumeJson $restorationPath $restoration
        }
        $restoration.pendingSetting = $null
        try {
            $restoration.environmentSha256After = (Get-FileHash -LiteralPath $environmentPath -Algorithm SHA256).Hash
            $restoration.environmentUnchanged = $restoration.environmentSha256After -eq $environmentHash
        } catch { [void]$restoration.cleanupErrors.Add($_.ToString()) }
        try { $restoration.remainingDocuments = [int]$word.Documents.Count }
        catch { [void]$restoration.cleanupErrors.Add($_.ToString()) }
        $failedSettings = @($restoration.checks | Where-Object { -not $_.matches -or $null -ne $_.setError -or $null -ne $_.readError })
        $restoration.allSettingsRestored = $restoration.checks.Count -eq $settingNames.Count -and $failedSettings.Count -eq 0 -and $restoration.environmentUnchanged
        if ($QuitWhenNoDocuments -and $restoration.remainingDocuments -eq 0) {
            try { [void]$word.Quit(); $restoration.wordQuit = $true }
            catch { [void]$restoration.cleanupErrors.Add($_.ToString()) }
        }
        $restoration.completed = [DateTime]::Now.ToString('o')
        Write-ResumeJson $restorationPath $restoration
        if ($stopJobId) {
            Write-ResumeJson (Join-Path $control ($stopJobId + '.json')) ([ordered]@{ id = $stopJobId; ok = $restoration.allSettingsRestored -and $restoration.cleanupErrors.Count -eq 0; result = $restoration })
            Write-Output ('JOB_FINISHED ' + $stopJobId)
        }
    }
    # No document is saved, selected, or closed by worker setup or cleanup.
    foreach ($reference in @($options, $document, $word)) {
        if ($null -ne $reference -and [Runtime.InteropServices.Marshal]::IsComObject($reference)) { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($reference) }
    }
}
if ($null -ne $restoration -and (-not $restoration.allSettingsRestored -or $restoration.cleanupErrors.Count -gt 0)) { exit 1 }
