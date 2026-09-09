param([string]$Root = 'C:\word\real-word-round2-20260907')
$ErrorActionPreference = 'Stop'
$resultsPath = Join-Path $Root '_scripts/task-d-results.json'
$uiPath = Join-Path $Root '_scripts/task-d-ui.json'
$archiveRoot = Join-Path $Root '_trials/d-before-adoption-docx'
if (Test-Path -LiteralPath $archiveRoot) { throw 'Adoption archive already exists; refusing to repeat.' }
$results = @(Get-Content -LiteralPath $resultsPath -Raw -Encoding UTF8 | ConvertFrom-Json)
$uiRows = @(Get-Content -LiteralPath $uiPath -Raw -Encoding UTF8 | ConvertFrom-Json)
$replacementRows = @(
    Get-Content -LiteralPath (Join-Path $Root '_trials/table-style-probe2/result.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    Get-Content -LiteralPath (Join-Path $Root '_trials/d-layout-retry/_scripts/task-d-results.json') -Raw -Encoding UTF8 | ConvertFrom-Json
) | ForEach-Object { $_ }
$uiReplacements = @{
    'revisions2/rev-table' = @{
        observation = 'Viewed the current Word document immediately after its first save/PDF export. Red table borders are visible; the first row spans both original cells and retains their two paragraphs. The inserted row has two cells. The deleted original second row is hidden in Simple Markup; the revision bar is visible. Native row insert/delete, tcPrChange and tblPrChange are established by separate package checks. Closed without another save.'
        screenshots = @('screenshots/d-rev-table-styled-final-0.jpg')
    }
    'fields2/fields-toc-stale' = @{
        observation = 'Viewed the current Word page after the first save and PDF export. TOC shows Original heading one/two/three and page 1 for all three. Body headings one and two show Changed, while heading three remains Original. No bookmark error is visible; before/after markers and all three body paragraphs are present. Closed without another save.'
        screenshots = @('screenshots/d-fields-toc-stale-final-0.jpg')
    }
    'shapes2/textbox-linked' = @{
        observation = 'Viewed the current Word page after the first save and PDF export. Two separate textboxes sit below the anchor paragraph, clear of before and after. Left contains Line 01 through Line 05; right contains Line 06 through Line 10, each exactly once. Text and frames do not overlap body text. Closed without another save.'
        screenshots = @('screenshots/d-textbox-linked-final-0.jpg')
    }
}
if ($results.Count -ne 17 -or $uiRows.Count -ne 17 -or @($replacementRows).Count -ne 3) { throw 'Unexpected record count.' }
foreach ($replacement in $replacementRows) {
    $case = [string]$replacement.case
    if (@($results | Where-Object case -eq $case).Count -ne 1 -or @($uiRows | Where-Object case -eq $case).Count -ne 1) { throw "Case mismatch: $case" }
    if (-not $replacement.selfcheck.passed -or -not $replacement.requiredOperationsPassed -or $replacement.error) { throw "Replacement is incomplete: $case" }
    if ((Get-FileHash -LiteralPath $replacement.path -Algorithm SHA256).Hash -ne $replacement.sha256) { throw "Replacement DOCX changed: $case" }
    if (-not (Test-Path -LiteralPath $replacement.pdf -PathType Leaf)) { throw "Missing replacement PDF: $case" }
    foreach ($evidence in $uiReplacements[$case].screenshots) {
        if (-not (Test-Path -LiteralPath (Join-Path $Root $evidence) -PathType Leaf)) { throw "Missing UI evidence: $evidence" }
    }
}
[void][IO.Directory]::CreateDirectory($archiveRoot)
Copy-Item -LiteralPath $resultsPath -Destination (Join-Path $archiveRoot 'all-results.json')
Copy-Item -LiteralPath $uiPath -Destination (Join-Path $archiveRoot 'all-ui.json')
$adoptions = @()
foreach ($replacement in $replacementRows) {
    $case = [string]$replacement.case
    $old = @($results | Where-Object case -eq $case)[0]
    $caseArchive = Join-Path $archiveRoot $case
    [void][IO.Directory]::CreateDirectory($caseArchive)
    Copy-Item -LiteralPath $old.path -Destination (Join-Path $caseArchive 'document.docx')
    Copy-Item -LiteralPath $old.pdf -Destination (Join-Path $caseArchive 'document.pdf')
    if ((Get-FileHash -LiteralPath (Join-Path $caseArchive 'document.docx') -Algorithm SHA256).Hash -ne $old.sha256) { throw "Archived DOCX hash mismatch: $case" }
    $nativePath = $replacement.path
    $nativePdf = $replacement.pdf
    $canonicalPath = Join-Path $Root ($case + '.docx')
    $canonicalPdf = Join-Path $Root ('_previews/' + $case + '.pdf')
    Copy-Item -LiteralPath $nativePath -Destination $canonicalPath -Force
    Copy-Item -LiteralPath $nativePdf -Destination $canonicalPdf -Force
    if ((Get-FileHash -LiteralPath $canonicalPath -Algorithm SHA256).Hash -ne $replacement.sha256) { throw "Adopted DOCX hash mismatch: $case" }
    $provenance = [ordered]@{nativeSavedPath=$nativePath;nativeExportedPdf=$nativePdf;oldSha256=$old.sha256;archive=$caseArchive;method='Byte copy of a verified native Word first-save output; no package edits or additional Word save.';closedWithoutAnotherSave=$true;adopted=(Get-Date).ToString('o')}
    $replacement.path = $canonicalPath
    $replacement.pdf = $canonicalPdf
    $replacement | Add-Member -NotePropertyName adoption -NotePropertyValue $provenance -Force
    $replacement | Add-Member -NotePropertyName visualChecked -NotePropertyValue $true -Force
    $replacement.status = 'saved; package selfcheck passed; direct Word UI inspected; independent PDF review pending'
    for ($i=0; $i -lt $results.Count; $i++) { if ($results[$i].case -eq $case) { $results[$i] = $replacement } }
    for ($i=0; $i -lt $uiRows.Count; $i++) { if ($uiRows[$i].case -eq $case) { $uiRows[$i] = [pscustomobject]@{case=$case;observation=$uiReplacements[$case].observation;screenshots=$uiReplacements[$case].screenshots} } }
    $adoptions += [ordered]@{case=$case;sha256=$replacement.sha256;provenance=$provenance}
}
ConvertTo-Json -InputObject $results -Depth 60 | Set-Content -LiteralPath $resultsPath -Encoding UTF8
ConvertTo-Json -InputObject $uiRows -Depth 20 | Set-Content -LiteralPath $uiPath -Encoding UTF8
ConvertTo-Json -InputObject $adoptions -Depth 20 | Set-Content -LiteralPath (Join-Path $Root '_readouts/task-d-adoptions.json') -Encoding UTF8
ConvertTo-Json -InputObject @($adoptions | ForEach-Object { [pscustomobject]@{case=$_.case;sha256=$_.sha256} })
