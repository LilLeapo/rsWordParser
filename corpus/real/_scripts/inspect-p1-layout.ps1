#requires -Version 7.0
param([string]$OutputRoot = (Split-Path -Parent $PSScriptRoot))
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$snapshotRoot = Join-Path $OutputRoot ('_checks/p1-layout/' + (Get-Date -Format 'yyyyMMdd-HHmmss-fff'))
[void][IO.Directory]::CreateDirectory($snapshotRoot)

function Read-Part($Archive, [string]$Part) {
    $entry = $Archive.GetEntry($Part)
    if ($null -eq $entry) { return $null }
    $reader = [IO.StreamReader]::new($entry.Open())
    try { [xml]$xml = $reader.ReadToEnd(); return ,$xml } finally { $reader.Dispose() }
}

function Namespace-Map($Xml) {
    $map = [Xml.XmlNamespaceManager]::new($Xml.NameTable)
    $map.AddNamespace('w', 'http://schemas.openxmlformats.org/wordprocessingml/2006/main')
    $map.AddNamespace('w14', 'http://schemas.microsoft.com/office/word/2010/wordml')
    $map.AddNamespace('w15', 'http://schemas.microsoft.com/office/word/2012/wordml')
    $map.AddNamespace('r', 'http://schemas.openxmlformats.org/officeDocument/2006/relationships')
    $map.AddNamespace('pr', 'http://schemas.openxmlformats.org/package/2006/relationships')
    $map.AddNamespace('wp', 'http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing')
    return ,$map
}

