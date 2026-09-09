[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)] [object] $Word,
    [Parameter(Mandatory = $true)] [string] $InputPath,
    [string] $ReferencePath,
    [string] $ReportDirectory,
    [string] $Sentence,
    [switch] $Converted,
    [switch] $SelectOnly,
    [ValidateRange(0, 3000)] [int] $SettleMilliseconds = 100
)
$ErrorActionPreference = 'Stop'
$helperRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $ReferencePath) { $ReferencePath = Join-Path $helperRoot 'toggle-round2-reference.json' }
if (-not $ReportDirectory) { $ReportDirectory = Join-Path $helperRoot 'toggle15-read' }
$helperId = 'task-c-read.ps1/v1'
if (-not $Word.Visible -or $Word.Documents.Count -lt 1) { throw 'Pass the visible Word application with the requested fixture already open.' }
$document = $Word.ActiveDocument
$expectedFullName = [IO.Path]::GetFullPath($InputPath)
if (-not [string]::Equals([IO.Path]::GetFullPath([string] $document.FullName), $expectedFullName, [StringComparison]::OrdinalIgnoreCase)) {
    throw "ActiveDocument is '$($document.FullName)', expected '$expectedFullName'."
}
$activeName = [string] $document.Name
$baseName = if ($Converted) { 'toggle-other-toggles.docx' } else { $activeName -replace '-compat15\.docx$', '.docx' }
if (-not $Converted -and $baseName -eq $activeName) { throw 'The primary run must use a -compat15 fixture. Use -Converted only for the Word-converted cross-check.' }
$reference = Get-Content -LiteralPath $ReferencePath -Raw -Encoding UTF8 | ConvertFrom-Json
$allCases = @($reference.rows | Where-Object file -eq $baseName)
$cases = @($allCases)
if ($Sentence) { $cases = @($cases | Where-Object { $_.sentence -ceq $Sentence }) }
if (-not $cases.Count) { throw "No matching measurement: $activeName / $Sentence" }
if ($SelectOnly -and $cases.Count -ne 1) { throw '-SelectOnly requires one -Sentence.' }

