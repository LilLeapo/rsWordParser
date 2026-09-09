[CmdletBinding()]
param(
    [string] $Round2Root = 'C:\word\real-word-round2-20260907',
    [string] $OutputPath
)
$ErrorActionPreference = 'Stop'
if (-not $OutputPath) { $OutputPath = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) 'toggle-round2-reference.json' }
$auditPath = Join-Path $Round2Root '_scripts\input-audit.json'
$audit = Get-Content -LiteralPath $auditPath -Raw -Encoding UTF8 | ConvertFrom-Json
$sourceRows = @($audit.toggleRequiredRows)
$rows = foreach ($case in $sourceRows) {
    $readoutPath = Join-Path $Round2Root ('_scripts\toggle-read\round2-20260907-live\' + [IO.Path]::GetFileNameWithoutExtension($case.file) + '.json')
    $readout = Get-Content -LiteralPath $readoutPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $matches = @($readout.rows | Where-Object { $_.sentence -ceq $case.sentence -and $_.property -ceq $case.property })
    if ($matches.Count -ne 1) { throw "Expected one prior row: $($case.file) / $($case.sentence)" }
    $prior = $matches[0]
    if ($null -eq $prior.desktopWord -or -not $prior.consistent -or @($prior.samples).Count -ne 2) {
        throw "Prior measurement incomplete: $($case.file) / $($case.sentence)"
    }
    [pscustomobject] @{
        file = $case.file
        sentence = $case.sentence
        property = $case.property
        declaredStyles = $case.declaredStyles
        priorDesktopWord = $prior.desktopWord
        priorSamples = $prior.samples
        priorCompatibilityMode = $prior.compatibilityMode
        priorFileCompatibilityMode = $readout.compatibilityMode
        priorCompatibilityNote = $(if ($null -eq $prior.compatibilityMode -and $null -eq $readout.compatibilityMode) { 'Exact numeric value absent in round2; prior GUI title showed compatibility mode.' } else { 'Recorded numeric value retained.' })
        priorReadoutPath = $readoutPath
        priorReadoutSha256 = (Get-FileHash -LiteralPath $readoutPath -Algorithm SHA256).Hash
        priorUiObservation = $prior.uiObservation
        priorUiEvidence = $prior.uiEvidence
        priorRangeStart = $prior.rangeStart
        priorRangeEnd = $prior.rangeEnd
    }
}
if (@($rows).Count -ne 25 -or @($rows.file | Sort-Object -Unique).Count -ne 8) { throw 'Expected 25 rows across eight fixtures.' }
$result = [pscustomobject] @{
    schemaVersion = 1
    method = 'Snapshot of completed round2 raw Word readings; no new Word measurement.'
    sourceRoot = $Round2Root
    createdAt = [DateTimeOffset]::Now.ToString('o')
    rows = @($rows)
}
[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($OutputPath)) | Out-Null
[IO.File]::WriteAllText($OutputPath, ($result | ConvertTo-Json -Depth 15), [Text.UTF8Encoding]::new($false))
$result | Select-Object schemaVersion, createdAt, @{n='rows';e={$_.rows.Count}}
