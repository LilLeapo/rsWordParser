# UI helper functions. The operator must inspect the actual Word window and
# supply the recovery/visual conclusions; these functions never infer them.

function Open-Edited3UiCase {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] [object] $Word,
        [Parameter(Mandatory)] [string] $File,
        [string] $InputDirectory = 'C:/word/round3-work-20260907/real-word-round3-inputs/edited'
    )
    if ($File -ne [IO.Path]::GetFileName($File)) { throw 'File must be a basename.' }
    $path = [IO.Path]::GetFullPath((Join-Path $InputDirectory $File))
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing input: $path" }
    $Word.Visible = $true
    $Word.DisplayAlerts = -1
    $missing = [Type]::Missing
    $document = $Word.Documents.Open($path, $false, $true, $false, $missing, $missing, $false, $missing, $missing, $missing, $missing, $true)
    if ($null -eq $document) { throw 'Word returned no document.' }
    $document.Activate()
    $document.ActiveWindow.View.Type = 3
    $document.ActiveWindow.View.Zoom.Percentage = 95
    $document.ActiveWindow.View.ShowAll = $false
    $document.ActiveWindow.View.ShowHiddenText = $false
    [void]$Word.Selection.HomeKey(6)
    return [ordered]@{ file = [string]$document.Name; path = [string]$document.FullName; compat = [int]$document.CompatibilityMode; method = 'Actual Word UI open, DisplayAlerts=-1, ReadOnly=true, Visible=true; inspect UI to determine prompts.' }
}

function Read-Edited3ActiveCase {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] [object] $Word,
        [Parameter(Mandatory)] [string] $File,
        [string] $InputDirectory = 'C:/word/round3-work-20260907/real-word-round3-inputs/edited',
        [string] $OutputDirectory = 'C:/word/real-word-round3-20260907',
        [switch] $ActivateData,
        [switch] $FocusFeature,
        [string] $ReadoutTag = 'initial'
    )
    $ErrorActionPreference = 'Stop'
    . (Join-Path $PSScriptRoot 'edited3-common.ps1')
    function Write-BatchStage([string] $Stage, [string] $Detail = '') { }
    $document = $Word.ActiveDocument
    if ($null -eq $document) { throw 'No active Word document.' }
    $expected = [IO.Path]::GetFullPath((Join-Path $InputDirectory $File))
    if (-not ([string]$document.FullName).Equals($expected, [StringComparison]::OrdinalIgnoreCase)) { throw "Active document does not match the requested original: $($document.FullName)" }
    $item = @((Get-Content -LiteralPath (Join-Path $InputDirectory 'manifest.json') -Raw -Encoding UTF8 | ConvertFrom-Json) | Where-Object { $_.file -eq $File -and $_.status -eq 'generated' })
    if ($item.Count -ne 1) { throw "Expected exactly one generated manifest entry: $File" }
    $item = $item[0]
    $reading = [ordered]@{ file = $File; base = $item.base; op = $item.op; expect = $item.expect; timestamp = [DateTime]::Now.ToString('o'); method = 'Read from actual active Word window; does not constitute a visual observation.'; active_path = [string]$document.FullName; metrics = (Get-E3DocumentMetrics $document); charts = @(); shape_geometry = @(); text = [string]$document.Content.Text; comments = @(); headers = @(); tables = @(); errors = (New-Object Collections.ArrayList) }
    if ($item.op -in @('chartdata', 'newchart')) { $reading.charts = Get-E3DocumentCharts $document ([bool]$ActivateData) }
    if ($item.op -in @('newimage', 'ink', 'replaceimage')) { $reading.shape_geometry = Get-E3ShapeGeometry $document }
    if ($item.op -eq 'comment') {
        try {
            $reading.comments = @($document.Comments | ForEach-Object { [ordered]@{ author = [string]$_.Author; text = [string]$_.Range.Text; scope = [string]$_.Scope.Text } })
            if ($FocusFeature -and $document.Comments.Count -gt 0) {
                $document.ActiveWindow.View.ShowComments = $true
                $document.ActiveWindow.View.ShowRevisionsAndComments = $true
                $document.Comments.Item($document.Comments.Count).Scope.Select()
            }
        } catch { Add-E3ReadError $reading 'ui.comments' $_ }
    }
    if ($item.op -eq 'header') {
        try {
            $reading.headers = @($document.Sections | ForEach-Object { [ordered]@{ section = [int]$_.Index; primary = [string]$_.Headers.Item(1).Range.Text; first = [string]$_.Headers.Item(2).Range.Text; even = [string]$_.Headers.Item(3).Range.Text } })
            if ($FocusFeature) { $document.Sections.Last.Headers.Item(1).Range.Select() }
        } catch { Add-E3ReadError $reading 'ui.headers' $_ }
    }
    if ($item.op -in @('insertrow', 'mergecells')) {
        try {
            $reading.tables = @($document.Tables | ForEach-Object { [ordered]@{ text = [string]$_.Range.Text; cells = [int]$_.Range.Cells.Count } })
            if ($FocusFeature -and $document.Tables.Count -gt 0) { $document.Tables.Item(1).Range.Select() }
        } catch { Add-E3ReadError $reading 'ui.tables' $_ }
    }
    if ($ReadoutTag -notmatch '^[a-zA-Z0-9_-]+$') { throw 'ReadoutTag must contain only letters, digits, underscore and hyphen.' }
    $readoutRoot = Join-Path $OutputDirectory '_readouts'
    [void][IO.Directory]::CreateDirectory($readoutRoot)
    $path = Join-Path $readoutRoot (([IO.Path]::GetFileNameWithoutExtension($File)) + '-ui-' + $ReadoutTag + '.json')
    if (Test-Path -LiteralPath $path) { throw "Refusing to overwrite a readout: $path" }
    [IO.File]::WriteAllText($path, ($reading | ConvertTo-Json -Depth 40), (New-Object Text.UTF8Encoding($false)))
    return $reading
}