function Get-SharedHash([string] $Path) {
    $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
    $sha = [Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-', '') }
    finally { $sha.Dispose(); $stream.Dispose() }
}
function Get-CaseRange([object] $Case) {
    if ($Case.property -eq 'HeaderDefault') {
        $pageRange = $document.GoTo(1, 1, 2)
        $pageRange.Select()
        if ([int] $Word.Selection.Information(3) -ne 2) { throw 'Could not navigate to physical page 2.' }
        $section = [int] $Word.Selection.Information(2)
        $header = $document.Sections.Item($section).Headers.Item(1)
        return [pscustomobject] @{ Range = $header.Range.Duplicate; section = $section; page = 2; linkToPrevious = [bool] $header.LinkToPrevious; exists = [bool] $header.Exists }
    }
    $complete = $document.Content.Duplicate
    $complete.TextRetrievalMode.IncludeHiddenText = $true
    $complete.TextRetrievalMode.IncludeFieldCodes = $true
    $needle = [string] $Case.sentence
    $target = $complete.Duplicate
    $find = $target.Find
    $find.ClearFormatting()
    $find.Text = $needle
    $find.Forward = $true
    $find.Wrap = 0
    $find.Format = $false
    $find.MatchCase = $false
    $find.MatchWholeWord = $false
    $find.MatchWildcards = $false
    $find.MatchSoundsLike = $false
    $find.MatchAllWordForms = $false
    if ($find.Execute()) {
        $target.TextRetrievalMode.IncludeHiddenText = $true
        $target.TextRetrievalMode.IncludeFieldCodes = $true
        $actual = [string] $target.Text
        if (-not [string]::Equals($actual, $needle, [StringComparison]::OrdinalIgnoreCase)) { throw "Word Find returned unexpected text: $actual" }
        if (-not $Converted -and $null -ne $Case.priorRangeStart -and [int] $target.Start -ne [int] $Case.priorRangeStart) {
            throw "Found text at $($target.Start), different from the byte-identical round2 body reference $($Case.priorRangeStart): $needle"
        }
        return [pscustomobject] @{ Range = $target; actualText = $actual; matchedCase = 'Word Range.Find; ordinal-ignore-case validation' }
    }
    # Hidden text can be omitted by Find. Offset fallback requires an exact map;
    # table cell terminators can make retrieved text longer than Word positions.
    if ($Case.property -ne 'Hidden') { throw "Word Find could not locate the requested sentence: $needle" }
    $text = [string] $complete.Text
    if ($text.Length -ne ([int] $complete.End - [int] $complete.Start)) { throw 'Word text length does not map to character positions.' }
    $offset = $text.IndexOf($needle, [StringComparison]::Ordinal)
    $matchedCase = 'exact'
    if ($offset -lt 0) {
        # Word returns AllCaps text in uppercase; retain the actual returned spelling.
        $offset = $text.IndexOf($needle, [StringComparison]::OrdinalIgnoreCase)
        $matchedCase = 'ordinal-ignore-case'
    }
    if ($offset -lt 0) { throw "Sentence absent from complete Word text: $needle" }
    if ($text.IndexOf($needle, $offset + 1, [StringComparison]::OrdinalIgnoreCase) -ge 0) { throw "Sentence is not unique: $needle" }
    $start = [int] $complete.Start + $offset
    $target = $complete.Duplicate
    $target.SetRange($start, $start + $needle.Length)
    $actual = [string] $target.Text
    if (-not [string]::Equals($actual, $needle, [StringComparison]::OrdinalIgnoreCase)) { throw 'Located range text did not match the requested sentence.' }
    return [pscustomobject] @{ Range = $target; actualText = $actual; matchedCase = $matchedCase }
}
function Select-Away([object] $Target) {
    $position = [Math]::Max(0, [int] $document.Content.End - 1)
    if ($position -ge [int] $Target.Start -and $position -le [int] $Target.End) { $position = 0 }
    $document.Range($position, $position).Select()
    if ($SettleMilliseconds) { Start-Sleep -Milliseconds $SettleMilliseconds }
}
function Select-Measurement([object] $Case, [object] $Target) {
    if ($Case.property -eq 'HeaderDefault' -or $Case.property -eq 'Hidden') { $Target.Select() }
    else {
        $position = [int] $Target.Start + [Math]::Min(2, [Math]::Max(0, [int] $Target.End - [int] $Target.Start - 1))
        $cursor = $Target.Duplicate
        $cursor.SetRange($position, $position)
        $cursor.Select()
    }
    if ($SettleMilliseconds) { Start-Sleep -Milliseconds $SettleMilliseconds }
}

$originalView = [pscustomobject] @{ showHiddenText = [bool] $document.ActiveWindow.View.ShowHiddenText; showAll = [bool] $document.ActiveWindow.View.ShowAll }
$document.ActiveWindow.View.ShowHiddenText = $false
$document.ActiveWindow.View.ShowAll = $false
if ($SelectOnly) {
    $located = Get-CaseRange $cases[0]
    Select-Measurement $cases[0] $located.Range
    [pscustomobject] @{ file = $activeName; sentence = $cases[0].sentence; property = $cases[0].property; rangeStart = [int] $located.Range.Start; rangeEnd = [int] $located.Range.End; selectionStart = [int] $Word.Selection.Start; selectionEnd = [int] $Word.Selection.End; compatibilityMode = [int] $document.CompatibilityMode; note = 'Selection only; no document save or close. Open Font dialog through the UI for evidence.' }
    return
}
$inputHash = Get-SharedHash $expectedFullName
$rows = foreach ($case in $cases) {
    $row = [ordered] @{
        file = $activeName; baseFile = $baseName; sentence = $case.sentence; property = $case.property
        compatibilityMode = [int] $document.CompatibilityMode
        priorDesktopWord = $case.priorDesktopWord; priorCompatibilityMode = $case.priorCompatibilityMode
        priorCompatibilityNote = $case.priorCompatibilityNote
        samples = @(); desktopWord = $null; consistent = $false; comparisonWithRound2 = 'unread'
        readMethod = $(if ($case.property -eq 'Hidden') { 'Selection.Font.Hidden with entire sentence selected twice, hidden display disabled' } elseif ($case.property -eq 'HeaderDefault') { 'Physical page2 navigation; selected default header text twice' } else { 'Selection.Font property with caret two characters into sentence twice' })
        rangeStart = $null; rangeEnd = $null; actualText = $null; matchedCase = $null; header = $null
        uiVerified = $false; uiObservation = $null; uiEvidence = @(); error = $null
        observedAt = [DateTimeOffset]::Now.ToString('o')
    }
    try {
        $located = Get-CaseRange $case
        $target = $located.Range
        $row.rangeStart = [int] $target.Start
        $row.rangeEnd = [int] $target.End
        if ($case.property -eq 'HeaderDefault') { $row.header = [pscustomobject] @{ section = $located.section; page = $located.page; linkToPrevious = $located.linkToPrevious; exists = $located.exists } }
        else { $row.actualText = $located.actualText; $row.matchedCase = $located.matchedCase }
        foreach ($readNumber in 1, 2) {
            Select-Away $target
            Select-Measurement $case $target
            if ($case.property -eq 'HeaderDefault') {
                $raw = [string] $Word.Selection.Range.Text
                $value = $raw.TrimEnd([char[]] @(13, 10, 7))
            } else {
                $font = $Word.Selection.Font
                $raw = [int] $font.($case.property)
                $value = if ($raw -eq -1) { $true } elseif ($raw -eq 0) { $false } else { $null }
            }
            $row.samples += [pscustomobject] @{
                read = $readNumber; raw = $raw; value = $value
                selectionStart = [int] $Word.Selection.Start; selectionEnd = [int] $Word.Selection.End
                showHiddenText = [bool] $document.ActiveWindow.View.ShowHiddenText; showAll = [bool] $document.ActiveWindow.View.ShowAll
            }
        }
        $row.consistent = $row.samples[0].raw -ceq $row.samples[1].raw
        if ($row.consistent -and $null -ne $row.samples[0].value) {
            $row.desktopWord = $row.samples[0].value
            $row.comparisonWithRound2 = if ($row.desktopWord -ceq $case.priorDesktopWord) { 'same' } else { 'different' }
        } else { $row.comparisonWithRound2 = 'indeterminate' }
    } catch { $row.error = $_.Exception.ToString() }
    [pscustomobject] $row
}
$result = [pscustomobject] @{
    helper = $helperId; variant = $(if ($Converted) { 'word-converted' } else { 'supplied-compat15' })
    file = $activeName; fullName = $expectedFullName; sourceSha256 = $inputHash; sourceSha256AfterRead = (Get-SharedHash $expectedFullName)
    referenceSha256 = (Get-FileHash -LiteralPath $ReferencePath -Algorithm SHA256).Hash
    wordVersion = [string] $Word.Version; wordBuild = [string] $Word.Build; compatibilityMode = [int] $document.CompatibilityMode
    documentSavedFlag = [bool] $document.Saved; originalView = $originalView; rows = @($rows)
    measuredAt = [DateTimeOffset]::Now.ToString('o')
    note = 'Reads current document only; no open, save, conversion or close. GUI evidence remains unverified until actual screenshots are added. Selection remains at last measured point.'
}
if ($result.sourceSha256 -ne $result.sourceSha256AfterRead) { throw 'Input bytes changed during read.' }
[IO.Directory]::CreateDirectory($ReportDirectory) | Out-Null
$reportPath = Join-Path $ReportDirectory ([IO.Path]::GetFileNameWithoutExtension($activeName) + '.json')
if ($Sentence -and (Test-Path -LiteralPath $reportPath)) {
    $prior = Get-Content -LiteralPath $reportPath -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($prior.helper -ne $helperId -or $prior.sourceSha256 -ne $inputHash) { throw 'Existing result belongs to different helper or document bytes.' }
    $merged = @{}
    foreach ($existing in $prior.rows) { $merged[$existing.sentence] = $existing }
    foreach ($row in $rows) { $merged[$row.sentence] = $row }
    $result.rows = @($allCases | ForEach-Object { if ($merged.ContainsKey($_.sentence)) { $merged[$_.sentence] } })
}
[IO.File]::WriteAllText($reportPath, ($result | ConvertTo-Json -Depth 15), [Text.UTF8Encoding]::new($false))
$result
