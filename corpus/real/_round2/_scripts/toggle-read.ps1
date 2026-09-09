[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)] [object] $Word,
    [string] $File,
    [string] $Sentence,
    [string] $AuditPath = (Join-Path $PSScriptRoot 'input-audit.json'),
    [switch] $WriteReports,
    [string] $SessionId = 'round2-20260907-live',
    [string] $OutputRoot = (Split-Path $PSScriptRoot -Parent),
    [ValidateRange(0, 3000)] [int] $SettleMilliseconds = 150
)

$ErrorActionPreference = 'Stop'
$helperId = 'toggle-read.ps1/v1'
$audit = Get-Content -LiteralPath $AuditPath -Raw -Encoding UTF8 | ConvertFrom-Json
if (-not $Word.Visible) { throw 'The passed Word application must be visible.' }
if ($Word.Documents.Count -lt 1) { throw 'The passed Word application has no open document.' }
$document = $Word.ActiveDocument
$activeName = [string] $document.Name
if ($File -and [IO.Path]::GetFileName($File) -ne $activeName) {
    throw "ActiveDocument is '$activeName', not the requested '$File'. Open the fixture before calling this helper."
}
$cases = @($audit.toggleRequiredRows | Where-Object { $_.file -eq $activeName })
if ($Sentence) { $cases = @($cases | Where-Object { $_.sentence -ceq $Sentence }) }
if ($cases.Count -eq 0) { throw "No required toggle case matches '$activeName' / '$Sentence'." }
if ($SessionId -notmatch '^[A-Za-z0-9_-]+$') { throw 'SessionId must contain only ASCII letters, digits, hyphens or underscores.' }

function Convert-WordBoolean([object] $RawValue) {
    if ([int] $RawValue -eq -1) { return $true }
    if ([int] $RawValue -eq 0) { return $false }
    return $null
}

function Select-Away([int] $TargetStart, [int] $TargetEnd) {
    $endPosition = [Math]::Max(0, [int] $document.Content.End - 1)
    $away = $endPosition
    if ($away -ge $TargetStart -and $away -le $TargetEnd) { $away = 0 }
    $awayRange = $document.Range($away, $away)
    $awayRange.Select()
    if ($SettleMilliseconds) { Start-Sleep -Milliseconds $SettleMilliseconds }
}

function Get-CaseRange([object] $Case) {
    if ($Case.property -eq 'HeaderDefault') {
        $page = $document.GoTo(1, 1, 2)
        $page.Select()
        if ([int] $Word.Selection.Information(3) -ne 2) {
            throw 'Word could not navigate to physical page 2.'
        }
        $sectionNumber = [int] $Word.Selection.Information(2)
        $header = $document.Sections.Item($sectionNumber).Headers.Item(1)
        return [pscustomobject] @{
            Range = $header.Range.Duplicate
            Section = $sectionNumber
            Page = 2
            LinkToPrevious = [bool] $header.LinkToPrevious
            HeaderExists = [bool] $header.Exists
        }
    }
    $target = $document.Content.Duplicate
    $target.TextRetrievalMode.IncludeHiddenText = $true
    $target.TextRetrievalMode.IncludeFieldCodes = $true
    $find = $target.Find
    $find.ClearFormatting()
    $find.Text = [string] $Case.sentence
    $find.Forward = $true
    $find.Wrap = 0
    $find.Format = $false
    $find.MatchCase = $true
    $find.MatchWholeWord = $false
    $find.MatchWildcards = $false
    if (-not $find.Execute()) {
        # Find can omit invisible runs; retrieval settings include them without changing the view.
        $complete = $document.Content.Duplicate
        $complete.TextRetrievalMode.IncludeHiddenText = $true
        $complete.TextRetrievalMode.IncludeFieldCodes = $true
        $completeText = [string] $complete.Text
        $offset = $completeText.IndexOf([string] $Case.sentence, [StringComparison]::Ordinal)
        if ($offset -lt 0) { throw "Sentence not found, including hidden text: $($Case.sentence)" }
        if ($completeText.Length -ne ([int] $complete.End - [int] $complete.Start)) {
            throw 'Retrieved text does not map directly to Word character positions; no guessed range was selected.'
        }
        $start = [int] $complete.Start + $offset
        $target = $complete.Duplicate
        $target.SetRange($start, $start + ([string] $Case.sentence).Length)
    }
    $target.TextRetrievalMode.IncludeHiddenText = $true
    $target.TextRetrievalMode.IncludeFieldCodes = $true
    if ([string] $target.Text -cne [string] $Case.sentence) {
        throw "Word Find returned an unexpected range for: $($Case.sentence)"
    }
    return [pscustomobject] @{ Range = $target; Section = $null; Page = $null; LinkToPrevious = $null; HeaderExists = $null }
}

