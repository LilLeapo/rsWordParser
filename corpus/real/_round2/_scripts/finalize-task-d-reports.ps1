# Definitions only. Run Complete-TaskDReports after the root agent authorizes it.
# UI input: [{case, observation, screenshots:[]}]. PDF input: {rows:[...]}.
# This report finalizer never starts Word or changes DOCX/PDF files.

function Complete-TaskDReports {
    [CmdletBinding()]
    param(
        [string] $OutputDirectory = 'C:/word/real-word-round2-20260907',
        [string] $ResultsPath,
        [string] $UiPath,
        [string] $PdfReviewPath,
        [int] $ExpectedCount = 17
    )
    $ErrorActionPreference = 'Stop'
    if (-not $ResultsPath) { $ResultsPath = Join-Path $OutputDirectory '_scripts/task-d-results.json' }
    if (-not $UiPath) { $UiPath = Join-Path $OutputDirectory '_scripts/task-d-ui.json' }
    if (-not $PdfReviewPath) { $PdfReviewPath = Join-Path $OutputDirectory '_readouts/task-d-pdf-review.json' }
    $exactWordVersion = '16.0.14334.20848'
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    $results = @((Get-Content -LiteralPath $ResultsPath -Raw -Encoding UTF8 | ConvertFrom-Json) | ForEach-Object { $_ })
    if ($results.Count -ne $ExpectedCount) { throw "Expected $ExpectedCount Task D records, found $($results.Count)." }
    $uiRows = @()
    if (Test-Path -LiteralPath $UiPath) { $uiRows = @((Get-Content -LiteralPath $UiPath -Raw -Encoding UTF8 | ConvertFrom-Json) | ForEach-Object { $_ }) }
    $pdfRows = @()
    if (Test-Path -LiteralPath $PdfReviewPath) {
        $pdfDocument = Get-Content -LiteralPath $PdfReviewPath -Raw -Encoding UTF8 | ConvertFrom-Json
        if ($null -eq $pdfDocument.PSObject.Properties['rows']) { throw 'PDF review file must contain a rows array.' }
        $pdfRows = @($pdfDocument.rows)
    }

    function Set-ReportProperty($Target, [string] $Name, $Value) {
        $Target | Add-Member -MemberType NoteProperty -Name $Name -Value $Value -Force
    }

    function Resolve-EvidencePath([string] $Path) {
        if ([string]::IsNullOrWhiteSpace($Path)) { return $null }
        if ([IO.Path]::IsPathRooted($Path)) { return [IO.Path]::GetFullPath($Path) }
        return [IO.Path]::GetFullPath((Join-Path $OutputDirectory $Path))
    }

    function Test-EvidenceFile([string] $Path) {
        $resolved = Resolve-EvidencePath $Path
        return ($null -ne $resolved -and (Test-Path -LiteralPath $resolved -PathType Leaf))
    }

    function ConvertTo-ReportCell($Value) {
        if ($null -eq $Value -or [string]::IsNullOrWhiteSpace([string]$Value)) { return '-' }
        return (([string]$Value).Replace('&','&amp;').Replace('<','&lt;').Replace('>','&gt;').Replace('|','&#124;').Replace('`','&#96;') -replace '\r\n|\r|\n','<br>')
    }

    $byCase = @{}
    $protectedBefore = @{}
    $protectedFields = @('operations','selfcheck','sha256','error','trace','observation','wordVersion','path','text','pages','paragraphs','revisions','comments','compatibility')
    foreach ($result in $results) {
        if (-not $result.case -or $byCase.ContainsKey([string]$result.case)) { throw 'Task D case names must be present and unique.' }
        if ($result.case -notmatch '^[a-z0-9]+/[a-z0-9-]+$') { throw "Unexpected case path: $($result.case)" }
        $byCase[[string]$result.case] = $result
        $protected = [ordered]@{}
        foreach ($field in $protectedFields) { $protected[$field] = $result.$field }
        $protectedBefore[[string]$result.case] = ConvertTo-Json -InputObject $protected -Depth 60 -Compress
    }
    $uiByCase = @{}
    foreach ($ui in $uiRows) {
        if (-not $byCase.ContainsKey([string]$ui.case)) { throw "UI observation has no Task D case: $($ui.case)" }
        if ($uiByCase.ContainsKey([string]$ui.case)) { throw "Duplicate UI case: $($ui.case)" }
        $uiByCase[[string]$ui.case] = $ui
    }
    $pdfByCase = @{}
    foreach ($pdf in $pdfRows) {
        if (-not $byCase.ContainsKey([string]$pdf.case)) { throw "PDF review has no Task D case: $($pdf.case)" }
        if ($pdfByCase.ContainsKey([string]$pdf.case)) { throw "Duplicate PDF review case: $($pdf.case)" }
        $pdfByCase[[string]$pdf.case] = $pdf
    }

    foreach ($result in $results) {
        if ($null -eq $result.PSObject.Properties['statusBeforeVisualReview']) { Set-ReportProperty $result 'statusBeforeVisualReview' $result.status }
        Set-ReportProperty $result 'wordExactVersion' $exactWordVersion
        $sourcePath = if ($result.path) { Resolve-EvidencePath $result.path } else { Join-Path $OutputDirectory ($result.case + '.docx') }
        $sourceExists = Test-Path -LiteralPath $sourcePath -PathType Leaf
        $actualHash = $null
        if ($sourceExists) { $actualHash = (Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256).Hash }
        $authoredHashMatches = $null
        if ($actualHash -and $result.sha256) { $authoredHashMatches = $actualHash -eq $result.sha256 }
        Set-ReportProperty $result 'reportIntegrity' ([pscustomobject][ordered]@{ sourceExists = $sourceExists; actualSha256 = $actualHash; matchesAuthoredSha256 = $authoredHashMatches })

        $ui = $null
        if ($uiByCase.ContainsKey([string]$result.case)) { $ui = $uiByCase[[string]$result.case] }
        $uiMissing = New-Object System.Collections.Generic.List[string]
        $uiScreenshots = @()
        $uiObservationPresent = $false
        if ($null -ne $ui) {
            $uiScreenshots = @($ui.screenshots | Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_) })
            $uiObservationPresent = -not [string]::IsNullOrWhiteSpace([string]$ui.observation)
            foreach ($screenshot in $uiScreenshots) { if (-not (Test-EvidenceFile $screenshot)) { $uiMissing.Add([string]$screenshot) } }
        }
        $uiChecked = $null -ne $ui -and $uiObservationPresent -and $uiScreenshots.Count -gt 0 -and $uiMissing.Count -eq 0
        Set-ReportProperty $result 'uiReview' ([pscustomobject][ordered]@{
            checked = $uiChecked
            scope = 'Direct Word UI first-page view; wider coverage only when explicitly stated in the observation.'
            observation = $(if ($null -ne $ui) { $ui.observation } else { $null })
            screenshots = $uiScreenshots
            missingEvidence = $uiMissing.ToArray()
            record = $ui
            reviewBarrierIsNotInspectionEvidence = $true
        })
        Set-ReportProperty $result 'visualChecked' $uiChecked
        Set-ReportProperty $result 'visualObservation' $(if ($uiChecked) { $ui.observation } else { $null })

        $pdf = $null
        if ($pdfByCase.ContainsKey([string]$result.case)) { $pdf = $pdfByCase[[string]$result.case] }
        $pdfMissing = New-Object System.Collections.Generic.List[string]
        $pdfSourceMatches = $false
        $pdfHashMatches = $false
        $pdfExists = $false
        $pdfReviewed = $false
        $pdfAllPages = $false
        $independentStructureChecked = $false
        $independentStructurePassed = $null
        if ($null -ne $pdf) {
            $pdfSourceMatches = $actualHash -and $pdf.sha256 -and ($actualHash -eq $pdf.sha256)
            $independentStructureChecked = $pdfSourceMatches -and $null -ne $pdf.structure -and @($pdf.structure.checks).Count -gt 0
            if ($independentStructureChecked) { $independentStructurePassed = $pdf.structure.passed }
            $pdfExists = Test-EvidenceFile $pdf.pdf
            if ($pdfExists -and $pdf.pdfSha256) { $pdfHashMatches = (Get-FileHash -LiteralPath (Resolve-EvidencePath $pdf.pdf) -Algorithm SHA256).Hash -eq $pdf.pdfSha256 }
            if (-not $pdfExists) { $pdfMissing.Add([string]$pdf.pdf) }
            $pageNumbers = @($pdf.visualReview.pageNumbers | Sort-Object -Unique)
            if ($pdf.pageCount -gt 0 -and $pageNumbers.Count -eq [int]$pdf.pageCount) {
                $pdfAllPages = $true
                foreach ($page in 1..([int]$pdf.pageCount)) { if ($pageNumbers -notcontains $page) { $pdfAllPages = $false } }
            }
            $pdfEvidence = @($pdf.visualReview.evidence | Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_) })
            foreach ($evidence in $pdfEvidence) { if (-not (Test-EvidenceFile $evidence)) { $pdfMissing.Add([string]$evidence) } }
            $pdfReviewed = $pdf.visualReviewed -eq $true -and $pdfSourceMatches -and $pdfHashMatches -and $pdfAllPages -and $pdfEvidence.Count -gt 0 -and $pdfMissing.Count -eq 0 -and -not [string]::IsNullOrWhiteSpace([string]$pdf.visualReview.observation)
        }
        Set-ReportProperty $result 'pdfReview' $pdf
        Set-ReportProperty $result 'pdfReviewValidation' ([pscustomobject][ordered]@{
            reviewed = $pdfReviewed
            sourceHashMatches = [bool]$pdfSourceMatches
            pdfExists = $pdfExists
            pdfHashMatches = $pdfHashMatches
            allPagesExplicitlyReviewed = $pdfAllPages
            missingEvidence = $pdfMissing.ToArray()
            independentStructureChecked = [bool]$independentStructureChecked
            independentStructurePassed = $independentStructurePassed
        })
        $statusParts = New-Object System.Collections.Generic.List[string]
        if (-not $sourceExists) { $statusParts.Add('DOCX missing') }
        elseif ($authoredHashMatches -eq $true) { $statusParts.Add('saved DOCX; authored SHA256 retained') }
        elseif ($authoredHashMatches -eq $false) { $statusParts.Add('saved DOCX; differs from recorded authored SHA256') }
        else { $statusParts.Add('saved DOCX; authored SHA256 unavailable') }
        if ($null -eq $result.selfcheck) { $statusParts.Add('original package selfcheck unavailable') }
        elseif ($result.selfcheck.passed -eq $true) { $statusParts.Add('original package selfcheck passed') }
        else { $statusParts.Add('original package selfcheck incomplete') }
        if (@($result.operations | Where-Object { $_.status -eq 'error' }).Count -gt 0) { $statusParts.Add('recorded operation error(s)') }
        if ($result.error) { $statusParts.Add('recorded creation/check error retained') }
        if ($independentStructureChecked) {
            if ($independentStructurePassed -eq $true) { $statusParts.Add('independent structure review passed') }
            else { $statusParts.Add('independent structure review incomplete') }
        } else { $statusParts.Add('independent structure review unavailable or stale') }
        if ($uiChecked) { $statusParts.Add('direct Word UI inspected within recorded scope') }
        else { $statusParts.Add('direct Word UI not verified') }
        if ($pdfReviewed) {
            $statusParts.Add('independent PDF pages inspected')
            if (@($pdf.visualReview.issues).Count -gt 0) { $statusParts.Add('PDF review issue(s) recorded') }
        } else { $statusParts.Add('independent PDF visual review not verified') }
        $result.status = $statusParts -join '; '
    }

    $domainReports = @{}
    foreach ($domain in @($results | ForEach-Object { ($_.case -split '/')[0] } | Sort-Object -Unique)) {
        $lines = New-Object System.Collections.Generic.List[string]
        $lines.Add('# Task D Observations')
        $lines.Add('')
        $lines.Add("Microsoft Word $exactWordVersion on Windows. Authoring actions, original package selfchecks and SHA256 records are retained in _scripts/task-d-results.json. Direct UI observations cover the recorded view, normally the first page. Independent PDF review is a separate inspection of actual Word-exported pages, validated against document/PDF hashes and existing image evidence. A ReviewBarrier checkpoint alone is not evidence of inspection.")
        $lines.Add('')
        $lines.Add('| File | Word Version | Authoring Record | Direct Word UI | Independent PDF Review | Checks and Limitations |')
        $lines.Add('| --- | --- | --- | --- | --- | --- |')
        foreach ($result in $results | Where-Object { $_.case -like "$domain/*" }) {
            $uiText = 'Not verified.'
            if ($result.uiReview.checked) { $uiText = $result.uiReview.scope + ' ' + $result.uiReview.observation + ' Screenshots: ' + ($result.uiReview.screenshots -join ', ') }
            elseif ($null -ne $result.uiReview.record) { $uiText = 'Observation record exists but evidence is incomplete. ' + $result.uiReview.observation + ' Missing: ' + ($result.uiReview.missingEvidence -join ', ') }
            $pdfText = 'Not verified.'
            if ($result.pdfReviewValidation.reviewed) {
                $pdfText = 'Independently viewed PDF pages ' + ($result.pdfReview.visualReview.pageNumbers -join ', ') + ': ' + $result.pdfReview.visualReview.observation + ' Evidence: ' + ($result.pdfReview.visualReview.evidence -join ', ')
                if (@($result.pdfReview.visualReview.issues).Count -gt 0) { $pdfText += ' Issues: ' + (ConvertTo-Json -InputObject $result.pdfReview.visualReview.issues -Compress -Depth 10) }
            } elseif ($null -ne $result.pdfReview) { $pdfText = 'PDF extraction/rendering record exists; complete visual review and matching evidence are not verified.' }
            $limitations = $result.status
            $originalFailures = @($result.selfcheck.checks | Where-Object { $_.passed -eq $false } | ForEach-Object { $_.name })
            if ($originalFailures.Count -gt 0) { $limitations += '; original unmet checks: ' + ($originalFailures -join ', ') }
            $independentFailures = @($result.pdfReview.structure.checks | Where-Object { $_.passed -eq $false } | ForEach-Object { $_.requirement })
            if ($independentFailures.Count -gt 0) { $limitations += '; independent unmet checks: ' + ($independentFailures -join ', ') }
            foreach ($operation in @($result.operations | Where-Object { $_.status -eq 'error' })) { $limitations += '; operation ' + $operation.operation + ': ' + $operation.error }
            if ($result.error) { $limitations += '; original error: ' + $result.error }
            $lines.Add('| ' + (ConvertTo-ReportCell ($result.case + '.docx')) + ' | ' + $exactWordVersion + ' | ' + (ConvertTo-ReportCell $result.observation) + ' | ' + (ConvertTo-ReportCell $uiText) + ' | ' + (ConvertTo-ReportCell $pdfText) + ' | ' + (ConvertTo-ReportCell $limitations) + ' |')
        }
        $domainReports[$domain] = $lines
    }

    foreach ($result in $results) {
        $protected = [ordered]@{}
        foreach ($field in $protectedFields) { $protected[$field] = $result.$field }
        if ((ConvertTo-Json -InputObject $protected -Depth 60 -Compress) -ne $protectedBefore[[string]$result.case]) { throw "Protected authoring data changed for $($result.case)." }
    }
    $summary = [ordered]@{
        cases = $results.Count
        exactWordVersion = $exactWordVersion
        directUiVerified = @($results | Where-Object { $_.uiReview.checked -eq $true }).Count
        independentPdfVisualVerified = @($results | Where-Object { $_.pdfReviewValidation.reviewed -eq $true }).Count
        originalSelfcheckPassed = @($results | Where-Object { $_.selfcheck.passed -eq $true }).Count
        originalSelfcheckUnavailable = @($results | Where-Object { $null -eq $_.selfcheck }).Count
        independentStructurePassed = @($results | Where-Object { $_.pdfReviewValidation.independentStructurePassed -eq $true }).Count
        authoredHashRetained = @($results | Where-Object { $_.reportIntegrity.matchesAuthoredSha256 -eq $true }).Count
        pendingUi = @($results | Where-Object { $_.uiReview.checked -ne $true } | ForEach-Object { $_.case })
        pendingPdfVisual = @($results | Where-Object { $_.pdfReviewValidation.reviewed -ne $true } | ForEach-Object { $_.case })
        originalErrorsRetained = @($results | Where-Object { $_.error } | ForEach-Object { [ordered]@{case=$_.case;error=$_.error} })
        note = 'Review coverage is separate from requirement conformance. No blanket all-D-passed conclusion is made.'
    }
    $snapshot = Join-Path $OutputDirectory '_scripts/task-d-results-before-visual-review.json'
    if (-not (Test-Path -LiteralPath $snapshot)) { [IO.File]::Copy($ResultsPath, $snapshot, $false) }
    [IO.File]::WriteAllText($ResultsPath, (ConvertTo-Json -InputObject $results -Depth 60), $utf8)
    foreach ($domain in $domainReports.Keys) {
        $directory = Join-Path $OutputDirectory $domain
        [void][IO.Directory]::CreateDirectory($directory)
        [IO.File]::WriteAllLines((Join-Path $directory 'OBSERVED.md'), $domainReports[$domain], $utf8)
    }
    [IO.File]::WriteAllText((Join-Path $OutputDirectory '_scripts/task-d-final-summary.json'), (ConvertTo-Json -InputObject $summary -Depth 20), $utf8)
    return [pscustomobject]$summary
}
