param(
    [string]$SourceDirectory = 'C:\Users\Administrator\rsWordParser\corpus\real\_roundtrip',
    [string]$DeliveryDirectory = (Join-Path (Split-Path $PSScriptRoot -Parent) '_roundtrip'),
    [string]$ReportPath = (Join-Path $PSScriptRoot 'roundtrip-structure.json')
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
Add-Type -AssemblyName System.Drawing

function Read-ZipText($Zip, [string]$Part) {
    $entry = $Zip.GetEntry($Part)
    if ($null -eq $entry) { return $null }
    $reader = [IO.StreamReader]::new($entry.Open())
    try { return $reader.ReadToEnd() } finally { $reader.Dispose() }
}

function New-NamespaceManager([xml]$Xml) {
    $ns = [Xml.XmlNamespaceManager]::new($Xml.NameTable)
    $ns.AddNamespace('ct', 'http://schemas.openxmlformats.org/package/2006/content-types')
    $ns.AddNamespace('pr', 'http://schemas.openxmlformats.org/package/2006/relationships')
    $ns.AddNamespace('w', 'http://schemas.openxmlformats.org/wordprocessingml/2006/main')
    $ns.AddNamespace('wp', 'http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing')
    $ns.AddNamespace('a', 'http://schemas.openxmlformats.org/drawingml/2006/main')
    $ns.AddNamespace('c', 'http://schemas.openxmlformats.org/drawingml/2006/chart')
    $ns.AddNamespace('pic', 'http://schemas.openxmlformats.org/drawingml/2006/picture')
    $ns.AddNamespace('r', 'http://schemas.openxmlformats.org/officeDocument/2006/relationships')
    $ns.AddNamespace('s', 'http://schemas.openxmlformats.org/spreadsheetml/2006/main')
    return ,$ns
}

function Get-NodeText($Node, [string]$XPath, $Ns) {
    return (@($Node.SelectNodes($XPath, $Ns) | ForEach-Object { $_.InnerText }) -join '')
}

function Inspect-Docx([string]$Path) {
    $fileStream = [IO.FileStream]::new($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
    $sha = [Security.Cryptography.SHA256]::Create()
    try { $fileHash = [BitConverter]::ToString($sha.ComputeHash($fileStream)).Replace('-', '') } finally { $sha.Dispose() }
    $fileStream.Position = 0
    $zip = [IO.Compression.ZipArchive]::new($fileStream, [IO.Compression.ZipArchiveMode]::Read)
    try {
        $issues = [Collections.Generic.List[string]]::new()
        $parsed = @{}
        foreach ($entry in $zip.Entries) {
            if ($entry.FullName -match '\.(xml|rels)$') {
                try { $parsed[$entry.FullName] = [xml](Read-ZipText $zip $entry.FullName) }
                catch { $issues.Add('XML parse failure: ' + $entry.FullName + ': ' + $_.Exception.Message) }
            }
        }
        [xml]$types = $parsed['[Content_Types].xml']
        $ns = New-NamespaceManager $types
        $duplicateDefaults = @($types.SelectNodes('/ct:Types/ct:Default', $ns) | Group-Object Extension | Where-Object Count -gt 1 | ForEach-Object {
            [ordered]@{ Extension = $_.Name; Count = $_.Count }
        })
        foreach ($duplicate in $duplicateDefaults) { $issues.Add('Duplicate content-type Default extension: ' + $duplicate.Extension) }
        $relationships = @()
        foreach ($part in @($parsed.Keys | Sort-Object)) {
            if ($part -notmatch '\.rels$') { continue }
            $owner = if ($part -eq '_rels/.rels') { '' } else { $part -replace '(^|/)_rels/', '$1' -replace '\.rels$', '' }
            [xml]$relXml = $parsed[$part]
            $relNs = New-NamespaceManager $relXml
            foreach ($rel in $relXml.SelectNodes('/pr:Relationships/pr:Relationship', $relNs)) {
                $external = $rel.GetAttribute('TargetMode') -eq 'External'
                $target = $rel.GetAttribute('Target')
                $resolved = if ($external) { $target } else { ([Uri]::new([Uri]('https://package.invalid/' + $owner), $target)).AbsolutePath.TrimStart('/') }
                $exists = $external -or ($null -ne $zip.GetEntry([Uri]::UnescapeDataString($resolved)))
                if (-not $exists) { $issues.Add('Missing relationship target: ' + $part + ' -> ' + $resolved) }
                $relationships += [ordered]@{ Part = $part; Id = $rel.GetAttribute('Id'); Type = $rel.GetAttribute('Type'); Target = $target; ResolvedTarget = $resolved; External = $external; Exists = $exists }
            }
        }
        $charts = @()
        foreach ($part in @($parsed.Keys | Sort-Object)) {
            if ($part -notmatch '^word/charts/chart[0-9]+\.xml$') { continue }
            [xml]$chart = $parsed[$part]
            $chartNs = New-NamespaceManager $chart
            $plot = $chart.SelectSingleNode('//c:plotArea', $chartNs)
            $series = @($chart.SelectNodes('//c:ser', $chartNs) | ForEach-Object {
                [ordered]@{
                    Name = Get-NodeText $_ 'c:tx//c:pt/c:v|c:tx/c:v' $chartNs
                    Categories = @($_.SelectNodes('c:cat//c:pt', $chartNs) | ForEach-Object { [ordered]@{ Index = $_.GetAttribute('idx'); Value = Get-NodeText $_ 'c:v' $chartNs } })
                    Values = @($_.SelectNodes('c:val//c:pt', $chartNs) | ForEach-Object { [ordered]@{ Index = $_.GetAttribute('idx'); Value = Get-NodeText $_ 'c:v' $chartNs } })
                    Formulas = @($_.SelectNodes('.//c:f', $chartNs) | ForEach-Object { $_.InnerText })
                }
            })
            $axisRefs = @($plot.SelectNodes('c:barChart/c:axId|c:lineChart/c:axId|c:scatterChart/c:axId', $chartNs) | ForEach-Object { $_.GetAttribute('val') })
            $axisDefs = @($plot.SelectNodes('c:catAx/c:axId|c:valAx/c:axId|c:dateAx/c:axId|c:serAx/c:axId', $chartNs) | ForEach-Object { $_.GetAttribute('val') })
            foreach ($axis in $axisRefs) { if ($axis -notin $axisDefs) { $issues.Add('Undefined chart axis in ' + $part + ': ' + $axis) } }
            $charts += [ordered]@{
                Part = $part
                Types = @($plot.ChildNodes | Where-Object LocalName -match 'Chart$' | ForEach-Object LocalName)
                Title = Get-NodeText $chart '//c:title//a:t' $chartNs
                Series = $series
                AxisReferences = $axisRefs
                AxisDefinitions = $axisDefs
                ExternalDataRelationshipIds = @($chart.SelectNodes('//c:externalData', $chartNs) | ForEach-Object { $_.GetAttribute('id', 'http://schemas.openxmlformats.org/officeDocument/2006/relationships') })
            }
        }
        $workbooks = @()
        $media = @()
        foreach ($entry in $zip.Entries) {
            if ($entry.FullName -notmatch '(\.xlsx$|^word/media/.+)') { continue }
            $stream = $entry.Open()
            $memory = [IO.MemoryStream]::new()
            try {
                $stream.CopyTo($memory)
                $memory.Position = 0
                if ($entry.FullName -match '\.xlsx$') {
                    $inner = [IO.Compression.ZipArchive]::new($memory, [IO.Compression.ZipArchiveMode]::Read, $true)
                    try {
                        $sharedStrings = @()
                        $sharedXmlText = Read-ZipText $inner 'xl/sharedStrings.xml'
                        if ($null -ne $sharedXmlText) {
                            [xml]$sharedXml = $sharedXmlText
                            $sharedNs = New-NamespaceManager $sharedXml
                            $sharedStrings = @($sharedXml.SelectNodes('/s:sst/s:si', $sharedNs) | ForEach-Object { Get-NodeText $_ './/s:t' $sharedNs })
                        }
                        $sheets = @()
                        foreach ($sheetEntry in $inner.Entries) {
                            if ($sheetEntry.FullName -notmatch '^xl/worksheets/.*\.xml$') { continue }
                            [xml]$sheet = Read-ZipText $inner $sheetEntry.FullName
                            $sheetNs = New-NamespaceManager $sheet
                            $sheets += [ordered]@{ Part = $sheetEntry.FullName; Cells = @($sheet.SelectNodes('//s:sheetData/s:row/s:c', $sheetNs) | ForEach-Object {
                                $rawValue = Get-NodeText $_ 's:v|s:is/s:t' $sheetNs
                                $cellValue = if ($_.GetAttribute('t') -eq 's') { $sharedStrings[[int]$rawValue] } else { $rawValue }
                                [ordered]@{ Address = $_.GetAttribute('r'); Type = $_.GetAttribute('t'); Value = $cellValue; RawValue = $rawValue }
                            }) }
                        }
                        $workbooks += [ordered]@{ Part = $entry.FullName; Sheets = $sheets }
                    } finally { $inner.Dispose() }
                } else {
                    $sha = [Security.Cryptography.SHA256]::Create()
                    try { $hash = [BitConverter]::ToString($sha.ComputeHash($memory.ToArray())).Replace('-', '') } finally { $sha.Dispose() }
                    $item = [ordered]@{ Part = $entry.FullName; Bytes = $entry.Length; Sha256 = $hash }
                    try {
                        $image = [Drawing.Image]::FromStream($memory, $true, $true)
                        try {
                            $item.Width = $image.Width
                            $item.Height = $image.Height
                            $item.FirstPixel = ([Drawing.Bitmap]$image).GetPixel(0, 0).ToString()
                            $item.DecodeSuccess = $true
                        } finally { $image.Dispose() }
                    } catch { $item.DecodeSuccess = $false; $item.DecodeError = $_.Exception.Message; $issues.Add('Image decode failure: ' + $entry.FullName) }
                    $media += $item
                }
            } finally { $memory.Dispose(); $stream.Dispose() }
        }
        [xml]$document = $parsed['word/document.xml']
        $docNs = New-NamespaceManager $document
        foreach ($picture in $document.SelectNodes('//pic:pic', $docNs)) {
            foreach ($child in @('nvPicPr', 'spPr')) {
                if ($null -eq $picture.SelectSingleNode('pic:' + $child, $docNs)) { $issues.Add('Picture missing required child pic:' + $child) }
            }
        }
        $drawings = @($document.SelectNodes('//wp:inline|//wp:anchor', $docNs) | ForEach-Object {
            $prop = $_.SelectSingleNode('wp:docPr', $docNs)
            $extent = $_.SelectSingleNode('wp:extent', $docNs)
            [ordered]@{
                Kind = $_.LocalName
                Name = $prop.GetAttribute('name')
                SizePx = @(([double]$extent.GetAttribute('cx') / 9525), ([double]$extent.GetAttribute('cy') / 9525))
                HorizontalOffsetPx = if ($null -ne $_.SelectSingleNode('wp:positionH/wp:posOffset', $docNs)) { [double](Get-NodeText $_ 'wp:positionH/wp:posOffset' $docNs) / 9525 } else { $null }
                VerticalOffsetPx = if ($null -ne $_.SelectSingleNode('wp:positionV/wp:posOffset', $docNs)) { [double](Get-NodeText $_ 'wp:positionV/wp:posOffset' $docNs) / 9525 } else { $null }
                HorizontalAlign = Get-NodeText $_ 'wp:positionH/wp:align' $docNs
                BehindDocument = $_.GetAttribute('behindDoc')
                Wrap = @($_.ChildNodes | Where-Object LocalName -match '^wrap' | ForEach-Object LocalName)
                MediaRelationshipIds = @($_.SelectNodes('.//a:blip', $docNs) | ForEach-Object { $_.GetAttribute('embed', 'http://schemas.openxmlformats.org/officeDocument/2006/relationships') })
                ChartRelationshipIds = @($_.SelectNodes('.//c:chart', $docNs) | ForEach-Object { $_.GetAttribute('id', 'http://schemas.openxmlformats.org/officeDocument/2006/relationships') })
            }
        })
        return [ordered]@{ Path = $Path; Sha256 = $fileHash; Parts = @($zip.Entries.FullName); Issues = @($issues); DuplicateContentTypeDefaults = $duplicateDefaults; Text = Get-NodeText $document '//w:t' $docNs; Relationships = $relationships; Charts = $charts; Workbooks = $workbooks; Media = $media; Drawings = $drawings }
    } finally { $zip.Dispose() }
}

$records = @()
foreach ($source in Get-ChildItem -LiteralPath $SourceDirectory -Filter '*.docx' | Sort-Object Name) {
    $delivery = Join-Path $DeliveryDirectory $source.Name
    $result = Inspect-Docx $delivery
    $result.SourceSha256 = (Get-FileHash -LiteralPath $source.FullName -Algorithm SHA256).Hash
    $result.SourceCopyMatches = $result.Sha256 -eq $result.SourceSha256
    $resaved = Join-Path $DeliveryDirectory ($source.BaseName + '-resaved-by-word.docx')
    $result.Resaved = if (Test-Path -LiteralPath $resaved) { Inspect-Docx $resaved } else { $null }
    $records += $result
}
$report = [ordered]@{ GeneratedAt = [DateTimeOffset]::Now.ToString('o'); Method = 'Read-only ZIP/XML, SHA256, and System.Drawing image decoder; no Word automation and no source DOCX writes. This is targeted structure inspection, not full OOXML schema validation.'; Documents = $records }
$report | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath $ReportPath -Encoding UTF8
$records | ForEach-Object { [pscustomobject]@{ File = [IO.Path]::GetFileName($_.Path); CopyMatches = $_.SourceCopyMatches; Issues = ($_.Issues -join '; '); ResavedIssues = if ($null -ne $_.Resaved) { $_.Resaved.Issues -join '; ' } else { '(not present)' } } } | Format-Table -AutoSize -Wrap