$results = [Collections.Generic.List[object]]::new()
foreach ($relativePath in @('revisions/revisions-comments.docx', 'shapes/textbox-shapes.docx', 'shapes/textbox-shapes-2.docx', 'sdt/content-controls.docx', 'sdt/content-controls-2.docx', 'links/hyperlinks-bookmarks.docx')) {
    $source = Join-Path $OutputRoot $relativePath
    $hash = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash
    $copy = Join-Path $snapshotRoot ([IO.Path]::GetFileNameWithoutExtension($source) + '.zip')
    Copy-Item -LiteralPath $source -Destination $copy
    $archive = [IO.Compression.ZipFile]::OpenRead($copy)
    $result = [ordered]@{ file = $relativePath; snapshot = [IO.Path]::GetRelativePath($OutputRoot, $copy).Replace('\', '/'); sha256 = $hash }
    try {
        $xml = Read-Part $archive 'word/document.xml'
        $ns = Namespace-Map $xml
        if ($relativePath.StartsWith('revisions/')) {
            $result['insertedText'] = @($xml.SelectNodes('//w:ins//w:t', $ns) | ForEach-Object InnerText)
            $result['deletedText'] = @($xml.SelectNodes('//w:del//w:delText', $ns) | ForEach-Object InnerText)
            $result['formatChanges'] = $xml.SelectNodes('//w:rPrChange', $ns).Count
            $comments = Read-Part $archive 'word/comments.xml'
            $commentNs = Namespace-Map $comments
            $result['comments'] = @($comments.SelectNodes('/w:comments/w:comment', $commentNs) | ForEach-Object {
                [ordered]@{ id = $_.GetAttribute('id', $commentNs.LookupNamespace('w')); author = $_.GetAttribute('author', $commentNs.LookupNamespace('w')); text = ($_.SelectNodes('.//w:t', $commentNs) | ForEach-Object InnerText) -join '' }
            })
            $extended = Read-Part $archive 'word/commentsExtended.xml'
            $extendedNs = Namespace-Map $extended
            $result['commentThreads'] = @($extended.SelectNodes('//w15:commentEx', $extendedNs) | ForEach-Object {
                $attributes = [ordered]@{}
                foreach ($attribute in $_.Attributes) { $attributes[$attribute.LocalName] = $attribute.Value }
                $attributes
            })
        }
        elseif ($relativePath.StartsWith('shapes/')) {
            $result['textboxes'] = @($xml.SelectNodes('//w:txbxContent', $ns) | ForEach-Object {
                [ordered]@{ paragraphs = @($_.SelectNodes('w:p', $ns) | ForEach-Object { ($_.SelectNodes('.//w:t', $ns) | ForEach-Object InnerText) -join '' }) }
            })
            $result['roundedRectangleGeometryCount'] = $xml.SelectNodes('//*[local-name()="prstGeom" and @prst="roundRect"]', $ns).Count
            $result['groupCountIncludingFallbacks'] = $xml.SelectNodes('//*[local-name()="wgp" or local-name()="group"]', $ns).Count
            $result['wordArtText'] = @($xml.SelectNodes('//*[local-name()="textpath"]/@string', $ns) | ForEach-Object Value)
            $result['anchors'] = @($xml.SelectNodes('//wp:anchor', $ns) | ForEach-Object {
                $anchorParagraph = $_.SelectSingleNode('ancestor::w:p[1]', $ns)
                [ordered]@{
                    name = $_.SelectSingleNode('wp:docPr', $ns).GetAttribute('name')
                    anchorParagraphIndex = $anchorParagraph.SelectNodes('preceding-sibling::w:p', $ns).Count + 1
                    verticalRelativeFrom = $_.SelectSingleNode('wp:positionV', $ns).GetAttribute('relativeFrom')
                    verticalPositionEmu = $_.SelectSingleNode('wp:positionV/wp:posOffset', $ns).InnerText
                    anchorParagraphText = ($anchorParagraph.SelectNodes('w:r/w:t', $ns) | ForEach-Object InnerText) -join ''
                }
            })
        }
        elseif ($relativePath.StartsWith('sdt/')) {
            $result['controls'] = @($xml.SelectNodes('//w:sdt', $ns) | ForEach-Object {
                $properties = $_.SelectSingleNode('w:sdtPr', $ns)
                $kind = @($properties.ChildNodes | Where-Object LocalName -in @('comboBox', 'dropDownList', 'date', 'checkbox') | Select-Object -ExpandProperty LocalName)
                if ($kind.Count -eq 0) { $kind = @('richText') }
                $lock = $properties.SelectSingleNode('w:lock', $ns)
                [ordered]@{
                    type = $kind[0]
                    text = ($_.SelectNodes('w:sdtContent//w:t', $ns) | ForEach-Object InnerText) -join ''
                    entries = @($properties.SelectNodes('.//w:listItem', $ns) | ForEach-Object { $_.GetAttribute('displayText', $ns.LookupNamespace('w')) })
                    lock = if ($null -eq $lock) { '' } else { $lock.GetAttribute('val', $ns.LookupNamespace('w')) }
                    propertiesXml = $properties.OuterXml
                }
            })
        }
        else {
            $rels = Read-Part $archive 'word/_rels/document.xml.rels'
            $relNs = Namespace-Map $rels
            $targets = @{}
            foreach ($relationship in $rels.SelectNodes('/pr:Relationships/pr:Relationship', $relNs)) { $targets[$relationship.GetAttribute('Id')] = $relationship.GetAttribute('Target') }
            $result['hyperlinks'] = @($xml.SelectNodes('//w:hyperlink', $ns) | ForEach-Object {
                $id = $_.GetAttribute('id', $ns.LookupNamespace('r'))
                [ordered]@{ text = ($_.SelectNodes('.//w:t', $ns) | ForEach-Object InnerText) -join ''; target = if ($id) { $targets[$id] } else { '' }; anchor = $_.GetAttribute('anchor', $ns.LookupNamespace('w')) }
            })
        }
    }
    finally { $archive.Dispose() }
    $result['originalUnchanged'] = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash -eq $hash
    $results.Add([pscustomobject]$result)
}
$results | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $OutputRoot '_scripts/p1-layout-details.json') -Encoding utf8
$results | ConvertTo-Json -Depth 12
