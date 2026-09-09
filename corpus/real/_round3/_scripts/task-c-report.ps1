[CmdletBinding()]
param(
    [string] $OutputRoot,
    [string] $ReferencePath,
    [string] $ReadoutDirectory,
    [string] $UiPath,
    [switch] $RequireComplete
)
$ErrorActionPreference = 'Stop'
$helperRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $OutputRoot) { $OutputRoot = Split-Path -Parent $helperRoot }
if (-not $ReferencePath) { $ReferencePath = Join-Path $helperRoot 'toggle-round2-reference.json' }
if (-not $ReadoutDirectory) { $ReadoutDirectory = Join-Path $helperRoot 'toggle15-read' }
if (-not $UiPath) { $UiPath = Join-Path $helperRoot 'task-c-ui.json' }
$reference = Get-Content -LiteralPath $ReferencePath -Raw -Encoding UTF8 | ConvertFrom-Json
$readouts = @(Get-ChildItem -LiteralPath $ReadoutDirectory -Filter '*.json' -File | ForEach-Object { Get-Content -LiteralPath $_.FullName -Raw -Encoding UTF8 | ConvertFrom-Json })
$ui = @()
if (Test-Path -LiteralPath $UiPath) { $ui = @((Get-Content -LiteralPath $UiPath -Raw -Encoding UTF8 | ConvertFrom-Json) | ForEach-Object { $_ }) }
$primary = @($readouts | Where-Object variant -eq 'supplied-compat15')
$converted = @($readouts | Where-Object variant -eq 'word-converted')
$primaryRows = @($primary | ForEach-Object { $_.rows })
$convertedRows = @($converted | ForEach-Object { $_.rows })
$environmentPath = Join-Path $OutputRoot 'environment.json'
$environment = if (Test-Path -LiteralPath $environmentPath) { Get-Content -LiteralPath $environmentPath -Raw -Encoding UTF8 | ConvertFrom-Json } else { $null }
$conversionPath = Join-Path $OutputRoot '_readouts/task-c-conversion.json'
$conversionOperation = if (Test-Path -LiteralPath $conversionPath) { Get-Content -LiteralPath $conversionPath -Raw -Encoding UTF8 | ConvertFrom-Json } else { $null }
$headers = '{"file":"\u6587\u4ef6","sentence":"\u53e5\u5b50","property":"\u5c5e\u6027","value":"\u517c\u5bb9\u6a21\u5f0f 15 \u4e0b\u5f00/\u5173","comparison":"\u4e0e\u7b2c\u4e8c\u8f6e\uff08\u6a21\u5f0f 12\uff09\u662f\u5426\u4e00\u81f4","on":"\u5f00","off":"\u5173","same":"\u4e00\u81f4","different":"\u4e0d\u4e00\u81f4"}' | ConvertFrom-Json
function Format-Value($Value) {
    if ($null -eq $Value) { return 'unread/indeterminate' }
    if ($Value -is [bool]) { return $(if ($Value) { $headers.on } else { $headers.off }) }
    return ([string] $Value).Replace('|', '\|').Replace("`r", ' ').Replace("`n", ' ')
}
$rows = foreach ($case in $reference.rows) {
    $matches = @($primaryRows | Where-Object { $_.baseFile -eq $case.file -and $_.sentence -ceq $case.sentence -and $_.property -eq $case.property })
    if ($matches.Count -gt 1) { throw "Duplicate primary row: $($case.file) / $($case.sentence)" }
    $row = if ($matches.Count) { $matches[0] } else { [pscustomobject] @{ file = ($case.file -replace '\.docx$', '-compat15.docx'); baseFile = $case.file; sentence = $case.sentence; property = $case.property; compatibilityMode = $null; samples = @(); desktopWord = $null; comparisonWithRound2 = 'unread'; consistent = $false; error = 'Not measured'; uiVerified = $false; uiObservation = $null; uiEvidence = @() } }
    $annotations = @($ui | Where-Object { $_.file -eq $row.file -and (-not $_.sentence -or $_.sentence -ceq $row.sentence) })
    $evidence = @($annotations | ForEach-Object { $_.evidence })
    foreach ($item in $evidence) {
        if (-not $item.path -or -not (Test-Path -LiteralPath (Join-Path $OutputRoot $item.path))) { throw "Missing UI evidence: $($item.path)" }
    }
    if ($annotations.Count) {
        $row.uiVerified = $evidence.Count -gt 0
        $row.uiObservation = @($annotations | ForEach-Object { $_.observation }) -join ' '
        $row.uiEvidence = $evidence
    }
    $row
}
$cross = foreach ($case in @($reference.rows | Where-Object file -eq 'toggle-other-toggles.docx')) {
    $p = @($rows | Where-Object { $_.baseFile -eq $case.file -and $_.sentence -ceq $case.sentence })[0]
    $m = @($convertedRows | Where-Object { $_.sentence -ceq $case.sentence -and $_.property -eq $case.property })
    if ($m.Count -gt 1) { throw "Duplicate converted row: $($case.sentence)" }
    $c = if ($m.Count) { $m[0] } else { $null }
    [pscustomobject] @{
        sentence = $case.sentence; property = $case.property
        priorMode12Value = $case.priorDesktopWord; suppliedMode15Value = $p.desktopWord
        wordConvertedValue = $(if ($c) { $c.desktopWord } else { $null })
        wordConvertedCompatibilityMode = $(if ($c) { $c.compatibilityMode } else { $null })
        allThreeAgree = [bool] ($c -and $c.consistent -and $p.consistent -and $null -ne $c.desktopWord -and $p.desktopWord -ceq $case.priorDesktopWord -and $c.desktopWord -ceq $case.priorDesktopWord)
        convertedSamples = $(if ($c) { $c.samples } else { @() })
    }
}
$problems = @()
if (@($primary).Count -ne 8 -or $primaryRows.Count -ne 25) { $problems += 'Expected eight supplied-compat15 readouts and25 readings.' }
if (@($rows | Where-Object { $_.compatibilityMode -ne 15 -or -not $_.consistent -or @($_.samples).Count -ne 2 -or $_.error }).Count) { $problems += 'One or more primary rows is incomplete or not compatibility15.' }
if (@($rows | Where-Object { -not $_.uiVerified }).Count) { $problems += 'One or more primary rows has no page/ribbon evidence.' }
foreach ($sentence in 'strike twice', 'dstrike twice', 'vanish once') {
    $r = @($rows | Where-Object { $_.baseFile -eq 'toggle-other-toggles.docx' -and $_.sentence -ceq $sentence })[0]
    if (-not @($r.uiEvidence | Where-Object kind -eq 'font-dialog').Count) { $problems += "Missing Font dialog evidence for: $sentence" }
}
if ($converted.Count -ne 1 -or $convertedRows.Count -ne 12 -or @($convertedRows | Where-Object { $_.compatibilityMode -ne 15 -or -not $_.consistent -or $_.error }).Count) { $problems += 'Word conversion cross-check is incomplete.' }
$summary = [pscustomobject] @{
    generatedAt = [DateTimeOffset]::Now.ToString('o'); primaryRows = $rows.Count
    measuredPrimaryRows = $primaryRows.Count; repeatedPrimaryReadings = @($primaryRows | ForEach-Object { $_.samples }).Count
    primaryMode15Rows = @($rows | Where-Object compatibilityMode -eq 15).Count
    sameAsRound2 = @($rows | Where-Object comparisonWithRound2 -eq 'same').Count
    differentFromRound2 = @($rows | Where-Object comparisonWithRound2 -eq 'different').Count
    uiSupportedRows = @($rows | Where-Object uiVerified -eq $true).Count
    convertedRows = $convertedRows.Count; allThreeAgreementRows = @($cross | Where-Object allThreeAgree -eq $true).Count
    incomplete = $problems; rows = @($rows); conversion = @($cross); conversionOperation = $conversionOperation
}
if ($RequireComplete -and $problems.Count) { throw ($problems -join ' ') }
$lines = @(
    '# Compatibility15 Toggle Readings', '',
    "Word / Office LTSC Professional Plus 2021 x64. WINWORD.EXE version: $($environment.product.FileVersion). Exact object-model version/build is retained in each raw readout and environment.json.", '',
    'Method: the same25 round2 points, with two real Word object-model readings per point. Font properties use a caret inside each sentence; Hidden uses the full sentence with hidden display disabled. The header point navigates to physical page2 and selects the inherited default header. GUI observations below are attached only when recorded evidence exists. No fixture is saved by the reading helper.', '',
    'The old eight fixtures are byte-identical to round2. Each supplied compatibility15 fixture adds word/settings.xml plus its required relationship/content-type registrations; document.xml and styles.xml bytes remain unchanged (round3-input-audit.json). Round2 toggle-para-and-char lacks a recorded numeric mode value, although its titlebar showed compatibility mode; this historical gap is retained.', '',
    'Earlier collector attempts are preserved in _scripts/toggle15-failed-attempts. Direct Content.Text offset mapping failed on Word table cell terminators, and substring-based locating confused strike/dstrike and caps/smallcaps. The corrected collector uses native Range.Find, case-insensitive text validation and the preserved source-range reference. The table below uses the latest readouts; unresolved reads remain explicit. The archived failures were helper-location errors, not Word-open failures.', '',
    "| $($headers.file) | $($headers.sentence) | $($headers.property) | $($headers.value) | $($headers.comparison) |", '| --- | --- | --- | --- | --- |'
)
foreach ($row in $rows) {
    $value = Format-Value $row.desktopWord
    if ($row.property -eq 'HeaderDefault') { $value += '; LinkToPrevious=' + $row.header.linkToPrevious }
    $samples = @($row.samples | ForEach-Object { Format-Value $_.raw }) -join ' / '
    $evidence = @($row.uiEvidence | ForEach-Object { '[' + $_.kind + '](' + $_.path.Replace('\', '/') + ')' }) -join ', '
    if (-not $evidence) { $evidence = 'GUI pending' }
    $comparison = if ($row.comparisonWithRound2 -eq 'same') { $headers.same } elseif ($row.comparisonWithRound2 -eq 'different') { $headers.different } else { $row.comparisonWithRound2 }
    $lines += '| ' + $row.file + ' | ' + $row.sentence + ' | ' + $row.property + ' | ' + $value + '; ' + $samples + '; ' + $evidence + ' | ' + $comparison + ' |'
}
$lines += @('', '## Word Conversion Cross-Check', '',
    'The conversion operation and SaveAs2 are performed separately in real Word. The reading helper never converts or saves. The converted file and raw reading retain its SHA256 and actual compatibility mode.', '',
    '[Converted DOCX](_resaved/toggle-other-toggles-converted.docx); [conversion operation record](_readouts/task-c-conversion.json).', '')
if ($conversionOperation) {
    $links = @($conversionOperation.uiEvidence | ForEach-Object { '[UI](' + ([string] $_).Replace('\', '/') + ')' }) -join ', '
    $lines += ('Actual method: ' + $conversionOperation.method)
    $lines += @('', "Source mode: $($conversionOperation.sourceCompatibilityMode); Word-converted mode: $($conversionOperation.convertedCompatibilityMode). The working copy SHA256 before conversion equals the preserved original: $($conversionOperation.workingSha256Before -eq $conversionOperation.sourceSha256).", '', $links, '')
}
$lines += @('| Sentence | Property | Round2 mode12 | Supplied mode15 | Word-converted | All three agree |', '| --- | --- | --- | --- | --- | --- |')
foreach ($row in $cross) { $lines += '| ' + $row.sentence + ' | ' + $row.property + ' | ' + (Format-Value $row.priorMode12Value) + ' | ' + (Format-Value $row.suppliedMode15Value) + ' | ' + (Format-Value $row.wordConvertedValue) + ' | ' + $row.allThreeAgree + ' |' }
$lines += @('', "Primary: $($summary.sameAsRound2)/25 same as round2; $($summary.differentFromRound2) different. Conversion cross-check: $($summary.allThreeAgreementRows)/12 three-way agreement.", '', '## Actual GUI Observations', '')
foreach ($item in $ui) { $lines += '- ' + $item.file + $(if ($item.sentence) { ' / ' + $item.sentence } else { '' }) + ': ' + $item.observation }
$lines += @('', '## Incomplete / Uncertain', '')
if ($problems.Count) { foreach ($problem in $problems) { $lines += '- ' + $problem } } else { $lines += 'No outstanding TaskC measurement or evidence items. The historical missing numeric mode for toggle-para-and-char remains documented above.' }
[IO.File]::WriteAllText((Join-Path $OutputRoot 'TOGGLE15.md'), ($lines -join "`r`n") + "`r`n", [Text.UTF8Encoding]::new($false))
[IO.Directory]::CreateDirectory((Join-Path $OutputRoot '_scripts')) | Out-Null
[IO.File]::WriteAllText((Join-Path $OutputRoot '_scripts/task-c-summary.json'), ($summary | ConvertTo-Json -Depth 16), [Text.UTF8Encoding]::new($false))
$summary | Select-Object measuredPrimaryRows,repeatedPrimaryReadings,primaryMode15Rows,sameAsRound2,differentFromRound2,uiSupportedRows,convertedRows,allThreeAgreementRows,incomplete