$originalView = [pscustomobject] @{
    ShowHiddenText = [bool] $document.ActiveWindow.View.ShowHiddenText
    ShowAll = [bool] $document.ActiveWindow.View.ShowAll
}
if (@($cases | Where-Object property -eq 'Hidden').Count -gt 0) {
    $document.ActiveWindow.View.ShowHiddenText = $false
    $document.ActiveWindow.View.ShowAll = $false
}
$rows = @()
foreach ($case in $cases) {
    $row = [ordered] @{
        file = $case.file
        sentence = $case.sentence
        property = $case.property
        compatibilityMode = [int] $document.CompatibilityMode
        readMethod = $(if ($case.property -eq 'Hidden') { 'Selection.Font.Hidden with the entire sentence selected twice; collapsed caret is not used' } elseif ($case.property -eq 'HeaderDefault') { 'Selection.Range.Text with header selected twice' } else { 'Selection.Font property at a collapsed caret inside the sentence, twice' })
        declaredStyles = $case.declaredStyles
        priorWeb = $case.priorWeb
        desktopWord = $null
        samples = @()
        consistent = $false
        comparison = 'unread'
        uiVerified = $false
        uiEvidence = @()
        uiStatus = 'unverified; object-model reading only'
        rangeStart = $null
        rangeEnd = $null
        storyType = $null
        header = $null
        error = $null
        observedAt = [DateTimeOffset]::Now.ToString('o')
    }
    try {
        $located = Get-CaseRange $case
        $target = $located.Range
        $row.rangeStart = [int] $target.Start
        $row.rangeEnd = [int] $target.End
        $row.storyType = [int] $target.StoryType
        if ($case.property -eq 'HeaderDefault') {
            $row.header = [ordered] @{
                section = $located.Section; page = $located.Page
                linkToPrevious = $located.LinkToPrevious; exists = $located.HeaderExists
            }
        }
        for ($readNumber = 1; $readNumber -le 2; $readNumber++) {
            Select-Away $row.rangeStart $row.rangeEnd
            if ($case.property -eq 'HeaderDefault') {
                $target.Select()
                if ($SettleMilliseconds) { Start-Sleep -Milliseconds $SettleMilliseconds }
                $raw = [string] $Word.Selection.Range.Text
                $value = $raw.TrimEnd([char[]] @(13, 10, 7))
                $row.samples += [pscustomobject] @{
                    read = $readNumber; raw = $raw; value = $value
                    selectionStart = [int] $Word.Selection.Start
                    selectionEnd = [int] $Word.Selection.End
                }
            } else {
                if ($case.property -eq 'Hidden') {
                    # Word can report Hidden=0 for a caret inside a hidden run; select the run.
                    $target.Select()
                } else {
                    # Stay inside the run, because paired runs have no separating whitespace.
                    $position = $row.rangeStart + [Math]::Min(2, [Math]::Max(0, $row.rangeEnd - $row.rangeStart - 1))
                    $cursor = $target.Duplicate
                    $cursor.SetRange($position, $position)
                    $cursor.Select()
                }
                if ($SettleMilliseconds) { Start-Sleep -Milliseconds $SettleMilliseconds }
                $font = $Word.Selection.Font
                $raw = [int] $font.($case.property)
                $value = Convert-WordBoolean $raw
                $row.samples += [pscustomobject] @{
                    read = $readNumber; raw = $raw; value = $value
                    selectionStart = [int] $Word.Selection.Start
                    selectionEnd = [int] $Word.Selection.End
                    fontName = [string] $font.Name
                    fontNameAscii = [string] $font.NameAscii
                    fontNameFarEast = [string] $font.NameFarEast
                    showHiddenText = [bool] $document.ActiveWindow.View.ShowHiddenText
                    showAll = [bool] $document.ActiveWindow.View.ShowAll
                }
            }
        }
        $row.consistent = $row.samples[0].raw -ceq $row.samples[1].raw
        if ($row.consistent -and $null -ne $row.samples[0].value) {
            $row.desktopWord = $row.samples[0].value
            if ($null -eq $case.priorWeb) { $row.comparison = 'web-unknown' }
            elseif ($row.desktopWord -ceq $case.priorWeb) { $row.comparison = 'same-as-web' }
            else { $row.comparison = 'different-from-web' }
        } else { $row.comparison = 'indeterminate' }
    } catch { $row.error = $_.Exception.ToString() }
    $rows += [pscustomobject] $row
}

