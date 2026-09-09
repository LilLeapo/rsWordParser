#requires -Version 7.0
[CmdletBinding()]
param([string]$OutputRoot = (Split-Path -Parent $PSScriptRoot))
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$checkRoot = Join-Path $OutputRoot ('_checks/smartart-math/' + (Get-Date -Format 'yyyyMMdd-HHmmss-fff'))
[void][IO.Directory]::CreateDirectory($checkRoot)
$results = [Collections.Generic.List[object]]::new()

function Read-PackageXml($Archive, [string]$Name) {
    $entry = $Archive.GetEntry($Name)
    if ($null -eq $entry) { throw "Missing package entry: $Name" }
    $reader = [IO.StreamReader]::new($entry.Open())
    try {
        $xml = [Xml.XmlDocument]::new()
        $xml.PreserveWhitespace = $true
        $xml.LoadXml($reader.ReadToEnd())
        return ,$xml
    }
    finally { $reader.Dispose() }
}

function New-Namespaces($Xml) {
    $ns = [Xml.XmlNamespaceManager]::new($Xml.NameTable)
    $ns.AddNamespace('w', 'http://schemas.openxmlformats.org/wordprocessingml/2006/main')
    $ns.AddNamespace('m', 'http://schemas.openxmlformats.org/officeDocument/2006/math')
    $ns.AddNamespace('a', 'http://schemas.openxmlformats.org/drawingml/2006/main')
    $ns.AddNamespace('dgm', 'http://schemas.openxmlformats.org/drawingml/2006/diagram')
    $ns.AddNamespace('wp', 'http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing')
    $ns.AddNamespace('rel', 'http://schemas.openxmlformats.org/package/2006/relationships')
    return ,$ns
}

function Require-XPath($Xml, $Namespaces, [string]$XPath, [int]$Minimum = 1) {
    $count = $Xml.SelectNodes($XPath, $Namespaces).Count
    if ($count -lt $Minimum) { throw "Expected at least $Minimum match(es) for $XPath; found $count." }
    return $count
}

