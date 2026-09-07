param(
    [string]$OutputDir = (Split-Path -Parent $PSScriptRoot),
    [string[]]$Only = @(),
    [switch]$ExportPdf,
    [switch]$ReviewBarrier,
    [object]$Word = $null
)

$ErrorActionPreference = 'Stop'
$missing = [Type]::Missing
Add-Type -AssemblyName System.IO.Compression.FileSystem
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.Drawing

function Unicode-Text([string]$Hex) {
    return -join @($Hex.Split(' ') | ForEach-Object { [char][Convert]::ToInt32($_,16) })
}
[string]$before = 'before ' + (Unicode-Text '524D 6587')
[string]$after = 'after ' + (Unicode-Text '540E 6587')
[string]$authorA = [string](Unicode-Text '4F5C 8005 7532')
[string]$authorB = [string](Unicode-Text '4F5C 8005 4E59')
[string]$figureLabel = [string](Unicode-Text '56FE')
$results = [Collections.Generic.List[object]]::new()
$scriptDirectory = Join-Path $OutputDir '_scripts'
New-Item -ItemType Directory -Path $scriptDirectory -Force | Out-Null
$resultPath = Join-Path $scriptDirectory 'task-d-results.json'
[string]$assetPath = [string](Join-Path $scriptDirectory 'task-d-picture.png')
$bitmap = [Drawing.Bitmap]::new(64,32)
$graphics = [Drawing.Graphics]::FromImage($bitmap)
try {
    $graphics.Clear([Drawing.Color]::FromArgb(33,126,167))
    $graphics.FillRectangle([Drawing.Brushes]::Gold,32,0,32,32)
    $bitmap.Save($assetPath,[Drawing.Imaging.ImageFormat]::Png)
} finally { $graphics.Dispose(); $bitmap.Dispose() }

