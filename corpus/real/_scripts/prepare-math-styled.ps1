#requires -Version 5.1
param([string]$Root = (Split-Path -Parent $PSScriptRoot))
$ErrorActionPreference = 'Stop'
$case = 'math/math-styled'
$template = Join-Path $Root ($case + '.docx')
if (-not (Test-Path -LiteralPath $template -PathType Leaf)) { throw 'The original math-styled template is missing.' }
$sourceHash = (Get-FileHash -LiteralPath $template -Algorithm SHA256).Hash
$stamp = [DateTime]::Now.ToString('yyyyMMdd-HHmmss-fff')
$resultPath = Join-Path $PSScriptRoot ('math-styled-prepare-' + $stamp + '.json')
$record = [ordered]@{
    action = 'Prepare'
    case = $case
    template = $template
    templateSha256Before = $sourceHash
    officeSaveMethodsCalled = $false
    status = 'preparing'
    phase = 'connect'
    log = $resultPath
}
$doc = $null
$word = $null
try {
    try { $word = [Runtime.InteropServices.Marshal]::GetActiveObject('Word.Application') }
    catch { $word = New-Object -ComObject Word.Application }
    $word.Visible = $true
    foreach ($candidate in $word.Documents) {
        $tag = $null
        try { $tag = [string]$candidate.Variables.Item('CorpusUICase').Value } catch {}
        if ($tag -eq $case -and $candidate.Path -eq '') { throw 'An unsaved math-styled UI case is already open.' }
    }
    $record.phase = 'new-document-from-template'
    $doc = $word.Documents.Add([string]$template, $false, 0, $true)
    if ($doc.Path -ne '') { throw 'Documents.Add did not return an unsaved document.' }
    try { $doc.Variables.Item('CorpusUICase').Value = $case }
    catch { [void]$doc.Variables.Add('CorpusUICase', $case) }
    $doc.Activate()
    $doc.ActiveWindow.View.Type = 3
    $doc.ActiveWindow.View.Zoom.Percentage = 130
    $record.documentName = [string]$doc.Name
    $record.windowHwnd = [int]$doc.ActiveWindow.Hwnd
    $record.wordVersion = [string]$word.Version
    $record.wordBuild = [string]$word.Build
    if ($doc.OMaths.Count -ne 1) { throw 'Expected exactly one equation in the template.' }
    $math = $doc.OMaths.Item(1)
    $math.Range.Font.Size = 14
    $math.Range.Font.Color = 0
    $math.Range.ParagraphFormat.Alignment = 1
    $math.Justification = 2
    $fraction = $null
    for ($i = 1; $i -le $math.Functions.Count; $i++) {
        if ($math.Functions.Item($i).Type -eq 7) { $fraction = $math.Functions.Item($i).Frac; break }
    }
    if ($null -eq $fraction) { throw 'The equation has no fraction.' }
    $aRange = $fraction.Num.Range.Characters.Item(1).Duplicate
    $aStart = [int]$aRange.Start
    $aEnd = [int]$aRange.End
    $mathA = [char]::ConvertFromUtf32(0x1D44E)
    if ([string]$aRange.Text -notin @('a', $mathA)) { throw 'The first numerator character is not a.' }
    $aRange.Select()
    $record.phase = 'normal-text-command'
    if (-not $word.CommandBars.GetEnabledMso('EquationNormalText')) { throw 'EquationNormalText is disabled for the selected character.' }
    $record.normalTextPressedBefore = [bool]$word.CommandBars.GetPressedMso('EquationNormalText')
    if (-not $record.normalTextPressedBefore) { $word.CommandBars.ExecuteMso('EquationNormalText') }
    $aRange = $fraction.Num.Range.Characters.Item(1).Duplicate
    if ([string]$aRange.Text -notin @('a', $mathA)) { throw 'The normal-text command changed the target range unexpectedly.' }
    $aRange.Text = 'a'
    $aRange = $doc.Range($aStart, $aStart + 1)
    $aRange.Font.Name = 'Cambria Math'
    $aRange.Font.Color = 255
    $aRange.Font.Size = 20
    $aRange.Font.Italic = -1
    $record.phase = 'read-only-xml-validation'
    # Rebuilding or resizing the whole equation here could normalize the local size.
    $xml = [xml]$doc.Content.WordOpenXML
    $ns = [Xml.XmlNamespaceManager]::new($xml.NameTable)
    $ns.AddNamespace('pkg', 'http://schemas.microsoft.com/office/2006/xmlPackage')
    $ns.AddNamespace('w', 'http://schemas.openxmlformats.org/wordprocessingml/2006/main')
    $ns.AddNamespace('m', 'http://schemas.openxmlformats.org/officeDocument/2006/math')
    $body = $xml.SelectSingleNode('/pkg:package/pkg:part[@pkg:name="/word/document.xml"]/pkg:xmlData/w:document/w:body', $ns)
    if ($null -eq $body) { throw 'WordOpenXML did not expose the document body.' }
    $aRuns = $body.SelectNodes('.//m:r[m:t="a"]', $ns)
    $otherRuns = $body.SelectNodes('.//m:r[m:t and not(m:t="a")]', $ns)
    $checks = [ordered]@{
        oneEquation = $body.SelectNodes('.//m:oMath', $ns).Count -eq 1
        fractionPresent = $body.SelectNodes('.//m:f', $ns).Count -eq 1
        superscriptPresent = $body.SelectNodes('.//m:sSup', $ns).Count -eq 1
        oneTargetRun = $aRuns.Count -eq 1
        targetNormalText = $body.SelectNodes('.//m:r[m:t="a"]/m:rPr/m:nor[not(@m:val) or @m:val="1" or @m:val="true"]', $ns).Count -eq 1
        targetRed = $body.SelectNodes('.//m:r[m:t="a"]/w:rPr/w:color[@w:val="FF0000"]', $ns).Count -eq 1
        targetTwentyPoint = $body.SelectNodes('.//m:r[m:t="a"]/w:rPr/w:sz[@w:val="40"]', $ns).Count -eq 1
        otherTextRunsFourteenPoint = $otherRuns.Count -gt 0 -and $body.SelectNodes('.//m:r[m:t and not(m:t="a")]/w:rPr/w:sz[@w:val="28"]', $ns).Count -eq $otherRuns.Count
        stillUnsaved = $doc.Path -eq ''
    }
    $record.checks = $checks
    $record.mathParagraphXml = @($body.SelectNodes('.//m:oMathPara', $ns) | ForEach-Object OuterXml)
    $record.runSizesHalfPoints = @($body.SelectNodes('.//m:r/w:rPr/w:sz/@w:val', $ns) | ForEach-Object Value | Sort-Object -Unique)
    $record.text = [string]$doc.Content.Text
    $record.preparedTargetRange = [ordered]@{ start = [int]$aRange.Start; end = [int]$aRange.End; text = [string]$aRange.Text; size = [single]$aRange.Font.Size }
    $failed = @($checks.Keys | Where-Object { -not $checks[$_] })
    if ($failed.Count -gt 0) { throw ('Preparation validation failed: ' + ($failed -join ', ')) }
    $record.status = 'prepared-for-ui-review'
    $record.phase = 'prepared-not-saved'
} catch {
    $record.status = 'failed-left-open'
    $record.error = $_.Exception.Message
} finally {
    $record.templateSha256After = (Get-FileHash -LiteralPath $template -Algorithm SHA256).Hash
    $record.templateUnchanged = $record.templateSha256After -eq $sourceHash
    if (-not $record.templateUnchanged) { $record.status = 'failed-template-hash-changed' }
    if ($null -ne $doc) {
        try {
            $doc.Activate()
            $caret = $doc.Paragraphs.Item(1).Range.Duplicate
            $caret.Collapse(1)
            $caret.Select()
            $record.documentLeftOpen = $true
            $record.documentPath = [string]$doc.Path
            $record.documentSavedFlag = [bool]$doc.Saved
        } catch { $record.leaveOpenError = $_.Exception.Message }
    }
    $record | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $resultPath -Encoding utf8
}
$record | ConvertTo-Json -Depth 12
if ($record.status -ne 'prepared-for-ui-review') { exit 1 }
