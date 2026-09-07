param(
    [Parameter(Mandatory=$true)][object]$Word,
    [string]$OutputDir = (Split-Path -Parent $PSScriptRoot),
    [string]$TrialName = 'table-style-probe'
)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$helperPath=Join-Path $PSScriptRoot 'task-d.ps1'
$tokens=$null; $parseErrors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($helperPath,[ref]$tokens,[ref]$parseErrors)
if ($parseErrors.Count) { throw 'Could not parse task-d.ps1 helpers.' }
foreach ($name in 'Unicode-Text','Find-Text','Set-Body','Attempt','Get-SharedFileSha256','Read-Package') {
    $function=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true)
    if ($null -eq $function) { throw "Missing helper: $name" }
    Invoke-Expression $function.Extent.Text
}
[string]$before='before '+(Unicode-Text '524D 6587')
[string]$after='after '+(Unicode-Text '540E 6587')
[string]$path=[string](Join-Path $OutputDir ("_trials/$TrialName/rev-table.docx"))
[string]$pdf=[string](Join-Path $OutputDir ("_previews/trials/$TrialName.pdf"))
[string]$resultPath=[string](Join-Path $OutputDir ("_trials/$TrialName/result.json"))
if (Test-Path -LiteralPath $path) { throw "Refusing to overwrite $path" }
foreach ($directory in @((Split-Path -Parent $path),(Split-Path -Parent $pdf))) { New-Item -ItemType Directory -Path $directory -Force | Out-Null }
$entry=[ordered]@{
    case='revisions2/rev-table'
    status='not saved'
    method='_scripts/task-d-table-trial.ps1 (desktop Word COM)'
    wordVersion=[string]$Word.Version
    wordBuild=[string]$Word.Build
    operations=[Collections.Generic.List[object]]::new()
    started=(Get-Date).ToString('o')
    observation='Native 2x2 table. Black and red table styles differ only in border color. With TrackRevisions and TrackFormatting enabled, inserted a row, deleted an original row, merged first-row cells, and applied the red border table style. Original saved once; PDF exported afterward; document remains open for visual inspection.'
}
$doc=$null
$originalAlerts=$Word.DisplayAlerts
try {
    $Word.DisplayAlerts=0
    $doc=$Word.Documents.Add()
    Set-Body $doc 'TABLE'
    $range=Find-Text $doc 'TABLE'; $range.Text=''
    $table=$doc.Tables.Add($range,2,2)
    foreach ($row in 1,2) { foreach ($column in 1,2) { $table.Cell($row,$column).Range.Text="Original R$row C$column" } }
    foreach ($spec in @(@('Black',0),@('Red',255))) {
        [string]$styleName='Round2 Table Border '+[string]$spec[0]
        $style=$doc.Styles.Add($styleName,3)
        $style.BaseStyle=-106
        foreach ($borderIndex in -1,-2,-3,-4,-5,-6) {
            $border=$style.Table.Borders.Item($borderIndex)
            $border.LineStyle=1
            $border.LineWidth=4
            $border.Color=[int]$spec[1]
        }
    }
    [string]$blackStyleName='Round2 Table Border Black'
    [string]$redStyleName='Round2 Table Border Red'
    $table.Style=$blackStyleName
    $entry.initialBorderColors=@(foreach ($borderIndex in -1,-2,-3,-4,-5,-6) { [int]$table.Borders.Item($borderIndex).Color })
    $doc.TrackFormatting=$true
    $doc.TrackRevisions=$true
    Attempt 'insert row before original second row' {
        $inserted=$table.Rows.Add($table.Rows.Item(2))
        $inserted.Cells.Item(1).Range.Text='Inserted row left'
        $inserted.Cells.Item(2).Range.Text='Inserted row right'
    }
    Attempt 'delete original second row' { (Find-Text $doc 'Original R2 C1').Rows.Item(1).Delete() }
    Attempt 'merge original first-row cells' { $table.Cell(1,1).Merge($table.Cell(1,2)) }
    Attempt 'change table border color to red through native table style' { $table.Style=$redStyleName }
    $entry.finalBorderColors=@(foreach ($borderIndex in -1,-2,-3,-4,-5,-6) { [int]$table.Borders.Item($borderIndex).Color })
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
    $parts=Read-Package $path
    $xml=$parts['word/document.xml']
    $ns=[Xml.XmlNamespaceManager]::new($xml.NameTable)
    $wordNamespace='http://schemas.openxmlformats.org/wordprocessingml/2006/main'
    $ns.AddNamespace('w',$wordNamespace)
    $counts=[ordered]@{
        rowInsert=$xml.SelectNodes('//w:trPr/w:ins',$ns).Count
        rowDelete=$xml.SelectNodes('//w:trPr/w:del',$ns).Count
        tcPrChange=$xml.SelectNodes('//w:tcPrChange',$ns).Count
        tblPrChange=$xml.SelectNodes('//w:tblPrChange',$ns).Count
        gridSpan2=$xml.SelectNodes('//w:gridSpan[@w:val="2"]',$ns).Count
    }
    $checks=[Collections.Generic.List[object]]::new()
    foreach ($name in $counts.Keys) { $checks.Add([ordered]@{name=$name;passed=$counts[$name] -ge 1;actual=$counts[$name]}) }
    $allText=-join @($xml.SelectNodes('//w:t',$ns) | ForEach-Object InnerText)
    $checks.Add([ordered]@{name='before/after markers';passed=$allText.Contains($before) -and $allText.Contains($after);actual=$allText})
    $currentProperties=$xml.SelectSingleNode('//w:tbl/w:tblPr',$ns)
    $styleId=$currentProperties.SelectSingleNode('w:tblStyle',$ns).GetAttribute('val',$wordNamespace)
    $styles=$parts['word/styles.xml']
    $appliedStyle=@($styles.SelectNodes('//w:style',$ns) | Where-Object { $_.GetAttribute('styleId',$wordNamespace) -eq $styleId })
    if ($appliedStyle.Count -ne 1) { throw "Expected one applied table style: $styleId" }
    $effectiveBorders=@(foreach ($side in 'top','left','bottom','right','insideH','insideV') {
        $borderNode=$currentProperties.SelectSingleNode("w:tblBorders/w:$side",$ns)
        $source='direct'
        if ($null -eq $borderNode) { $borderNode=$appliedStyle[0].SelectSingleNode("w:tblPr/w:tblBorders/w:$side",$ns); $source='style' }
        [ordered]@{side=$side;source=$source;color=$(if($borderNode){$borderNode.GetAttribute('color',$wordNamespace)}else{'missing'});value=$(if($borderNode){$borderNode.GetAttribute('val',$wordNamespace)}else{'missing'})}
    })
    $checks.Add([ordered]@{name='six effective table borders are red';passed=@($effectiveBorders | Where-Object { $_.color -ine 'FF0000' -or $_.value -in 'nil','none','missing' }).Count -eq 0;actual=$effectiveBorders})
    $checks.Add([ordered]@{name='Word border colors black before and red after';passed=@($entry.initialBorderColors | Where-Object {$_ -ne 0}).Count -eq 0 -and @($entry.finalBorderColors | Where-Object {$_ -ne 255}).Count -eq 0;actual=@{before=$entry.initialBorderColors;after=$entry.finalBorderColors}})
    $operationErrors=@($entry.operations | Where-Object status -eq 'error')
    $entry.requiredOperationsPassed=$operationErrors.Count -eq 0
    $checks.Add([ordered]@{name='required Word operations succeeded';passed=$entry.requiredOperationsPassed;actual=$operationErrors})
    $entry.selfcheck=[ordered]@{passed=@($checks | Where-Object { -not $_.passed }).Count -eq 0;checks=@($checks.ToArray());counts=$counts;fieldCodes=@();appliedStyleId=$styleId;tableProperties=$currentProperties.OuterXml}
    Attempt 'export PDF after original save' { $doc.ExportAsFixedFormat($pdf,17,$false) }
    if (Test-Path -LiteralPath $pdf) { $entry.pdf=$pdf }
    if ((Get-SharedFileSha256 $path) -ne $entry.sha256) { throw 'Original DOCX changed after its first save.' }
    $entry.status=if($entry.selfcheck.passed){'saved; package selfcheck passed; visual review pending'}else{'saved; package selfcheck incomplete; trial retained'}
    $doc.Activate()
    $doc.ActiveWindow.View.Type=3
    $doc.ActiveWindow.View.Zoom.Percentage=90
    $doc.Range(0,0).Select()
    $Word.ScreenRefresh()
} catch {
    $entry.status='trial error; retained saved file and active document if present'
    $entry.error=$_.Exception.Message
    $entry.trace=$_.ScriptStackTrace
} finally {
    $Word.DisplayAlerts=$originalAlerts
    $entry.finished=(Get-Date).ToString('o')
    $entry.leftOpen=$null -ne $doc
    $entry | ConvertTo-Json -Depth 18 | Set-Content -LiteralPath $resultPath -Encoding UTF8
}
$entry | ConvertTo-Json -Depth 18