function Find-Text($Document,[string]$Text,[switch]$Last) {
    $range = $Document.Content.Duplicate
    $find=$range.Find
    $original=@{}
    foreach ($name in 'MatchCase','MatchWholeWord','MatchWildcards','MatchSoundsLike','MatchAllWordForms','Forward','Wrap','Format','Text') { $original[$name]=$find.$name }
    try {
        $find.MatchCase=$true; $find.MatchWholeWord=$false; $find.MatchWildcards=$false
        $find.MatchSoundsLike=$false; $find.MatchAllWordForms=$false; $find.Format=$false
        $find.Forward = -not $Last
        $find.Wrap = 0
        if (-not $find.Execute($Text)) { throw "Text not found: $Text" }
    } finally { foreach ($name in $original.Keys) { $find.$name=$original[$name] } }
    return $range
}
function Set-Body($Document,[string]$Text) {
    $Document.Content.Text = "$before`r$Text`r$after`r"
}
function End-Range($Document) {
    return $Document.Range($Document.Content.End-1,$Document.Content.End-1)
}
function Attempt([string]$Description,[scriptblock]$Action) {
    try { & $Action | Out-Null; $entry.operations.Add([ordered]@{operation=$Description;status='ok'}) }
    catch { $entry.operations.Add([ordered]@{operation=$Description;status='error';error=$_.Exception.Message}) }
}
function New-BibliographySource([string]$Tag,[string]$Kind,[string]$Title,[string]$Surname,[string]$Year) {
    # Sources.Add requires source XML; Word owns serialization into the DOCX package.
    $namespace = 'http://schemas.openxmlformats.org/officeDocument/2006/bibliography'
    $xml = [Xml.XmlDocument]::new()
    $root = $xml.CreateElement('b','Source',$namespace)
    [void]$xml.AppendChild($root)
    foreach ($pair in @(@('Tag',$Tag),@('SourceType',$Kind),@('Title',$Title),@('Year',$Year),@('Guid',([guid]::NewGuid().ToString('B'))))) {
        $node = $xml.CreateElement('b',$pair[0],$namespace); $node.InnerText=$pair[1]; [void]$root.AppendChild($node)
    }
    $parent = $root
    foreach ($name in 'Author','Author','NameList','Person') {
        $node=$xml.CreateElement('b',$name,$namespace); [void]$parent.AppendChild($node); $parent=$node
    }
    $node=$xml.CreateElement('b','Last',$namespace); $node.InnerText=$Surname; [void]$parent.AppendChild($node)
    $node=$xml.CreateElement('b','First',$namespace); $node.InnerText='Case'; [void]$parent.AppendChild($node)
    foreach ($pair in $(if ($Kind -eq 'Book') { @(@('City','Test City'),@('Publisher','Corpus Press')) } else { @(@('JournalName','Corpus Journal'),@('Volume','7'),@('Issue','2'),@('Pages','10-18')) })) {
        $node=$xml.CreateElement('b',$pair[0],$namespace); $node.InnerText=$pair[1]; [void]$root.AppendChild($node)
    }
    return $xml.OuterXml
}
function Get-SharedFileSha256([string]$Path) {
    $stream = [IO.File]::Open($Path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
    $sha256 = [Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($sha256.ComputeHash($stream)).Replace('-','') }
    finally { $sha256.Dispose(); $stream.Dispose() }
}
function Read-Package([string]$Path) {
    $stream = [IO.File]::Open($Path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
    $zip = $null
    $parts = @{}
    try {
        $zip = [IO.Compression.ZipArchive]::new($stream,[IO.Compression.ZipArchiveMode]::Read,$true)
        foreach ($part in $zip.Entries) {
            if ($part.FullName -match '\.xml$') {
                $reader = [IO.StreamReader]::new($part.Open())
                try {
                    $xml = [Xml.XmlDocument]::new()
                    $xml.PreserveWhitespace=$true
                    $xml.LoadXml($reader.ReadToEnd())
                    $parts[$part.FullName]=$xml
                } finally { $reader.Dispose() }
            }
        }
    } finally { if ($null -ne $zip) { $zip.Dispose() }; $stream.Dispose() }
    return $parts
}
function Get-FieldCodes($Xml,$NamespaceManager) {
    $codes=[Collections.Generic.List[string]]::new()
    $stack=[Collections.Generic.List[object]]::new()
    $namespace=$NamespaceManager.LookupNamespace('w')
    foreach ($node in $Xml.SelectNodes('//w:fldChar | //w:instrText | //w:fldSimple',$NamespaceManager)) {
        if ($node.LocalName -eq 'fldSimple') { $codes.Add($node.GetAttribute('instr',$namespace)); continue }
        if ($node.LocalName -eq 'instrText') {
            if ($stack.Count -gt 0 -and $stack[$stack.Count-1].reading) { [void]$stack[$stack.Count-1].code.Append($node.InnerText) }
            continue
        }
        switch ($node.GetAttribute('fldCharType',$namespace)) {
            'begin' { $stack.Add(@{reading=$true;code=[Text.StringBuilder]::new()}) }
            'separate' { if ($stack.Count -gt 0) { $stack[$stack.Count-1].reading=$false } }
            'end' {
                if ($stack.Count -gt 0) { $codes.Add($stack[$stack.Count-1].code.ToString()); $stack.RemoveAt($stack.Count-1) }
            }
        }
    }
    return $codes.ToArray()
}
function Check-Package([string]$Case,[string]$Path) {
    $parts=Read-Package $Path
    $xml=$parts['word/document.xml']
    $ns=[Xml.XmlNamespaceManager]::new($xml.NameTable)
    foreach ($pair in @(
        @('w','http://schemas.openxmlformats.org/wordprocessingml/2006/main'),
        @('b','http://schemas.openxmlformats.org/officeDocument/2006/bibliography'),
        @('wp','http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing'),
        @('wps','http://schemas.microsoft.com/office/word/2010/wordprocessingShape'),
        @('w15','http://schemas.microsoft.com/office/word/2012/wordml')
    )) { $ns.AddNamespace($pair[0],$pair[1]) }
    $counts=[ordered]@{}
    foreach ($name in 'p','ins','del','delText','moveFrom','moveTo','moveFromRangeStart','moveToRangeStart','moveFromRangeEnd','moveToRangeEnd','rPrChange','pPrChange','sectPrChange','tcPrChange','tblPrChange','cellMerge','sectPr') {
        $counts[$name]=$xml.SelectNodes("//w:$name",$ns).Count
    }
    $counts.rowInsert=$xml.SelectNodes('//w:trPr/w:ins',$ns).Count
    $counts.rowDelete=$xml.SelectNodes('//w:trPr/w:del',$ns).Count
    $counts.numberingRevision=$xml.SelectNodes('//w:pPr[w:numPr and w:pPrChange] | //w:numPr/w:numberingChange | //w:numPr/w:ins',$ns).Count
    $counts.anchors=$xml.SelectNodes('//wp:anchor',$ns).Count
    $counts.linkedTextbox=$xml.SelectNodes('//wps:linkedTxbx',$ns).Count
    $fields=@(Get-FieldCodes $xml $ns)
    $checks=[Collections.Generic.List[object]]::new()
    function Add-Check([string]$Name,[bool]$Passed,$Actual) { $checks.Add([ordered]@{name=$Name;passed=$Passed;actual=$Actual}) }
    $allText= -join @($xml.SelectNodes('//w:t',$ns) | ForEach-Object InnerText)
    if ($Case -notlike 'blank/*') { Add-Check 'before/after markers' ($allText.Contains($before) -and $allText.Contains($after)) $allText }
    switch (($Case -split '/')[-1]) {
        { $_ -in 'blank-new','blank-styles-used' } {
            $body=$xml.SelectSingleNode('/w:document/w:body',$ns)
            $children=@($body.ChildNodes | Where-Object NodeType -eq Element)
            Add-Check 'one empty paragraph and final sectPr only' ($children.Count -eq 2 -and $children[0].LocalName -eq 'p' -and $children[1].LocalName -eq 'sectPr' -and $children[0].SelectNodes('.//w:t | .//w:delText',$ns).Count -eq 0) @($children | ForEach-Object LocalName)
            foreach ($name in 'word/styles.xml','word/settings.xml','word/theme/theme1.xml','word/fontTable.xml','word/webSettings.xml') { Add-Check "part $name" ($parts.ContainsKey($name)) ($parts.ContainsKey($name)) }
            if ($_ -eq 'blank-styles-used') {
                $styles=$parts['word/styles.xml']
                foreach ($name in 'heading 1','heading 2','List Paragraph') {
                    $found=$styles.SelectNodes('//w:style/w:name',$ns) | Where-Object { $_.GetAttribute('val',$ns.LookupNamespace('w')) -ieq $name }
                    Add-Check "used style $name" (@($found).Count -gt 0) @($found | ForEach-Object OuterXml)
                }
            }
        }
        'rev-insert-delete' {
            $authors=@($xml.SelectNodes('//w:ins/@w:author | //w:del/@w:author',$ns) | ForEach-Object Value | Select-Object -Unique)
            Add-Check 'two insertions, deletion text and two authors' ($counts.ins -eq 2 -and $counts.del -ge 1 -and $counts.delText -ge 1 -and $authors.Count -eq 2) @{counts=$counts;authors=$authors}
        }
        'rev-move' { Add-Check 'tracked move wrappers and paired range markers' ($counts.moveFrom -ge 1 -and $counts.moveTo -ge 1 -and $counts.moveFromRangeStart -ge 1 -and $counts.moveToRangeStart -ge 1 -and $counts.moveFromRangeEnd -eq $counts.moveFromRangeStart -and $counts.moveToRangeEnd -eq $counts.moveToRangeStart) $counts }
        'rev-format' { Add-Check 'run, paragraph and numbering revisions' ($counts.rPrChange -ge 1 -and $counts.pPrChange -ge 1 -and $counts.numberingRevision -ge 1) $counts }
        'rev-table' {
            foreach ($name in 'rowInsert','rowDelete','tcPrChange','tblPrChange') { Add-Check $name ($counts[$name] -ge 1) $counts[$name] }
            Add-Check 'merged cell span' ($xml.SelectNodes('//w:gridSpan[@w:val="2"]',$ns).Count -ge 1) $xml.SelectNodes('//w:gridSpan',$ns).Count
            $borderColors=@($xml.SelectNodes('//w:tblPr[not(ancestor::w:tblPrChange)]/w:tblBorders/*/@w:color',$ns) | ForEach-Object Value)
            Add-Check 'current table borders are red' ($borderColors.Count -ge 4 -and @($borderColors | Where-Object { $_ -ine 'FF0000' }).Count -eq 0) $borderColors
        }
        'rev-section' { Add-Check 'section property revision' ($counts.sectPrChange -ge 1) $counts.sectPrChange }
        'rev-accept-reject' {
            Add-Check 'one remaining insertion, accepted retained, rejected absent' ($counts.ins -eq 1 -and $allText.Contains('ACCEPTED-INSERT') -and (-not $allText.Contains('REJECTED-INSERT')) -and $allText.Contains('PENDING-INSERT')) @{counts=$counts;text=$allText}
        }
        'rev-comment-threads' {
            $commentCount=0; $parentCount=0; $doneCount=0
            $threadNodes=@(); $nestedResolved=$false
            if ($parts.ContainsKey('word/comments.xml')) { $commentCount=$parts['word/comments.xml'].SelectNodes('//w:comment',$ns).Count }
            if ($parts.ContainsKey('word/commentsExtended.xml')) {
                $extended=$parts['word/commentsExtended.xml']
                $parentCount=$extended.SelectNodes('//@w15:paraIdParent',$ns).Count
                $doneCount=$extended.SelectNodes('//*[@w15:done="1" or @w15:done="true"]',$ns).Count
                $threadNodes=@($extended.SelectNodes('//w15:commentEx',$ns) | ForEach-Object { @{id=$_.GetAttribute('paraId',$ns.LookupNamespace('w15'));parent=$_.GetAttribute('paraIdParent',$ns.LookupNamespace('w15'));done=$_.GetAttribute('done',$ns.LookupNamespace('w15'))} })
                foreach ($rootNode in $threadNodes | Where-Object { -not $_.parent -and $_.done -in '1','true' }) {
                    foreach ($replyNode in $threadNodes | Where-Object parent -eq $rootNode.id) {
                        if (@($threadNodes | Where-Object parent -eq $replyNode.id).Count -ge 1) { $nestedResolved=$true }
                    }
                }
            }
            Add-Check 'five comments with replies and resolved state' ($commentCount -eq 5 -and $parentCount -ge 2 -and $doneCount -ge 1) @{comments=$commentCount;paraIdParents=$parentCount;done=$doneCount}
            Add-Check 'resolved root has reply-to-reply depth two' ($nestedResolved) $threadNodes
        }
        'fields-seq-captions' {
            $sequenceCount=@($fields | Where-Object { $_ -match ('\bSEQ\s+'+[regex]::Escape($figureLabel)) }).Count
            Add-Check 'three SEQ figure fields and a REF' ($sequenceCount -eq 3 -and @($fields | Where-Object { $_ -match '\bREF\s' }).Count -ge 1) $fields
        }
        'fields-index' { Add-Check 'three XE fields and INDEX' (@($fields | Where-Object { $_ -match '^\s*XE\s' }).Count -eq 3 -and @($fields | Where-Object { $_ -match '^\s*INDEX\b' }).Count -ge 1) $fields }
        'fields-toc-stale' {
            Add-Check 'TOC exists and old cached titles coexist with changed headings' (@($fields | Where-Object { $_ -match '^\s*TOC\b' }).Count -eq 1 -and $allText.Contains('Original heading one') -and $allText.Contains('Original heading two') -and $allText.Contains('Changed heading one') -and $allText.Contains('Changed heading two')) $allText
            $counts.bookmarkStart=$xml.SelectNodes('//w:bookmarkStart',$ns).Count
            $counts.bookmarkEnd=$xml.SelectNodes('//w:bookmarkEnd',$ns).Count
            $bookmarkNames=@($xml.SelectNodes('//w:bookmarkStart/@w:name',$ns) | ForEach-Object Value)
            $bookmarkIds=@($xml.SelectNodes('//w:bookmarkStart/@w:id',$ns) | ForEach-Object Value)
            $bookmarkEndIds=@($xml.SelectNodes('//w:bookmarkEnd/@w:id',$ns) | ForEach-Object Value)
            $references=@(foreach ($field in $fields) {
                $match=[regex]::Match($field,'(?i)^\s*PAGEREF\s+(?:"(?<target>[^"]+)"|(?<target>\S+))')
                if (-not $match.Success -and $field -match '(?i)^\s*HYPERLINK\b') { $match=[regex]::Match($field,'(?i)\\l\s+(?:"(?<target>[^"]+)"|(?<target>\S+))') }
                if ($match.Success) { @{field=$field;target=$match.Groups['target'].Value} }
            })
            $references+=@($xml.SelectNodes('//w:hyperlink[@w:anchor]',$ns) | ForEach-Object { @{field='w:hyperlink/@w:anchor';target=$_.GetAttribute('anchor',$ns.LookupNamespace('w'))} })
            $targets=@($references | ForEach-Object target | Select-Object -Unique)
            $missingTargets=@($targets | Where-Object { $_ -notin $bookmarkNames })
            $unpairedIds=@($bookmarkIds | Where-Object { $_ -notin $bookmarkEndIds })+@($bookmarkEndIds | Where-Object { $_ -notin $bookmarkIds })
            Add-Check 'three TOC bookmark targets exist with paired markers' ($targets.Count -eq 3 -and $missingTargets.Count -eq 0 -and $unpairedIds.Count -eq 0 -and $counts.bookmarkStart -eq $counts.bookmarkEnd) @{references=$references;targets=$targets;missingTargets=$missingTargets;bookmarkNames=$bookmarkNames;bookmarkStart=$counts.bookmarkStart;bookmarkEnd=$counts.bookmarkEnd;unpairedIds=$unpairedIds}
            $errorPrefix=Unicode-Text '9519 8BEF'
            Add-Check 'cached field results contain no bookmark error' ($allText -notmatch '(?i)Error!\s*(Bookmark not defined|Reference source not found)' -and -not $allText.Contains($errorPrefix)) $allText
        }
        'fields-citations' {
            $sourceParts=@($parts.Keys | Where-Object { $_ -like 'customXml/*' -and $parts[$_].DocumentElement.LocalName -eq 'Sources' -and $parts[$_].DocumentElement.NamespaceURI -eq 'http://schemas.openxmlformats.org/officeDocument/2006/bibliography' })
            $sourceTypes=@(foreach ($key in $sourceParts) { $parts[$key].SelectNodes('/b:Sources/b:Source/b:SourceType',$ns) | ForEach-Object InnerText })
            Add-Check 'two CITATION fields, bibliography, embedded book and journal sources' (@($fields | Where-Object { $_ -match '^\s*CITATION\b' }).Count -eq 2 -and @($fields | Where-Object { $_ -match '^\s*BIBLIOGRAPHY\b' }).Count -eq 1 -and $sourceTypes.Count -eq 2 -and $sourceTypes -contains 'Book' -and $sourceTypes -contains 'JournalArticle') @{fields=$fields;sourceParts=$sourceParts;sourceTypes=$sourceTypes}
        }
        'fields-page-in-footer' {
            $footerFields=@(foreach ($key in $parts.Keys | Where-Object { $_ -match '^word/footer\d+\.xml$' }) { Get-FieldCodes $parts[$key] $ns })
            Add-Check 'PAGE and NUMPAGES in footer' (@($footerFields | Where-Object { $_ -match '^\s*PAGE\b' }).Count -eq 1 -and @($footerFields | Where-Object { $_ -match '^\s*NUMPAGES\b' }).Count -eq 1) $footerFields
            Add-Check 'three Word pages' ($entry.pages -eq 3) $entry.pages
        }
        'sections-breaks-zoo' {
            $sections=@($xml.SelectNodes('//w:sectPr[not(ancestor::w:sectPrChange)]',$ns))
            $types=@($sections | ForEach-Object { $type=$_.SelectSingleNode('w:type/@w:val',$ns); if ($type) { $type.Value } else { 'nextPage (default)' } })
            Add-Check 'five sections; continuous, even and odd page types' ($sections.Count -eq 5 -and ($types -contains 'continuous') -and ($types -contains 'evenPage') -and ($types -contains 'oddPage') -and @($types | Where-Object { $_ -like 'nextPage*' }).Count -ge 1) $types
            Add-Check 'third section first-page header' ($sections.Count -eq 5 -and $sections[2].SelectNodes('w:titlePg | w:headerReference[@w:type="first"]',$ns).Count -eq 2) $(if ($sections.Count -ge 3) { $sections[2].OuterXml } else { 'missing' })
        }
        'image-z-order' {
            $anchors=@($xml.SelectNodes('//wp:anchor',$ns) | ForEach-Object { [ordered]@{name=$_.SelectSingleNode('wp:docPr',$ns).GetAttribute('name');relativeHeight=[long]$_.GetAttribute('relativeHeight');hasRelativeHeight=$_.HasAttribute('relativeHeight')} })
            $first=@($anchors | Where-Object name -eq 'Round2 Picture 1'); $second=@($anchors | Where-Object name -eq 'Round2 Picture 2'); $third=@($anchors | Where-Object name -eq 'Round2 Picture 3')
            $uniqueNames=$anchors.Count -eq 3 -and $first.Count -eq 1 -and $second.Count -eq 1 -and $third.Count -eq 1 -and @($anchors | Where-Object { -not $_.hasRelativeHeight }).Count -eq 0
            Add-Check 'three anchors ordered picture 2 < 1 < 3' ($uniqueNames -and $second[0].relativeHeight -lt $first[0].relativeHeight -and $first[0].relativeHeight -lt $third[0].relativeHeight) $anchors
        }
        'textbox-linked' {
            $linked=$xml.SelectNodes('//wps:txbx[@id] | //wps:txbx[@seq] | //wps:linkedTxbx',$ns)
            Add-Check 'two native linked textbox representations' ($linked.Count -ge 2 -and $counts.linkedTextbox -ge 1) @($linked | ForEach-Object OuterXml)
        }
    }
    return [ordered]@{passed=(@($checks | Where-Object { -not $_.passed }).Count -eq 0);checks=@($checks.ToArray());counts=$counts;fieldCodes=$fields}
}
function Write-Reports {
    ConvertTo-Json -InputObject @($results.ToArray()) -Depth 18 | Set-Content -LiteralPath $resultPath -Encoding UTF8
    foreach ($domain in 'blank','revisions2','fields2','sections2','image2','shapes2','ink') {
        $dir=Join-Path $OutputDir $domain
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
        $lines=[Collections.Generic.List[string]]::new()
        $lines.Add('# Task D observations')
        $lines.Add('')
        $lines.Add('Documents were authored and saved once by desktop Microsoft Word COM. Package checks use shared read-only file streams. Visual inspection is pending unless separately recorded. PDF export, when requested, occurs after the original DOCX save and is never saved back to DOCX.')
        $lines.Add('')
        $lines.Add('| File | Word build / platform | Method | Actions and Word readings | Package selfcheck / limitations |')
        $lines.Add('| --- | --- | --- | --- | --- |')
        foreach ($result in $results | Where-Object { $_.case -like "$domain/*" }) {
            $details=([string]$result.observation -replace '\|','/' -replace '[\r\n]+',' ')
            $failures=@($result.selfcheck.checks | Where-Object { -not $_.passed } | ForEach-Object name)
            $errors=@($result.operations | Where-Object status -eq 'error' | ForEach-Object { $_.operation+': '+$_.error })
            $status=$result.status
            if ($failures.Count) { $status+='; missing: '+($failures -join ', ') }
            if ($errors.Count) { $status+='; operation errors: '+($errors -join '; ') }
            if ($result.error) { $status+='; '+$result.error }
            $status=$status -replace '\|','/' -replace '[\r\n]+',' '
            $lines.Add("| $($result.case).docx | $($result.wordVersion) / Windows | _scripts/task-d.ps1 (COM) | $details | $status |")
        }
        if ($domain -eq 'ink') { $lines.Add('| ink/ink-to-shape-2.docx | - | Not attempted | Optional freehand UI task cannot be produced by this COM script. | Skipped; no fabricated substitute. |') }
        $lines | Set-Content -LiteralPath (Join-Path $dir 'OBSERVED.md') -Encoding UTF8
    }
}

$cases=@(
    'blank/blank-new','blank/blank-styles-used',
    'revisions2/rev-insert-delete','revisions2/rev-move','revisions2/rev-format','revisions2/rev-table','revisions2/rev-section','revisions2/rev-accept-reject','revisions2/rev-comment-threads',
    'fields2/fields-seq-captions','fields2/fields-index','fields2/fields-toc-stale','fields2/fields-citations','fields2/fields-page-in-footer',
    'sections2/sections-breaks-zoo','image2/image-z-order','shapes2/textbox-linked'
)
if ($Only.Count) { $cases=@($cases | Where-Object { $_ -in $Only -or ($_ -split '/')[-1] -in $Only }) }
if (-not $cases.Count) { throw 'No recognized task D cases selected.' }
foreach ($case in $cases) {
    if (Test-Path -LiteralPath (Join-Path $OutputDir "$case.docx")) { throw "Output already exists; use a fresh output directory to retain original Word output: $case.docx" }
}

$doc=$null; $settings=@{}; $ownedInstance=$false
try {
    if ($null -eq $Word) {
        $word=New-Object -ComObject Word.Application
        if ($word.Documents.Count -ne 0) { throw 'The new COM application has documents; refusing to control an instance containing existing documents.' }
        $ownedInstance=$true
        $word.Visible=$false
    }
    $settings.UserName=$word.UserName
    $settings.UserInitials=$word.UserInitials
    $settings.DisplayAlerts=$word.DisplayAlerts
    $settings.UpdateFieldsAtPrint=$word.Options.UpdateFieldsAtPrint
    $settings.UpdateLinksAtPrint=$word.Options.UpdateLinksAtPrint
    $settings.SaveNormalPrompt=$word.Options.SaveNormalPrompt
    $settings.NormalTemplateSaved=$word.NormalTemplate.Saved
    $word.DisplayAlerts=0
    $word.Options.UpdateFieldsAtPrint=$false
    $word.Options.UpdateLinksAtPrint=$false
    $word.Options.SaveNormalPrompt=$false
    foreach ($case in $cases) {
        $captionRestore=$null
        $entry=[ordered]@{case=$case;status='started';wordVersion=($word.Version+' / build '+$word.Build);started=(Get-Date).ToString('o');operations=[Collections.Generic.List[object]]::new();visualChecked=$false;observation='';selfcheck=$null}
        $results.Add($entry)
        try {
            $word.UserName=$authorA; $word.UserInitials='A'
            [string]$path=[string](Join-Path $OutputDir "$case.docx")
            New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
            $doc=$word.Documents.Add()
            $doc.SetCompatibilityMode(15)
            $doc.TrackRevisions=$false
            if ($case -notlike 'blank/*') { $doc.Content.Font.Size=11 }
            $null = switch (($case -split '/')[-1]) {
                'blank-new' { $entry.observation='A new empty Word document, no text entered.' }
                'blank-styles-used' {
                    $doc.Content.Text="Heading one`rHeading two`rBody text`rBullet item`rNumbered item`r"
                    $doc.Paragraphs.Item(1).Range.Style=-2
                    $doc.Paragraphs.Item(2).Range.Style=-3
                    $doc.Paragraphs.Item(3).Range.Style=-67
                    $doc.Paragraphs.Item(4).Range.Style=-180
                    $doc.Paragraphs.Item(4).Range.ListFormat.ApplyBulletDefault()
                    $doc.Paragraphs.Item(5).Range.Style=-180
                    $doc.Paragraphs.Item(5).Range.ListFormat.ApplyNumberDefault()
                    [void]$doc.Content.Delete()
                    $doc.Content.Style=-1
                    $doc.Content.ListFormat.RemoveNumbers()
                    $entry.observation='Heading 1, Heading 2, Body Text, bulleted and numbered List Paragraphs were used, then all content deleted.'
                }
                'rev-insert-delete' {
                    Set-Body $doc "Author A insertion point.`rDelete this original sentence.`rAuthor B insertion point."
                    $doc.TrackRevisions=$true
                    (Find-Text $doc 'Author A insertion point.').InsertAfter(' Author A inserted this sentence.')
                    [void](Find-Text $doc 'Delete this original sentence.').Delete()
                    $word.UserName=$authorB; $word.UserInitials='B'
                    (Find-Text $doc 'Author B insertion point.').InsertAfter(' Author B inserted this sentence.')
                    $entry.observation='Author A inserted one sentence and deleted an original sentence. Author B inserted a separate sentence; revisions remain enabled.'
                }
                'rev-move' {
                    Set-Body $doc "Move this entire original paragraph with several words to the end of the body.`rStationary paragraph one remains here.`rStationary paragraph two remains here."
                    $doc.TrackMoves=$true; $doc.TrackRevisions=$true
                    $moving=(Find-Text $doc 'Move this entire original paragraph with several words to the end of the body.').Paragraphs.Item(1).Range.Duplicate
                    $moving.Cut()
                    $target=Find-Text $doc $after; $target.Collapse(1); $target.Paste()
                    $entry.observation='With TrackMoves and TrackRevisions enabled, cut the whole original paragraph and pasted immediately before after. Native move markers are checked, never synthesized.'
                }
                'rev-format' {
                    Set-Body $doc "Run formatting target.`rParagraph alignment and indent target.`rNumbering target."
                    $doc.TrackFormatting=$true; $doc.TrackRevisions=$true
                    $range=Find-Text $doc 'Run formatting target.'; $range.Font.Bold=-1; $range.Font.Color=255
                    $range=Find-Text $doc 'Paragraph alignment and indent target.'; $range.ParagraphFormat.Alignment=1; $range.ParagraphFormat.LeftIndent=36
                    (Find-Text $doc 'Numbering target.').ListFormat.ApplyNumberDefault()
                    $entry.observation='Tracked bold/red text, centered paragraph with 36 pt left indent, and numbered paragraph.'
                }
                'rev-table' {
                    Set-Body $doc 'TABLE'
                    $range=Find-Text $doc 'TABLE'; $range.Text=''; $table=$doc.Tables.Add($range,2,2)
                    foreach ($r in 1,2) { foreach ($c in 1,2) { $table.Cell($r,$c).Range.Text="Original R$r C$c" } }
                    $table.Borders.Enable=1
                    $doc.TrackFormatting=$true; $doc.TrackRevisions=$true
                    Attempt 'insert row before original second row' { $inserted=$table.Rows.Add($table.Rows.Item(2)); $inserted.Cells.Item(1).Range.Text='Inserted row left'; $inserted.Cells.Item(2).Range.Text='Inserted row right' }
                    Attempt 'delete original second row' { (Find-Text $doc 'Original R2 C1').Rows.Item(1).Delete() }
                    Attempt 'merge original first-row cells' { $table.Cell(1,1).Merge($table.Cell(1,2)) }
                    Attempt 'change table border color to red' { foreach ($border in $table.Borders) { $border.Color=255 } }
                    $entry.observation='Started with 2x2; tracked insertion of a row, deletion of an original row, merge of first-row cells, red border change. Each operation and actual native revision representation is recorded.'
                }
                'rev-section' {
                    Set-Body $doc 'Section formatting target.'
                    $doc.TrackFormatting=$true; $doc.TrackRevisions=$true
                    $doc.Sections.Item(1).PageSetup.TopMargin=54
                    $doc.Sections.Item(1).PageSetup.LeftMargin=54
                    $doc.Sections.Item(1).PageSetup.Orientation=1
                    $entry.observation='Tracked 54 pt top/left margins and landscape orientation.'
                }
                'rev-accept-reject' {
                    Set-Body $doc "First original anchor.`rSecond original anchor.`rThird original anchor."
                    $doc.TrackRevisions=$true
                    (Find-Text $doc 'First original anchor.').InsertAfter(' ACCEPTED-INSERT')
                    (Find-Text $doc 'Second original anchor.').InsertAfter(' REJECTED-INSERT')
                    (Find-Text $doc 'Third original anchor.').InsertAfter(' PENDING-INSERT')
                    $doc.Revisions.Item(1).Accept()
                    $doc.Revisions.Item(1).Reject()
                    $entry.observation='Three independent insertion revisions; accepted first, rejected second, third remains pending.'
                }
                'rev-comment-threads' {
                    Set-Body $doc "First threaded anchor.`rSecond comment covers this whole paragraph.`rThird comment covers the word target."
                    $root=$doc.Comments.Add((Find-Text $doc 'First threaded anchor.'),'Root comment, resolved after two replies.')
                    $reply=$root.Replies.Add((Find-Text $doc 'First threaded anchor.'),'First reply.')
                    try { $reply.Replies.Add((Find-Text $doc 'First threaded anchor.'),'Second-level reply.') | Out-Null; $entry.replyMethod='reply.Replies.Add' }
                    catch { $entry.nestedReplyError=$_.Exception.Message; $root.Replies.Add((Find-Text $doc 'First threaded anchor.'),'Second reply to the thread.') | Out-Null; $entry.replyMethod='root.Replies.Add fallback; Word may flatten reply depth' }
                    $root.Done=$true
                    $doc.Comments.Add((Find-Text $doc 'Second comment covers this whole paragraph.').Paragraphs.Item(1).Range,'Whole paragraph comment.') | Out-Null
                    $doc.Comments.Add((Find-Text $doc 'target'),'One-word comment.') | Out-Null
                    $entry.observation='Three root comments: a resolved thread with two replies, a whole-paragraph comment, and a one-word comment. Reply API: '+$entry.replyMethod
                }
                'fields-seq-captions' {
                    Set-Body $doc "PICTURE-1`rPICTURE-2`rPICTURE-3`rCross-reference: TARGET-REF"
                    $addedLabel=$false
                    try { $label=$word.CaptionLabels.Item($figureLabel) } catch { $label=$word.CaptionLabels.Add($figureLabel); $addedLabel=$true }
                    $captionRestore=@{label=$label;added=$addedLabel;numberStyle=$label.NumberStyle;chapterNumber=$label.IncludeChapterNumber}
                    $label.NumberStyle=0; $label.IncludeChapterNumber=$false
                    foreach ($index in 1,2,3) {
                        $range=Find-Text $doc "PICTURE-$index"; $range.Text=''
                        $picture=$doc.InlineShapes.AddPicture($assetPath,$false,$true,$range)
                        $picture.Width=48; $picture.Height=24
                        $picture.Range.InsertCaption($figureLabel," sample $index",$missing,1,$false)
                    }
                    $range=Find-Text $doc 'TARGET-REF'; $range.Text=''
                    $range.InsertCrossReference($figureLabel,3,'2',$true)
                    [void]$doc.Fields.Update()
                    $entry.observation='Three 64x32 PNGs displayed at 48x24 pt; native captions use the requested Chinese figure label. Cross-reference points to the label and number of caption 2.'
                }
                'fields-index' {
                    Set-Body $doc "Alpha sample.`rBeta sample.`rGamma sample.`rINDEX-LOCATION"
                    foreach ($term in 'Alpha','Beta','Gamma') { $doc.Indexes.MarkEntry((Find-Text $doc $term),$term) | Out-Null }
                    $range=Find-Text $doc 'INDEX-LOCATION'; $range.Text=''
                    $index=$doc.Indexes.Add($range); $index.Update()
                    $entry.observation='Marked Alpha, Beta and Gamma as three index entries; inserted and updated an index before after.'
                }
                'fields-toc-stale' {
                    Set-Body $doc "TOC-LOCATION`rOriginal heading one`rFirst body text.`rOriginal heading two`rSecond body text.`rOriginal heading three`rThird body text."
                    (Find-Text $doc 'Original heading one').Style=-2
                    (Find-Text $doc 'Original heading two').Style=-3
                    (Find-Text $doc 'Original heading three').Style=-4
                    $range=Find-Text $doc 'TOC-LOCATION'; $range.Text=''
                    $toc=$doc.TablesOfContents.Add($range,$true,1,3); $toc.Update()
                    $entry.tocBefore=$toc.Range.Text
                    foreach ($heading in 'Original heading one','Original heading two') {
                        $range=Find-Text $doc $heading -Last
                        # Replacing only the prefix leaves the native TOC bookmark around the heading.
                        $range.End=$range.Start+'Original'.Length
                        $range.Text='Changed'
                    }
                    $entry.tocAfter=$toc.Range.Text
                    $entry.observation='TOC was updated once with original three-level headings. Only the Original prefix in body headings 1 and 2 was replaced with Changed, preserving native TOC bookmark ranges. TOC was deliberately not updated; bookmark targets and cached field errors are checked.'
                }
                'fields-citations' {
                    Set-Body $doc "Book citation: CITE-BOOK`rJournal citation: CITE-JOURNAL`rBIBLIOGRAPHY-LOCATION"
                    [string]$bookSourceXml=[string](New-BibliographySource 'Round2Book2026' 'Book' 'Corpus Book' 'BookAuthor' '2026')
                    [string]$journalSourceXml=[string](New-BibliographySource 'Round2Journal2025' 'JournalArticle' 'Corpus Journal Article' 'JournalAuthor' '2025')
                    $doc.Bibliography.Sources.Add($bookSourceXml) | Out-Null
                    $doc.Bibliography.Sources.Add($journalSourceXml) | Out-Null
                    $range=Find-Text $doc 'CITE-BOOK'; $doc.Fields.Add($range,96,'Round2Book2026',$false) | Out-Null
                    $range=Find-Text $doc 'CITE-JOURNAL'; $doc.Fields.Add($range,96,'Round2Journal2025',$false) | Out-Null
                    $range=Find-Text $doc 'BIBLIOGRAPHY-LOCATION'; $doc.Fields.Add($range,97,$missing,$false) | Out-Null
                    [void]$doc.Fields.Update()
                    $entry.observation='Two document-local bibliography sources, a book and journal article; two native CITATION fields and one BIBLIOGRAPHY field. Sources supplied through Word Bibliography.Sources.Add.'
                }
                'fields-page-in-footer' {
                    Set-Body $doc ("Page one body.`r"+[char]12+"Page two body.`r"+[char]12+'Page three body.')
                    $footer=$doc.Sections.Item(1).Footers.Item(1)
                    $footer.Range.Text=(Unicode-Text '7B2C')+' '
                    $range=$footer.Range.Duplicate; $range.End--; $range.Collapse(0); $doc.Fields.Add($range,33) | Out-Null
                    $range=$footer.Range.Duplicate; $range.End--; $range.Collapse(0); $range.InsertAfter(' '+(Unicode-Text '9875')+' / '+(Unicode-Text '5171')+' ')
                    $range=$footer.Range.Duplicate; $range.End--; $range.Collapse(0); $doc.Fields.Add($range,26) | Out-Null
                    $range=$footer.Range.Duplicate; $range.End--; $range.Collapse(0); $range.InsertAfter(' '+(Unicode-Text '9875'))
                    $doc.Repaginate(); [void]$footer.Range.Fields.Update()
                    $entry.observation='Three pages separated by two page breaks. Footer uses Chinese page labels with native PAGE and NUMPAGES fields.'
                }
                'sections-breaks-zoo' {
                    $doc.Content.Text="$before`rSection one.`r"
                    $breaks=@(2,3,4,5)
                    foreach ($index in 0,1,2,3) {
                        (End-Range $doc).InsertBreak($breaks[$index])
                        (End-Range $doc).InsertAfter("Section $($index+2).`r")
                    }
                    (End-Range $doc).InsertAfter("$after`r")
                    foreach ($section in $doc.Sections) { $section.PageSetup.DifferentFirstPageHeaderFooter=0 }
                    $third=$doc.Sections.Item(3); $third.PageSetup.DifferentFirstPageHeaderFooter=-1
                    $third.Headers.Item(2).LinkToPrevious=$false
                    $third.Headers.Item(2).Range.Text='Section three first-page header'
                    $entry.observation='Five sections with next-page, continuous, even-page, odd-page breaks in that order. Section 3 has Different First Page and an independent first-page header.'
                }
                'image-z-order' {
                    Set-Body $doc 'Floating images anchor.'
                    $anchor=Find-Text $doc 'Floating images anchor.'; $anchor.Collapse(1)
                    $pictures=@()
                    foreach ($index in 1,2,3) {
                        $shape=$doc.Shapes.AddPicture($assetPath,$false,$true,(40+25*$index),(15+15*$index),100,50,$anchor)
                        $shape.Name="Round2 Picture $index"
                        $shape.RelativeHorizontalPosition=0; $shape.RelativeVerticalPosition=2; $shape.WrapFormat.Type=3; $shape.LockAnchor=-1
                        $pictures+=,$shape
                    }
                    $pictures[0].ZOrder(0)
                    $pictures[0].ZOrder(3)
                    $pictures[2].ZOrder(0)
                    $anchor.ParagraphFormat.SpaceAfter=130
                    $entry.zOrder=@($pictures | ForEach-Object { @{name=$_.Name;position=$_.ZOrderPosition} })
                    $entry.observation='Three overlapping floating PNGs. Brought picture 1 forward to establish a meaningful starting stack, sent picture 1 backward one layer, then brought picture 3 to front. Final bottom-to-top: 2, 1, 3.'
                }
                'textbox-linked' {
                    Set-Body $doc 'Linked textboxes anchor.'
                    $anchor=Find-Text $doc 'Linked textboxes anchor.'; $anchor.Collapse(1)
                    $first=$doc.Shapes.AddTextbox(1,30,25,180,90,$anchor)
                    $second=$doc.Shapes.AddTextbox(1,250,25,180,90,$anchor)
                    foreach ($shape in @($first,$second)) { $shape.RelativeHorizontalPosition=0; $shape.RelativeVerticalPosition=2; $shape.WrapFormat.Type=3; $shape.LockAnchor=-1; $shape.TextFrame.AutoSize=0; $shape.TextFrame.TextRange.Font.Size=10; $shape.TextFrame.TextRange.ParagraphFormat.SpaceAfter=0 }
                    $first.Left=0; $second.Left=210
                    $first.Top=25; $second.Top=25
                    $first.TextFrame.Next=$second.TextFrame
                    $textLines=@(foreach ($number in 1..10) { 'Line {0:00}: linked story text.' -f $number })
                    $first.TextFrame.TextRange.Text=($textLines -join "`r")
                    $anchor.ParagraphFormat.SpaceAfter=150
                    $entry.observation='Two 180x90 pt textboxes, explicitly positioned 0/210 pt from the left margin and 25 pt below their anchor paragraph after setting relative coordinate modes. Linked through TextFrame.Next; ten lines entered in the first flow into the second. Native linked textbox markup is inspected.'
                    $entry.linkedStory=$first.TextFrame.ContainingRange.Text
                }
            }
            $doc.Repaginate()
            $entry.pages=[int]$doc.ComputeStatistics(2)
            $entry.paragraphs=[int]$doc.Paragraphs.Count
            $entry.revisions=[int]$doc.Revisions.Count
            $entry.comments=[int]$doc.Comments.Count
            $entry.compatibility=[int]$doc.CompatibilityMode
            $entry.text=$doc.Content.Text
            $doc.SaveAs2($path,12)
            $entry.path=$path
            $entry.sha256=Get-SharedFileSha256 $path
            $entry.selfcheck=Check-Package $case $path
            $requiredOperationErrors=@($entry.operations | Where-Object status -eq 'error')
            $entry.requiredOperationsPassed=$requiredOperationErrors.Count -eq 0
            if (-not $entry.requiredOperationsPassed) {
                $entry.selfcheck.passed=$false
                $entry.selfcheck.checks+=@{name='required Word operations succeeded';passed=$false;actual=$requiredOperationErrors}
            }
            $entry.status=if ($entry.selfcheck.passed) { 'saved; package selfcheck passed; visual review pending' } else { 'saved; package selfcheck incomplete; original retained' }
            if ($ExportPdf) {
                [string]$pdf=[string](Join-Path $OutputDir ('_previews/'+$case+'.pdf'))
                New-Item -ItemType Directory -Path (Split-Path -Parent $pdf) -Force | Out-Null
                Attempt 'export PDF after original save' { $doc.ExportAsFixedFormat($pdf,17,$false) }
                if (Test-Path -LiteralPath $pdf) { $entry.pdf=$pdf }
                if ($case -eq 'fields2/fields-toc-stale') {
                    $entry.tocAfterPdf=$toc.Range.Text
                    $errorPrefix=Unicode-Text '9519 8BEF'
                    $cacheOk=$entry.tocAfterPdf.Contains('Original heading one') -and $entry.tocAfterPdf.Contains('Original heading two') -and $entry.tocAfterPdf -notmatch '(?i)Error!\s*(Bookmark not defined|Reference source not found)' -and -not $entry.tocAfterPdf.Contains($errorPrefix)
                    $entry.selfcheck.checks+=@{name='TOC cache retains old labels without bookmark errors after PDF export';passed=$cacheOk;actual=$entry.tocAfterPdf}
                    if (-not $cacheOk) { $entry.selfcheck.passed=$false; $entry.status='saved; package selfcheck incomplete; original retained' }
                }
            }
            if ((Get-SharedFileSha256 $path) -ne $entry.sha256) { throw 'Original DOCX changed after its first save.' }
            if ($ReviewBarrier) {
                $doc.Activate()
                $doc.ActiveWindow.View.Type=3
                $doc.ActiveWindow.View.Zoom.Percentage=90
                $doc.Range(0,0).Select()
                $word.ScreenRefresh()
                $reviewPath=Join-Path $OutputDir '_control/task-d-review.json'
                $releasePath=Join-Path $OutputDir '_control/task-d-review.release'
                @{case=$case;document=$doc.Name;path=$path;sha256=$entry.sha256;selfcheckPassed=$entry.selfcheck.passed} | ConvertTo-Json | Set-Content -LiteralPath $reviewPath -Encoding UTF8
                $deadline=(Get-Date).AddMinutes(15)
                while (-not (Test-Path -LiteralPath $releasePath)) {
                    if ((Get-Date) -gt $deadline) { throw 'Visual review checkpoint timed out.' }
                    Start-Sleep -Milliseconds 250
                }
                Remove-Item -LiteralPath $releasePath
                Remove-Item -LiteralPath $reviewPath
                $entry.visualReviewCheckpoint='inspected before close; screenshots recorded separately'
            }
            Write-Output "$case : $($entry.status)"
        } catch {
            $entry.status='creation error; review retained trial if present'
            $entry.error=$_.Exception.Message
            $entry.trace=$_.ScriptStackTrace
            if ($null -ne $doc -and -not (Test-Path -LiteralPath $path)) {
                try { $doc.SaveAs2($path,12); $entry.path=$path; $entry.selfcheck=Check-Package $case $path } catch { $entry.trialSaveError=$_.Exception.Message }
            }
            Write-Warning "$case : $($entry.error)"
        } finally {
            if ($null -ne $captionRestore) {
                try {
                    if ($captionRestore.added) { $captionRestore.label.Delete() }
                    else { $captionRestore.label.NumberStyle=$captionRestore.numberStyle; $captionRestore.label.IncludeChapterNumber=$captionRestore.chapterNumber }
                } catch { $entry.captionRestoreError=$_.Exception.Message }
                $captionRestore=$null
            }
            if ($null -ne $doc) { try { $doc.Close(0) } catch { $entry.closeError=$_.Exception.Message }; $doc=$null }
            $entry.finished=(Get-Date).ToString('o')
            Write-Reports
        }
    }
} finally {
    if ($null -ne $word) {
        if ($null -ne $doc) { try { $doc.Close(0) } catch {} }
        foreach ($name in 'UserName','UserInitials','DisplayAlerts') { if ($settings.ContainsKey($name)) { try { $word.$name=$settings[$name] } catch { Write-Warning "Could not restore $name : $_" } } }
        foreach ($name in 'UpdateFieldsAtPrint','UpdateLinksAtPrint','SaveNormalPrompt') { if ($settings.ContainsKey($name)) { try { $word.Options.$name=$settings[$name] } catch { Write-Warning "Could not restore $name : $_" } } }
        if ($settings.ContainsKey('NormalTemplateSaved')) { try { $word.NormalTemplate.Saved=$settings.NormalTemplateSaved } catch {} }
        if ($ownedInstance) { try { $word.Quit(0) } catch { Write-Warning $_.Exception.Message } }
    }
    if ($null -ne $word -and $ownedInstance) {
        [void][Runtime.InteropServices.Marshal]::ReleaseComObject($word)
        [GC]::Collect(); [GC]::WaitForPendingFinalizers()
    }
}
Write-Output "Task D results: $resultPath"