foreach ($domain in @('smartart', 'math')) {
    foreach ($file in @(Get-ChildItem -LiteralPath (Join-Path $OutputRoot $domain) -Filter '*.docx' -File)) {
        if ($file.Name.StartsWith('~$')) { continue }
        $record = [ordered]@{ file = "$domain/$($file.Name)"; status = 'failed'; errors = @(); checks = [ordered]@{} }
        $archive = $null
        try {
            $hash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
            $copyFolder = Join-Path $checkRoot $domain
            [void][IO.Directory]::CreateDirectory($copyFolder)
            $copyPath = Join-Path $copyFolder ($file.BaseName + '.zip')
            Copy-Item -LiteralPath $file.FullName -Destination $copyPath
            $record['copy'] = [IO.Path]::GetRelativePath($OutputRoot, $copyPath).Replace('\', '/')
            $record['originalSha256'] = $hash
            $archive = [IO.Compression.ZipFile]::OpenRead($copyPath)
            $document = Read-PackageXml $archive 'word/document.xml'
            $ns = New-Namespaces $document
            $paragraphText = @($document.SelectNodes('/w:document/w:body/w:p', $ns) | ForEach-Object {
                ($_.SelectNodes('.//w:t', $ns) | ForEach-Object InnerText) -join ''
            })
            if (@($paragraphText | Where-Object { $_ -ceq 'before 前文' }).Count -ne 1 -or @($paragraphText | Where-Object { $_ -ceq 'after 后文' }).Count -ne 1) {
                throw 'Standalone before/after marker paragraphs are missing or duplicated.'
            }
            $record.checks['markerParagraphs'] = 'passed'
            $stem = $file.BaseName -replace '-\d+$', ''
            if ($domain -eq 'smartart') {
                $requiredParts = @('data1.xml', 'layout1.xml', 'quickStyle1.xml', 'colors1.xml', 'drawing1.xml')
                foreach ($part in $requiredParts) {
                    if ($null -eq $archive.GetEntry("word/diagrams/$part")) { throw "Missing SmartArt part: word/diagrams/$part" }
                }
                $record.checks['fiveRequiredParts'] = $requiredParts
                $record.checks['relIds'] = Require-XPath $document $ns '//dgm:relIds[@*]' 1
                $data = Read-PackageXml $archive 'word/diagrams/data1.xml'
                $dataNs = New-Namespaces $data
                $drawing = Read-PackageXml $archive 'word/diagrams/drawing1.xml'
                $drawingNs = New-Namespaces $drawing
                $record.checks['dataText'] = @($data.SelectNodes('//a:p[a:r/a:t]', $dataNs) | ForEach-Object { ($_.SelectNodes('.//a:t', $dataNs) | ForEach-Object InnerText) -join '' })
                $record.checks['drawingText'] = @($drawing.SelectNodes('//a:p[a:r/a:t]', $drawingNs) | ForEach-Object { ($_.SelectNodes('.//a:t', $drawingNs) | ForEach-Object InnerText) -join '' })
                $expected = switch ($stem) {
                    'smartart-list' { @('第一项', '第二项', '第三项', '第四项', '第五项') }
                    'smartart-hierarchy' { @('总经理', '部门A', '部门B', '组1', '助理') }
                    'smartart-cycle' { @('计划', '实施', '检查') }
                    'smartart-picture' { @('图片一', '图片二') }
                    'smartart-edited-text' { @('准备', '执行（已修改）', '完成') }
                    default { @('准备', '执行', '完成') }
                }
                foreach ($text in $expected) {
                    if ($text -notin $record.checks['dataText'] -or $text -notin $record.checks['drawingText']) { throw "SmartArt text absent from data or drawing part: $text" }
                }
                switch ($stem) {
                    'smartart-hierarchy' { $record.checks['connections'] = Require-XPath $data $dataNs '//dgm:cxn' 4 }
                    'smartart-picture' {
                        $record.checks['pictureFills'] = Require-XPath $drawing $drawingNs '//a:blipFill/a:blip' 2
                        $rels = Read-PackageXml $archive 'word/diagrams/_rels/drawing1.xml.rels'
                        $relsNs = New-Namespaces $rels
                        $record.checks['drawingImageRelationships'] = Require-XPath $rels $relsNs '//rel:Relationship[contains(@Type,"/image")]' 1
                    }
                    'smartart-floating' { $record.checks['sameParagraphAnchors'] = Require-XPath $document $ns '//w:p[.//dgm:relIds and count(.//wp:anchor)>=2]' 1 }
                    'smartart-in-table' { $record.checks['tableDiagram'] = Require-XPath $document $ns '//w:tbl//dgm:relIds' 1 }
                }
            }
            else {
                $record.checks['oMath'] = Require-XPath $document $ns '//m:oMath' 1
                $record.checks['mathText'] = @($document.SelectNodes('//m:t', $ns) | ForEach-Object InnerText)
                switch ($stem) {
                    'math-fraction' {
                        $record.checks['fraction'] = Require-XPath $document $ns '//m:f' 1
                        $record.checks['superscript'] = Require-XPath $document $ns '//m:sSup' 1
                    }
                    'math-integral' { $record.checks['nary'] = Require-XPath $document $ns '//m:nary[not(m:naryPr/m:chr) or m:naryPr/m:chr[@m:val="∫"]]' 1 }
                    'math-matrix' { $record.checks['bracketed2x2Matrix'] = Require-XPath $document $ns '//m:d[m:dPr/m:begChr[@m:val="["] and m:dPr/m:endChr[@m:val="]"]]/m:e/m:m[count(m:mr)=2 and count(m:mr[1]/m:e)=2 and count(m:mr[2]/m:e)=2]' 1 }
                    'math-inline' { $record.checks['inlineWithText'] = Require-XPath $document $ns '//w:p[m:oMath and w:r/w:t[contains(.,"质能方程")]]' 1 }
                    'math-display-two' { $record.checks['oneParagraphTwoEquations'] = Require-XPath $document $ns '//m:oMathPara[count(m:oMath)=2]' 1 }
                    'math-builtin' {
                        $record.checks['threeParagraphEquations'] = Require-XPath $document $ns '//m:oMathPara' 3
                        $record.checks['radical'] = Require-XPath $document $ns '//m:rad' 1
                        $record.checks['summation'] = Require-XPath $document $ns '//m:nary' 1
                    }
                    'math-latex' {
                        $record.checks['fraction'] = Require-XPath $document $ns '//m:f' 1
                        $record.checks['summation'] = Require-XPath $document $ns '//m:nary' 1
                        $creation = Get-Content -LiteralPath (Join-Path $OutputRoot '_scripts/smartart-math-results.json') -Raw -Encoding utf8 | ConvertFrom-Json
                        $evidence = @($creation.cases | Where-Object { $_.file -eq "math/$($file.Name)" -and $_.savedOnce })[-1]
                        if ($evidence.details.inputModeCommand -ne 'EquationLaTexFormat' -or -not $evidence.details.inputModeReadBack) { throw 'Missing successful LaTeX Ribbon mode command/read-back evidence.' }
                        $record.checks['latexModeCommandAndReadBack'] = 'passed'
                    }
                    'math-linear' {
                        if ($document.SelectNodes('//m:oMath/*[not(self::m:r)]', $ns).Count -gt 0) { throw 'Linear equation still contains structured OMath children.' }
                        $record.checks['onlyMathRuns'] = 'passed'
                    }
                    'math-styled' {
                        $record.checks['coloredRun'] = Require-XPath $document $ns '//m:r/w:rPr/w:color[@w:val="FF0000"]' 1
                        $record.checks['twentyPointRun'] = Require-XPath $document $ns '//m:r/w:rPr/w:sz[@w:val="40"]' 1
                        $record.checks['centeredMath'] = Require-XPath $document $ns '//m:oMathParaPr/m:jc[@m:val="center"]' 1
                        $record.checks['fontSizesHalfPoints'] = @($document.SelectNodes('//m:r/w:rPr/w:sz/@w:val', $ns) | ForEach-Object Value | Sort-Object -Unique)
                        if ($record.checks['fontSizesHalfPoints'].Count -lt 2) { $record['specificationDeviation'] = 'Word applied the requested 20-point size to the entire equation; partial red color is present, but mixed font sizes are not.' }
                    }
                    'math-in-table' { $record.checks['tableFormula'] = Require-XPath $document $ns '//w:tbl//m:oMath//m:f' 1 }
                }
            }
            if ((Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash -ne $hash) { throw 'The original file changed during copied-package verification.' }
            $record['originalUnchanged'] = $true
            $record['status'] = 'passed'
        }
        catch { $record['errors'] = @($_.Exception.Message) }
        finally { if ($null -ne $archive) { $archive.Dispose() } }
        $results.Add([pscustomobject]$record)
    }
}
$results | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $OutputRoot '_scripts/smartart-math-selfcheck.json') -Encoding utf8
$results | Select-Object file,status,@{Name='error'; Expression={ $_.errors -join '; ' }} | Format-Table -AutoSize