$result = [pscustomobject] @{
    helper = $helperId; sessionId = $SessionId
    measuredAt = [DateTimeOffset]::Now.ToString('o')
    file = $activeName; fullName = [string] $document.FullName
    wordVersion = [string] $Word.Version; wordBuild = [string] $Word.Build
    compatibilityMode = [int] $document.CompatibilityMode
    visible = [bool] $Word.Visible; documentSavedFlag = [bool] $document.Saved
    originalView = $originalView; rows = $rows
    note = 'No document opened, saved or closed. Selection remains at the last measured location. Hidden is measured with the entire sentence selected, while hidden-text display remains disabled. Other font properties use a collapsed caret inside the sentence. Actual document compatibility mode is recorded. UI confirmation remains required.'
}

if ($WriteReports) {
    $reportDirectory = Join-Path (Join-Path (Join-Path $OutputRoot '_scripts') 'toggle-read') $SessionId
    [void] [IO.Directory]::CreateDirectory($reportDirectory)
    $jsonPath = Join-Path $reportDirectory ([IO.Path]::GetFileNameWithoutExtension($activeName) + '.json')
    $merged = @{}
    if (Test-Path -LiteralPath $jsonPath) {
        $previous = Get-Content -LiteralPath $jsonPath -Raw -Encoding UTF8 | ConvertFrom-Json
        if ($previous.helper -ne $helperId -or $previous.sessionId -ne $SessionId) {
            throw "Refusing to consume an unrelated report: $jsonPath"
        }
        foreach ($previousRow in $previous.rows) { $merged[$previousRow.sentence] = $previousRow }
    }
    foreach ($row in $rows) { $merged[$row.sentence] = $row }
    $result.rows = @($audit.toggleRequiredRows | Where-Object file -eq $activeName | ForEach-Object {
        if ($merged.ContainsKey($_.sentence)) { $merged[$_.sentence] }
    })
    $utf8 = New-Object Text.UTF8Encoding($false)
    [IO.File]::WriteAllText($jsonPath, ($result | ConvertTo-Json -Depth 12), $utf8)

    $allReadings = @{}
    foreach ($reportFile in Get-ChildItem -LiteralPath $reportDirectory -Filter '*.json' -File) {
        $report = Get-Content -LiteralPath $reportFile.FullName -Raw -Encoding UTF8 | ConvertFrom-Json
        if ($report.helper -eq $helperId -and $report.sessionId -eq $SessionId) {
            foreach ($reportRow in $report.rows) { $allReadings[$reportRow.file + '|' + $reportRow.sentence] = $reportRow }
        }
    }
    $heading = '{"file":"\u6587\u4ef6","sentence":"\u53e5\u5b50","property":"\u5c5e\u6027","desktop":"\u684c\u9762 Word \u91cc\u5f00/\u5173\uff08\u6216\u9875\u7709\u6587\u5b57\uff09","comparison":"\u4e0e\u7f51\u9875\u7248\u7ed3\u8bba\u662f\u5426\u4e00\u81f4"}' | ConvertFrom-Json
    $lines = @('# Toggle Readings (Provisional)', '', "Session: $SessionId", '', 'All rows are UI unverified. Values below are two repeated Selection.Font readings or selected header text from desktop Word, not completed ribbon/dialog or visual confirmation.', '', "| $($heading.file) | $($heading.sentence) | $($heading.property) | $($heading.desktop) | $($heading.comparison) |", '| --- | --- | --- | --- | --- |')
    foreach ($case in $audit.toggleRequiredRows) {
        $reading = $allReadings[$case.file + '|' + $case.sentence]
        $display = 'not read; UI unverified'
        $comparison = '-'
        if ($reading) {
            if ($reading.error) { $display = 'read error; UI unverified' }
            elseif ($null -eq $reading.desktopWord) { $display = 'indeterminate; UI unverified' }
            elseif ($reading.desktopWord -is [bool]) { $display = $(if ($reading.desktopWord) { 'ON' } else { 'OFF' }) + '; UI unverified' }
            else { $display = ([string] $reading.desktopWord).Replace('|', '\|').Replace("`r", ' ').Replace("`n", ' ') + '; UI unverified' }
            $comparison = [string] $reading.comparison
        }
        $lines += "| $($case.file) | $($case.sentence) | $($case.property) | $display | $comparison |"
    }
    [IO.File]::WriteAllText((Join-Path $OutputRoot 'TOGGLE.md'), ($lines -join "`r`n") + "`r`n", $utf8)
}

$result