function Save-Edited3UiArtifacts {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)] [object] $Word,
        [Parameter(Mandatory)] [string] $File,
        [string] $InputDirectory = 'C:/word/round3-work-20260907/real-word-round3-inputs/edited',
        [string] $OutputDirectory = 'C:/word/real-word-round3-20260907',
        [Parameter(Mandatory)] [string] $StateDescription
    )
    $ErrorActionPreference = 'Stop'
    $document = $Word.ActiveDocument
    $expected = [IO.Path]::GetFullPath((Join-Path $InputDirectory $File))
    if (-not ([string]$document.FullName).Equals($expected, [StringComparison]::OrdinalIgnoreCase)) { throw "Active document does not match the requested original: $($document.FullName)" }
    $stem = [IO.Path]::GetFileNameWithoutExtension($File)
    $resaved = Join-Path $OutputDirectory ('_resaved/' + $stem + '-resaved-by-word.docx')
    $pdf = Join-Path $OutputDirectory ('_previews/edited3/' + $stem + '.pdf')
    foreach ($path in @($resaved, $pdf)) {
        if (Test-Path -LiteralPath $path) { throw "Refusing to overwrite an artifact: $path" }
        [void][IO.Directory]::CreateDirectory((Split-Path -Parent $path))
    }
    function Get-UiInputHash([string] $Path) {
        $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
        $hasher = [Security.Cryptography.SHA256]::Create()
        try { return [BitConverter]::ToString($hasher.ComputeHash($stream)).Replace('-', '') }
        finally { $hasher.Dispose(); $stream.Dispose() }
    }
    $inputHash = Get-UiInputHash $expected
    $document.SaveAs2([string]$resaved, 16)
    $document.ExportAsFixedFormat([string]$pdf, 17)
    if ((Get-UiInputHash $expected) -ne $inputHash) { throw 'Original input changed unexpectedly.' }
    return [ordered]@{ file = $File; resaved = $resaved; pdf = $pdf; state_description = $StateDescription; method = 'Native Word SaveAs2, then PDF export; document remains open. A PDF export can update in-memory fields after the saved DOCX.' }
}
