#requires -Version 7.0
param(
    [string]$OutputRoot = (Split-Path -Parent $PSScriptRoot),
    [string]$WordPlatform = 'Word LTSC Professional Plus 2021 16.0.14334.20848 x64 / Windows 11 22631'
)
$ErrorActionPreference = 'Stop'
$results = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'canvas-images-results.json') | ConvertFrom-Json
$visual = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'canvas-images-visual.json') | ConvertFrom-Json -AsHashtable
$lines = [Collections.Generic.List[string]]::new()
$lines.Add('| 文件 | Word 版本（build）/ 平台 | 制作方式（UI / 脚本名） | 步骤要点 | 看到什么 |')
$lines.Add('| --- | --- | --- | --- | --- |')
$steps = @{
    'canvas-shapes' = 'Shapes.AddCanvas; rectangle with text, theme-filled ellipse, straight line, arrow.'
    'canvas-picture' = 'Shapes.AddCanvas; CanvasItems.AddPicture plus one rectangle.'
    'canvas-resized' = 'Shapes.AddCanvas with four shapes, then ScaleWidth/ScaleHeight 0.5.'
    'canvas-floating' = 'Shapes.AddCanvas with four shapes; square wrapping, horizontal offset 65 pt.'
    'canvas-textbox' = 'Shapes.AddCanvas; CanvasItems.AddTextbox with two paragraphs.'
    'image-inline' = 'InlineShapes.AddPicture, embedded PNG, 144 pt wide.'
    'image-wrap-square' = 'Insert PNG, ConvertToShape, square wrapping.'
    'image-wrap-tight' = 'Insert PNG, ConvertToShape, tight wrapping.'
    'image-behind' = 'Insert PNG, ConvertToShape, behind-text wrapping.'
    'image-front' = 'Insert PNG, ConvertToShape, in-front-of-text wrapping.'
    'image-top-bottom' = 'Insert PNG, ConvertToShape, top/bottom wrapping.'
    'image-cropped' = 'Insert PNG; crop left 18 pt and top 9 pt through PictureFormat.'
    'image-svg' = 'InlineShapes.AddPicture using the included two-color SVG asset.'
    'image-linked' = 'InlineShapes.AddPicture with LinkToFile=true, SaveWithDocument=false.'
    'image-insert-and-link' = 'InlineShapes.AddPicture with LinkToFile=true, SaveWithDocument=true.'
    'image-emf' = 'Required UI copy/paste as enhanced metafile.'
    'image-rotated' = 'Insert PNG, ConvertToShape, rotate 45 degrees, horizontal flip.'
    'image-alt-decorative' = 'Attempt to set the Word InlineShape Decorative property.'
    'image-two-in-run' = 'Insert two pictures into one paragraph without an intervening space.'
}
function Escape-Cell([string]$Text) { return (($Text -replace '\|', '\|') -replace '\r?\n', '<br>') }
$selected = [Collections.Generic.List[object]]::new()
foreach ($group in $results.cases | Group-Object name) {
    $files = @($group.Group | Where-Object { $_.path } | Group-Object path | ForEach-Object { $_.Group | Select-Object -Last 1 })
    if ($files.Count -gt 0) { foreach ($row in $files) { $selected.Add($row) } }
    else { $selected.Add(($group.Group | Select-Object -Last 1)) }
}
foreach ($row in $selected | Sort-Object name, path) {
    $expectedRelative = ($row.name -split '-')[0] + '/' + $row.name + '.docx'
    $uiProduced = -not $row.path -and (Test-Path -LiteralPath (Join-Path $OutputRoot $expectedRelative)) -and $visual.notes.ContainsKey($expectedRelative)
    $relative = if ($row.path) { [IO.Path]::GetRelativePath($OutputRoot, $row.path).Replace('\', '/') } elseif ($uiProduced) { $expectedRelative } else { $expectedRelative + ' (not produced)' }
    $note = $visual.notes[$relative]
    $observation = if ($null -ne $note -and $note.reviewed) { $note.observation } elseif ($row.path) { 'NOT VISUALLY REVIEWED. Saved Word DOCX and PDF exist; object-model/ZIP checks alone do not establish the visible result.' } else { 'NOT PRODUCED. ' + $row.error }
    if ($null -eq $note -and $row.path -and $row.package -and $row.package.warnings.Count -gt 0) { $observation += ' Package notes: ' + ($row.package.warnings -join ' ') }
    $method = if ($note.method) { $note.method } else { '_scripts/create-canvas-images.ps1; same-instance Word PDF; copied-ZIP self-check' }
    $caseSteps = if ($note.steps) { $note.steps } else { $steps[$row.name] }
    $cells = @($relative, $WordPlatform, $method, $caseSteps, $observation) | ForEach-Object { Escape-Cell $_ }
    $lines.Add('| ' + ($cells -join ' | ') + ' |')
}
$destination = Join-Path $PSScriptRoot 'observed-canvas-image.md'
$lines | Set-Content -LiteralPath $destination -Encoding utf8
[pscustomobject]@{ Path = $destination; Rows = $selected.Count; ReviewedNotes = $visual.notes.Count }
