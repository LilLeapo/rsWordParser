param([string]$OutputRoot = (Split-Path -Parent $PSScriptRoot))
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
Add-Type -AssemblyName System.Drawing
$relativePaths = @('image/image-emf.docx', 'chart/chart-pasted-embedded.docx', 'chart/chart-pasted-linked.docx', 'chart/chart-pasted-picture.docx', 'chart/chartex-sunburst-2.docx', 'chart/chartex-treemap.docx')
$copyRoot = Join-Path $OutputRoot ('_checks/ui-offline/' + [DateTime]::Now.ToString('yyyyMMdd-HHmmss-fff'))
[void][IO.Directory]::CreateDirectory($copyRoot)
$results = @()
foreach ($relative in $relativePaths) {
    $path = Join-Path $OutputRoot $relative
    if (-not (Test-Path -LiteralPath $path)) { continue }
    $hashBefore = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
    $copy = Join-Path $copyRoot ($relative.Replace('/', '-') -replace '\.docx$', '.zip')
    Copy-Item -LiteralPath $path -Destination $copy
    $zip = [IO.Compression.ZipFile]::OpenRead($copy)
    try {
        $allXml = @{}
        foreach ($entry in $zip.Entries) {
            if ($entry.FullName -match '\.(xml|rels)$') {
                $stream = $entry.Open()
                try { $xml = [xml]::new(); $xml.Load($stream); $allXml[$entry.FullName] = $xml } finally { $stream.Dispose() }
            }
        }
        $doc = $allXml['word/document.xml']
        $tags = [ordered]@{}
        foreach ($name in @('inline', 'anchor', 'pic', 'chart', 'OLEObject', 'srcRect', 'svgBlip', 'AlternateContent', 'Fallback')) {
            $tags[$name] = $doc.SelectNodes('//*[local-name()="' + $name + '"]').Count
        }
        $media = @()
        foreach ($entry in $zip.Entries | Where-Object FullName -match '^word/media/.+') {
            $stream = $entry.Open()
            $memory = [IO.MemoryStream]::new()
            try {
                $stream.CopyTo($memory)
                $memory.Position = 0
                $record = [ordered]@{ part = $entry.FullName; bytes = $entry.Length }
                try {
                    $image = [Drawing.Image]::FromStream($memory, $true, $true)
                    try { $record.decoded = $true; $record.width = $image.Width; $record.height = $image.Height; $record.rawFormatGuid = $image.RawFormat.Guid.ToString() } finally { $image.Dispose() }
                } catch { $record.decoded = $false; $record.error = $_.Exception.Message }
                $media += $record
            } finally { $memory.Dispose(); $stream.Dispose() }
        }
        $charts = @()
        foreach ($key in @($allXml.Keys | Where-Object { $_ -match '^word/charts/chart(Ex)?\d+\.xml$' })) {
            $xml = $allXml[$key]
            $charts += [ordered]@{
                part = $key
                layoutIds = @($xml.SelectNodes('//*[@layoutId]') | ForEach-Object { $_.GetAttribute('layoutId') })
                externalData = @($xml.SelectNodes('//*[local-name()="externalData"]') | ForEach-Object OuterXml)
                refErrorCount = $xml.SelectNodes('//*[text()="#REF!"]').Count
                dimensions = @($xml.SelectNodes('//*[local-name()="strDim" or local-name()="numDim"]') | ForEach-Object {
                    [ordered]@{ kind = $_.LocalName; type = $_.GetAttribute('type'); formula = ($_.SelectNodes('./*[local-name()="f"]') | ForEach-Object InnerText) -join ''; points = @($_.SelectNodes('.//*[local-name()="pt"]') | ForEach-Object InnerText) }
                })
            }
        }
        $rels = @()
        foreach ($key in @($allXml.Keys | Where-Object { $_ -match '\.rels$' })) {
            foreach ($node in $allXml[$key].SelectNodes('//*[local-name()="Relationship"]')) {
                if ($node.GetAttribute('Type') -match '/(image|chart|chartEx|package|oleObject)$') {
                    $rels += [ordered]@{ part = $key; id = $node.GetAttribute('Id'); type = $node.GetAttribute('Type'); target = $node.GetAttribute('Target'); targetMode = $node.GetAttribute('TargetMode') }
                }
            }
        }
        $results += [ordered]@{
            file = $relative
            copy = $copy
            sourceSha256 = $hashBefore
            originalUnchanged = $hashBefore -eq (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
            parts = @($zip.Entries.FullName)
            tags = $tags
            docPr = @($doc.SelectNodes('//*[local-name()="docPr"]') | ForEach-Object OuterXml)
            transforms = @($doc.SelectNodes('//*[local-name()="xfrm"]') | ForEach-Object OuterXml)
            sourceRectangles = @($doc.SelectNodes('//*[local-name()="srcRect"]') | ForEach-Object OuterXml)
            inlineExtents = @($doc.SelectNodes('//*[local-name()="inline"]/*[local-name()="extent"]') | ForEach-Object OuterXml)
            media = $media
            charts = $charts
            relationships = $rels
        }
    } finally { $zip.Dispose() }
}
$destination = Join-Path $PSScriptRoot 'ui-samples-offline-check.json'
$results | ConvertTo-Json -Depth 15 | Set-Content -LiteralPath $destination -Encoding utf8
$results | ForEach-Object { [pscustomobject]@{ File = $_.file; OriginalUnchanged = $_.originalUnchanged; Tags = $_.tags; Media = $_.media; Charts = $_.charts; Relationships = $_.relationships } } | ConvertTo-Json -Depth 12
