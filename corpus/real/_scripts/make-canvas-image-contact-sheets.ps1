param([string]$OutputRoot = (Split-Path -Parent $PSScriptRoot))
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$results = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'canvas-images-results.json') | ConvertFrom-Json
$rows = @($results.cases | Where-Object path | Group-Object name | ForEach-Object { $_.Group | Select-Object -Last 1 } | Sort-Object name)
$font = [Drawing.Font]::new('Consolas', 16)
try {
    for ($start = 0; $start -lt $rows.Count; $start += 6) {
        $subset = @($rows | Select-Object -Skip $start -First 6)
        $bitmap = [Drawing.Bitmap]::new(1416, [int][Math]::Ceiling($subset.Count / 2.0) * 525)
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        try {
            $graphics.Clear([Drawing.Color]::White)
            for ($index = 0; $index -lt $subset.Count; $index++) {
                $preview = [IO.Path]::ChangeExtension($subset[$index].preview, '.png')
                $image = [Drawing.Image]::FromFile($preview)
                try {
                    $x = ($index % 2) * 708
                    $y = [int][Math]::Floor($index / 2.0) * 525
                    $graphics.DrawString([IO.Path]::GetFileNameWithoutExtension($preview), $font, [Drawing.Brushes]::Black, [single]($x + 10), [single]($y + 4))
                    $graphics.DrawImage($image, [Drawing.Rectangle]::new($x, $y + 25, 708, 500), [Drawing.Rectangle]::new(0, 0, 708, 500), [Drawing.GraphicsUnit]::Pixel)
                } finally { $image.Dispose() }
            }
            $destination = Join-Path $OutputRoot ('_previews/canvas-image-contact-' + [string]([int]($start / 6) + 1) + '.png')
            $bitmap.Save($destination, [Drawing.Imaging.ImageFormat]::Png)
            $destination
        } finally { $graphics.Dispose(); $bitmap.Dispose() }
    }
} finally { $font.Dispose() }
