param([string]$OutputRoot = (Split-Path -Parent $PSScriptRoot))
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
Add-Type -AssemblyName System.Drawing
$copyRoot = Join-Path $OutputRoot ('_checks/ink-offline/' + [DateTime]::Now.ToString('yyyyMMdd-HHmmss-fff'))
[void][IO.Directory]::CreateDirectory($copyRoot)
$results = @()
foreach ($path in Get-ChildItem -LiteralPath (Join-Path $OutputRoot 'ink') -Filter '*.docx' | Where-Object Name -notlike '~$*') {
    $hashBefore = (Get-FileHash -LiteralPath $path.FullName -Algorithm SHA256).Hash
    $copy = Join-Path $copyRoot ($path.BaseName + '.zip')
    Copy-Item -LiteralPath $path.FullName -Destination $copy
    $zip = [IO.Compression.ZipFile]::OpenRead($copy)
    try {
        $xmlParts = @{}
        foreach ($entry in $zip.Entries | Where-Object FullName -match '\.(xml|rels)$') {
            $stream = $entry.Open()
            try { $xml = [xml]::new(); $xml.Load($stream); $xmlParts[$entry.FullName] = $xml } finally { $stream.Dispose() }
        }
        $doc = $xmlParts['word/document.xml']
        $ink = @()
        foreach ($part in $xmlParts.Keys | Where-Object { $_ -match '^word/ink/.+\.xml$' } | Sort-Object) {
            $xml = $xmlParts[$part]
            $ink += [ordered]@{
                part = $part
                namespace = $xml.DocumentElement.NamespaceURI
                traceCount = $xml.SelectNodes('//*[local-name()="trace"]').Count
                traces = @($xml.SelectNodes('//*[local-name()="trace"]') | ForEach-Object InnerText)
                brushProperties = @($xml.SelectNodes('//*[local-name()="brushProperty"]') | ForEach-Object { [ordered]@{ name = $_.GetAttribute('name'); value = $_.GetAttribute('value'); units = $_.GetAttribute('units') } })
                channels = @($xml.SelectNodes('//*[local-name()="channel"]') | ForEach-Object OuterXml)
            }
        }
        $media = @()
        foreach ($entry in $zip.Entries | Where-Object FullName -match '^word/media/.+') {
            $stream = $entry.Open()
            $memory = [IO.MemoryStream]::new()
            try {
                $stream.CopyTo($memory); $memory.Position = 0
                $record = [ordered]@{ part = $entry.FullName; bytes = $entry.Length }
                try {
                    $bitmap = [Drawing.Bitmap]::new($memory)
                    try {
                        $record.decoded = $true; $record.width = $bitmap.Width; $record.height = $bitmap.Height
                        $colors = @{}
                        for ($y = 0; $y -lt $bitmap.Height; $y++) {
                            for ($x = 0; $x -lt $bitmap.Width; $x++) {
                                $color = $bitmap.GetPixel($x, $y)
                                if ($color.A -gt 0) { $key = $color.ToArgb().ToString('X8'); $colors[$key] = 1 + [int]$colors[$key] }
                            }
                        }
                        $record.visibleArgbCounts = $colors
                    } finally { $bitmap.Dispose() }
                } catch { $record.decoded = $false; $record.error = $_.Exception.Message }
                $media += $record
            } finally { $memory.Dispose(); $stream.Dispose() }
        }
        $counts = [ordered]@{}
        foreach ($name in @('contentPart', 'AlternateContent', 'Choice', 'Fallback', 'anchor', 'inline', 'pict', 'pic', 'sp', 'wsp', 'prstGeom', 'oMath', 'oMathPara')) {
            $counts[$name] = $doc.SelectNodes('//*[local-name()="' + $name + '"]').Count
        }
        $results += [ordered]@{
            file = 'ink/' + $path.Name
            copy = $copy
            sourceSha256 = $hashBefore
            originalUnchanged = $hashBefore -eq (Get-FileHash -LiteralPath $path.FullName -Algorithm SHA256).Hash
            parts = @($zip.Entries.FullName)
            counts = $counts
            contentParts = @($doc.SelectNodes('//*[local-name()="contentPart"]') | ForEach-Object OuterXml)
            choiceRequirements = @($doc.SelectNodes('//*[local-name()="Choice"]') | ForEach-Object { $_.GetAttribute('Requires') })
            anchorExtents = @($doc.SelectNodes('//*[local-name()="anchor"]/*[local-name()="extent"]') | ForEach-Object OuterXml)
            presetGeometry = @($doc.SelectNodes('//*[local-name()="prstGeom"]') | ForEach-Object OuterXml)
            math = @($doc.SelectNodes('//*[local-name()="oMath"]') | ForEach-Object OuterXml)
            ink = $ink
            media = $media
            relationships = @($xmlParts['word/_rels/document.xml.rels'].SelectNodes('//*[local-name()="Relationship"]') | Where-Object { $_.GetAttribute('Target') -match '^(ink|media)/' } | ForEach-Object { [ordered]@{ id = $_.GetAttribute('Id'); type = $_.GetAttribute('Type'); target = $_.GetAttribute('Target') } })
        }
    } finally { $zip.Dispose() }
}
$results | ConvertTo-Json -Depth 15 | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'ink-offline-review.json') -Encoding utf8
$results | ForEach-Object { [pscustomobject]@{ File = $_.file; OriginalUnchanged = $_.originalUnchanged; Counts = $_.counts; Ink = $_.ink; Media = $_.media } } | ConvertTo-Json -Depth 12
