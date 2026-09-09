#requires -Version 7.0
[CmdletBinding()]
param(
    [string]$OutputRoot = (Split-Path -Parent $PSScriptRoot),
    [string[]]$Only = @()
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.IO.Compression.FileSystem
$OutputRoot = [IO.Path]::GetFullPath($OutputRoot)
$script:Word = $null
$script:Document = $null
$script:Details = [ordered]@{}
$rows = [Collections.Generic.List[object]]::new()
$logPath = Join-Path $OutputRoot '_scripts/canvas-images-results.json'
foreach ($folder in @('canvas', 'image', '_assets', '_previews/canvas', '_previews/image', '_scripts', '_checks/canvas', '_checks/image')) {
    [void][IO.Directory]::CreateDirectory((Join-Path $OutputRoot $folder))
}
if (Test-Path -LiteralPath $logPath) {
    $previous = Get-Content -Raw -LiteralPath $logPath | ConvertFrom-Json
    foreach ($row in $previous.cases) { $rows.Add($row) }
}
$asset = Join-Path $OutputRoot '_assets/tiny.png'
if (-not (Test-Path -LiteralPath $asset)) {
    $bitmap = [Drawing.Bitmap]::new(64, 32)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.Clear([Drawing.Color]::FromArgb(36, 146, 126))
        $graphics.FillRectangle([Drawing.Brushes]::Gold, 32, 0, 32, 32)
        $bitmap.Save($asset, [Drawing.Imaging.ImageFormat]::Png)
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
}
$svgAsset = Join-Path $PSScriptRoot 'canvas-images.svg'
$before = 'before ' + [char]0x524d + [char]0x6587
$after = 'after ' + [char]0x540e + [char]0x6587
$canvasText = [string][char]0x753b + [char]0x5e03

function Get-UnusedPath([string]$Relative) {
    $path = Join-Path $OutputRoot $Relative
    $folder = [IO.Path]::GetDirectoryName($path)
    $stem = [IO.Path]::GetFileNameWithoutExtension($path)
    $extension = [IO.Path]::GetExtension($path)
    for ($suffix = 2; (Test-Path -LiteralPath $path); $suffix++) {
        $path = Join-Path $folder ($stem + '-' + $suffix + $extension)
    }
    return $path
}

function Write-Results {
    [ordered]@{
        generatedAt = [DateTimeOffset]::Now.ToString('o')
        method = 'A fresh visible Windows desktop Word COM instance for each case. One SaveAs2 per document, then PDF export without another DOCX save. Package inspection uses a copied ZIP and never modifies DOCX.'
        evidence = 'Object-model and package facts only. PDF/Word visual observations are recorded separately by the reviewing agent.'
        cases = @($rows.ToArray())
    } | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $logPath -Encoding utf8
}

function Get-FeatureRange {
    $range = $script:Document.Paragraphs.Item($script:FeatureParagraph).Range.Duplicate
    $range.Collapse(1)
    return ,$range
}

function Add-InlinePicture([bool]$Link = $false, [bool]$Embed = $true, [string]$Path = $asset) {
    $picture = $script:Document.InlineShapes.AddPicture($Path, $Link, $Embed, (Get-FeatureRange))
    $picture.LockAspectRatio = -1
    $picture.Width = 144
    $picture.AlternativeText = 'Green and yellow color blocks, 64 by 32 pixels'
    return ,$picture
}

function Add-FloatingPicture([int]$Wrap) {
    $inline = Add-InlinePicture
    $shape = $inline.ConvertToShape()
    $shape.WrapFormat.Type = $Wrap
    $shape.RelativeHorizontalPosition = 0
    $shape.RelativeVerticalPosition = 2
    $shape.Left = 180
    $shape.Top = 0
    $script:Details['wrapType'] = [int]$shape.WrapFormat.Type
    $script:Details['positionPoints'] = @([single]$shape.Left, [single]$shape.Top)
    return ,$shape
}

function Add-Canvas([string]$Variant) {
    $anchorRange = Get-FeatureRange
    $anchorRange.Text = ' '
    $anchorRange.Collapse(1)
    $anchorRange.Select()
    $canvas = $script:Document.Shapes.AddCanvas(0, 0, 360, 200, $anchorRange)
    $canvas.Name = $Variant
    $canvas.RelativeHorizontalPosition = 0
    $canvas.RelativeVerticalPosition = 2
    $canvas.Left = 0
    $canvas.Top = 0
    $canvas.LockAnchor = -1
    $canvas.WrapFormat.Type = 4
    $canvas.Line.Visible = 0
    if ($Variant -eq 'canvas-picture') {
        $picture = $canvas.CanvasItems.AddPicture($asset, $false, $true, 15, 30, 128, 64)
        $rectangle = $canvas.CanvasItems.AddShape(1, 200, 35, 130, 70)
        $rectangle.Fill.ForeColor.RGB = 0xCC6633
        $rectangle.TextFrame.TextRange.Text = $canvasText
    } elseif ($Variant -eq 'canvas-textbox') {
        $textbox = $canvas.CanvasItems.AddTextbox(1, 25, 25, 300, 125)
        $textbox.TextFrame.TextRange.Text = 'Canvas text paragraph one' + "`r" + 'Canvas text paragraph two'
        $textbox.Fill.ForeColor.RGB = 0xE8F4FF
    } else {
        $rectangle = $canvas.CanvasItems.AddShape(1, 20, 20, 135, 65)
        $rectangle.Fill.ForeColor.RGB = 0xCC6633
        $rectangle.TextFrame.TextRange.Text = $canvasText
        $oval = $canvas.CanvasItems.AddShape(9, 200, 20, 120, 65)
        $oval.Fill.ForeColor.ObjectThemeColor = 6
        $line = $canvas.CanvasItems.AddLine(25, 125, 150, 125)
        $line.Line.Weight = 2
        $arrow = $canvas.CanvasItems.AddLine(205, 125, 325, 125)
        $arrow.Line.Weight = 2
        $arrow.Line.EndArrowheadStyle = 3
    }
    if ($Variant -eq 'canvas-resized') {
        $script:Details['sizeBeforePoints'] = @([single]$canvas.Width, [single]$canvas.Height)
        $canvas.LockAspectRatio = 0
        $canvas.ScaleWidth(0.5, 0, 0)
        $canvas.ScaleHeight(0.5, 0, 0)
        if ([Math]::Abs($canvas.Width - 180) -gt 0.1 -or [Math]::Abs($canvas.Height - 100) -gt 0.1) { throw 'Canvas resize did not reach the required 180 x 100 pt.' }
    }
    if ($Variant -eq 'canvas-floating') {
        $canvas.WrapFormat.Type = 0
        $canvas.Left = 65
        $canvas.Top = 0
    }
    $items = @()
    for ($i = 1; $i -le $canvas.CanvasItems.Count; $i++) {
        $item = $canvas.CanvasItems.Item($i)
        $text = ''
        try { $text = [string]$item.TextFrame.TextRange.Text } catch { }
        $items += [ordered]@{ index = $i; type = [int]$item.Type; text = $text; sizePoints = @([single]$item.Width, [single]$item.Height); positionPoints = @([single]$item.Left, [single]$item.Top) }
    }
    $script:Details['canvasItems'] = $items
    $script:Details['canvasSizePoints'] = @([single]$canvas.Width, [single]$canvas.Height)
    $script:Details['floating'] = $true
    $script:Details['wrapType'] = [int]$canvas.WrapFormat.Type
    $script:Details['anchorRangeStart'] = [int]$canvas.Anchor.Start
    $script:Details['requestedAnchorRangeStart'] = [int]$anchorRange.Start
}

function Inspect-Copy([string]$DocumentPath, [string]$Domain, [string]$CaseName) {
    $copy = Join-Path $OutputRoot ('_checks/' + $Domain + '/' + [IO.Path]::GetFileNameWithoutExtension($DocumentPath) + '.zip')
    Copy-Item -LiteralPath $DocumentPath -Destination $copy -ErrorAction Stop
    $archive = [IO.Compression.ZipFile]::OpenRead($copy)
    try {
        $stream = $archive.GetEntry('word/document.xml').Open()
        try { $xml = [xml]::new(); $xml.Load($stream) } finally { $stream.Dispose() }
        $names = @($archive.Entries.FullName)
        $tags = @('lockedCanvas', 'wpc', 'wsp', 'sp', 'cxnSp', 'pic', 'inline', 'anchor', 'wrapSquare', 'wrapTight', 'wrapPolygon', 'wrapTopAndBottom', 'wrapNone', 'srcRect', 'svgBlip', 'decorative', 'chOff', 'chExt')
        $counts = [ordered]@{}
        foreach ($tag in $tags) { $counts[$tag] = $xml.SelectNodes('//*[local-name()="' + $tag + '"]').Count }
        $blips = @($xml.SelectNodes('//*[local-name()="blip"]') | ForEach-Object { [ordered]@{ embed = $_.GetAttribute('embed', 'http://schemas.openxmlformats.org/officeDocument/2006/relationships'); link = $_.GetAttribute('link', 'http://schemas.openxmlformats.org/officeDocument/2006/relationships') } })
        $transforms = @($xml.SelectNodes('//*[local-name()="xfrm"]') | ForEach-Object { [ordered]@{ rot = $_.GetAttribute('rot'); flipH = $_.GetAttribute('flipH'); xml = $_.OuterXml } })
        $markers = @($xml.SelectNodes('//*[local-name()="body"]/*[local-name()="p"]') | ForEach-Object { ($_.SelectNodes('./*[local-name()="r"]/*[local-name()="t"]') | ForEach-Object InnerText) -join '' })
        $warnings = @()
        if ($Domain -eq 'canvas' -and $counts.lockedCanvas -eq 0) { $warnings += 'Word emitted a modern wpc canvas instead of the lc:lockedCanvas wrapper requested by docs/07.' }
        if ($Domain -eq 'canvas' -and ($counts.lockedCanvas + $counts.wpc) -eq 0) { $warnings += 'No recognized canvas wrapper was saved.' }
        if ($markers -notcontains $before -or $markers -notcontains $after) { $warnings += 'Standalone before/after marker missing.' }
        $expectedWrappers = @{ 'image-inline' = 'inline'; 'image-wrap-square' = 'wrapSquare'; 'image-wrap-tight' = 'wrapTight'; 'image-behind' = 'wrapNone'; 'image-front' = 'wrapNone'; 'image-top-bottom' = 'wrapTopAndBottom'; 'image-cropped' = 'srcRect'; 'image-svg' = 'svgBlip'; 'image-alt-decorative' = 'decorative' }
        if ($expectedWrappers.ContainsKey($CaseName) -and $counts[$expectedWrappers[$CaseName]] -eq 0) { $warnings += ('Requested feature tag absent: ' + $expectedWrappers[$CaseName]) }
        if ($CaseName -eq 'image-wrap-tight' -and $counts.wrapPolygon -eq 0) { $warnings += 'Tight image has no wrapPolygon.' }
        if ($CaseName -in @('image-linked', 'image-insert-and-link') -and @($blips | Where-Object { $_.link }).Count -eq 0) { $warnings += 'Requested linked image has no blip r:link.' }
        if ($CaseName -eq 'image-insert-and-link' -and @($blips | Where-Object { $_.link -and $_.embed }).Count -eq 0) { $warnings += 'Insert-and-link image does not have both r:embed and r:link on the same blip.' }
        if ($CaseName -eq 'image-rotated' -and @($transforms | Where-Object { $_.rot -eq '2700000' -and $_.flipH -eq '1' }).Count -eq 0) { $warnings += 'Requested rotation/flip transform is not rot=2700000 and flipH=1.' }
        if ($CaseName -eq 'image-two-in-run' -and $counts.pic -ne 2) { $warnings += 'Expected exactly two picture elements.' }
        return [ordered]@{ zipCopy = $copy; sourceHash = (Get-FileHash -LiteralPath $DocumentPath -Algorithm SHA256).Hash; counts = $counts; blips = $blips; transforms = $transforms; media = @($names | Where-Object { $_ -match '^word/media/.+' }); bodyParagraphTexts = $markers; warnings = $warnings }
    } finally { $archive.Dispose() }
}

$cases = @(
    'canvas-shapes', 'canvas-picture', 'canvas-resized', 'canvas-floating', 'canvas-textbox',
    'image-inline', 'image-wrap-square', 'image-wrap-tight', 'image-behind', 'image-front', 'image-top-bottom',
    'image-cropped', 'image-svg', 'image-linked', 'image-insert-and-link', 'image-emf', 'image-rotated', 'image-alt-decorative', 'image-two-in-run'
)
if ($Only.Count -gt 0) {
    $unknown = @($Only | Where-Object { $_ -notin $cases })
    if ($unknown.Count -gt 0) { throw ('Unknown case: ' + ($unknown -join ', ')) }
    $cases = @($cases | Where-Object { $_ -in $Only })
}
try {
    foreach ($case in $cases) {
        $domain = if ($case.StartsWith('canvas-')) { 'canvas' } else { 'image' }
        $record = [ordered]@{ name = $case; startedAt = [DateTimeOffset]::Now.ToString('o'); wordVersion = $null; wordBuild = $null; windowHandle = $null; status = 'started'; path = $null; preview = $null; details = $null; package = $null; error = $null }
        $script:Details = [ordered]@{}
        try {
            if ($case -eq 'image-emf') { $record.status = 'pending-ui'; $record.error = 'Requires the specified UI copy/paste as enhanced metafile. No substitute was generated.'; continue }
            $script:Word = New-Object -ComObject Word.Application
            $script:Word.Visible = $true
            $script:Word.DisplayAlerts = 0
            $script:Word.AutomationSecurity = 3
            $record.wordVersion = [string]$script:Word.Version
            $record.wordBuild = [string]$script:Word.Build
            $script:Document = $script:Word.Documents.Add()
            if ($null -eq $script:Document) { throw 'Word Documents.Add returned null.' }
            try { $record.windowHandle = [int]$script:Document.ActiveWindow.Hwnd } catch { }
            $script:FeatureParagraph = if ($domain -eq 'canvas') { 1 } else { 2 }
            $script:Document.Content.Text = if ($domain -eq 'canvas') { "`r" + $after + "`r" } else { $before + "`r`r" + $after + "`r" }
            $script:Document.Content.Font.Name = 'Calibri'
            $script:Document.Content.Font.Size = 12
            if ($domain -eq 'canvas') {
                Add-Canvas $case
                $script:Document.Paragraphs.Item(1).Range.InsertParagraphBefore()
                $script:Document.Paragraphs.Item(1).Range.Text = $before + "`r"
            }
            else {
                switch ($case) {
                    'image-inline' { $picture = Add-InlinePicture }
                    'image-wrap-square' { $picture = Add-FloatingPicture 0 }
                    'image-wrap-tight' { $picture = Add-FloatingPicture 1 }
                    'image-behind' { $picture = Add-FloatingPicture 5 }
                    'image-front' { $picture = Add-FloatingPicture 3 }
                    'image-top-bottom' { $picture = Add-FloatingPicture 4 }
                    'image-cropped' { $picture = Add-InlinePicture; $picture.PictureFormat.CropLeft = 18; $picture.PictureFormat.CropTop = 9; $script:Details['cropPoints'] = @([single]$picture.PictureFormat.CropLeft, [single]$picture.PictureFormat.CropTop) }
                    'image-svg' { $picture = Add-InlinePicture -Path $svgAsset }
                    'image-linked' { $picture = Add-InlinePicture -Link $true -Embed $false; $script:Details['sourceAsset'] = $asset; $script:Details['savePictureWithDocument'] = $false }
                    'image-insert-and-link' { $picture = Add-InlinePicture -Link $true -Embed $true; $script:Details['sourceAsset'] = $asset; $script:Details['savePictureWithDocument'] = $true }
                    'image-rotated' { $picture = Add-FloatingPicture 0; $picture.Flip(0); $picture.Rotation = 45; $script:Details['rotationDegrees'] = [single]$picture.Rotation; $script:Details['horizontalFlip'] = [int]$picture.HorizontalFlip }
                    'image-alt-decorative' {
                        $picture = Add-InlinePicture
                        try { $picture.Decorative = -1; $script:Details['decorativeComValue'] = $picture.Decorative }
                        catch { $record.status = 'pending-ui'; $record.error = 'This Word COM InlineShape exposes no writable Decorative property: ' + $_.Exception.Message; continue }
                    }
                    'image-two-in-run' {
                        $picture = Add-InlinePicture
                        $range = $picture.Range.Duplicate
                        $range.Collapse(0)
                        $second = $script:Document.InlineShapes.AddPicture($asset, $false, $true, $range)
                        $second.LockAspectRatio = -1
                        $second.Width = 144
                        $script:Details['secondPictureWidthPoints'] = [single]$second.Width
                    }
                }
                $script:Details['pictureSizePoints'] = @([single]$picture.Width, [single]$picture.Height)
                $script:Details['inlineShapeCount'] = [int]$script:Document.InlineShapes.Count
                $script:Details['floatingShapeCount'] = [int]$script:Document.Shapes.Count
            }
            $path = Get-UnusedPath ($domain + '/' + $case + '.docx')
            $pdf = Join-Path $OutputRoot ('_previews/' + $domain + '/' + [IO.Path]::GetFileNameWithoutExtension($path) + '.pdf')
            $script:Document.SaveAs2($path, 12)
            $record.path = $path
            $script:Document.ExportAsFixedFormat($pdf, 17)
            $record.preview = $pdf
            $script:Document.Close(0)
            $script:Document = $null
            $record.package = Inspect-Copy $path $domain $case
            $record.status = if ($record.package.warnings.Count -gt 0) { 'saved-with-structure-note' } else { 'saved' }
        }
        catch { $record.status = 'failed'; $record.error = $_.Exception.Message + ' at ' + $_.ScriptStackTrace }
        finally {
            if ($null -ne $script:Document) { try { $script:Document.Close(0) } catch { }; $script:Document = $null }
            if ($null -ne $script:Word) {
                try { $script:Word.Quit(0) } catch { }
                try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($script:Word) } catch { }
                $script:Word = $null
            }
            $record.details = $script:Details
            $record.finishedAt = [DateTimeOffset]::Now.ToString('o')
            $rows.Add($record)
            Write-Results
            [pscustomobject]@{ case = $case; status = $record.status; path = $record.path; error = $record.error } | ConvertTo-Json -Compress
        }
    }
} finally {
    if ($null -ne $script:Document) { try { $script:Document.Close(0) } catch { } }
    if ($null -ne $script:Word) { try { $script:Word.Quit(0) } catch { }; [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($script:Word) }
}
