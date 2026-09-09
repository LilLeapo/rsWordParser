#requires -Version 7.0
[CmdletBinding()]
param(
    [string]$OutputRoot = (Split-Path -Parent $PSScriptRoot),
    [string[]]$Only = @(),
    [bool]$ShowWord = $true,
    [switch]$ExportPdf,
    [switch]$VerifyOnly
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$OutputRoot = [IO.Path]::GetFullPath($OutputRoot)
$script:Word = $null
$script:WordOwned = $false
$script:WordProcessId = $null
$script:Document = $null
$script:Details = [ordered]@{}
$script:SourceWorkbook = $null
$script:SourceWorkbookValues = $null
$runId = [DateTimeOffset]::Now.ToString('o')
$rows = [Collections.Generic.List[object]]::new()
$logPath = Join-Path $OutputRoot '_scripts/ole-results.json'
foreach ($directory in @('_scripts', '_assets', 'ole', '_previews/ole', '_checks/ole')) {
    [void][IO.Directory]::CreateDirectory((Join-Path $OutputRoot $directory))
}
if (Test-Path -LiteralPath $logPath) {
    $previous = Get-Content -LiteralPath $logPath -Raw -Encoding utf8 | ConvertFrom-Json
    foreach ($row in $previous.cases) { $rows.Add($row) }
}
Add-Type -AssemblyName System.IO.Compression.FileSystem
if (-not ('CorpusOleNative' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CorpusOleNative {
    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
}
'@
}

function Get-UnusedPath([string]$RelativePath) {
    $path = Join-Path $OutputRoot $RelativePath
    if (-not (Test-Path -LiteralPath $path)) { return $path }
    $directory = [IO.Path]::GetDirectoryName($path)
    $stem = [IO.Path]::GetFileNameWithoutExtension($path)
    $extension = [IO.Path]::GetExtension($path)
    for ($suffix = 2; ; $suffix++) {
        $path = Join-Path $directory "$stem-$suffix$extension"
        if (-not (Test-Path -LiteralPath $path)) { return $path }
    }
}

function Get-ProcessIdFromWindow([long]$WindowHandle) {
    $owner = [uint32]0
    [void][CorpusOleNative]::GetWindowThreadProcessId([IntPtr]$WindowHandle, [ref]$owner)
    return [int]$owner
}

function Initialize-WordInstance {
    $script:WordOwned = $false
    $script:WordProcessId = $null
    $existing = @(Get-Process -Name WINWORD -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)
    $script:Word = New-Object -ComObject Word.Application
    $probe = $script:Word.Documents.Add()
    try {
        $owner = Get-ProcessIdFromWindow $probe.Windows.Item(1).Hwnd
        if ($owner -eq 0 -or $owner -in $existing) { throw "Word COM instance uses pre-existing or unknown PID $owner." }
        $script:WordOwned = $true
        $script:WordProcessId = $owner
        $script:Word.Visible = $ShowWord
        $script:Word.DisplayAlerts = 0
        $script:Word.AutomationSecurity = 3
        $script:Word.Options.SaveNormalPrompt = $false
    }
    finally { $probe.Close(0); [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($probe) }
}

function Get-SourceWorkbook {
    if ($null -ne $script:SourceWorkbook) { return $script:SourceWorkbook }
    $source = Get-UnusedPath '_assets/ole-excel-source.xlsx'
    $existing = @(Get-Process -Name EXCEL -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)
    $excel = New-Object -ComObject Excel.Application
    $owned = $false
    $book = $null
    try {
        $owner = Get-ProcessIdFromWindow $excel.Hwnd
        if ($owner -eq 0 -or $owner -in $existing) { throw "Excel source writer uses pre-existing or unknown PID $owner." }
        $owned = $true
        $excel.Visible = $false
        $excel.DisplayAlerts = $false
        $book = $excel.Workbooks.Add()
        while ($book.Worksheets.Count -gt 1) { $book.Worksheets.Item($book.Worksheets.Count).Delete() }
        $sheet = $book.Worksheets.Item(1)
        $sheet.Name = '示例'
        $sheet.Cells.Item(1, 1).Value2 = 12
        $sheet.Cells.Item(1, 2).Value2 = 34
        $sheet.Range('A1:B1').Font.Size = 14
        $sheet.Range('A:B').ColumnWidth = 10
        $sheet.PageSetup.PrintArea = '$A$1:$B$1'
        $book.SaveAs($source, 51)
        $script:SourceWorkbookValues = [ordered]@{ A1 = [double]$sheet.Cells.Item(1, 1).Value2; B1 = [double]$sheet.Cells.Item(1, 2).Value2 }
        $script:SourceWorkbook = $source
    }
    finally {
        if ($null -ne $book) { $book.Close($false); [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($book) }
        if ($owned) { $excel.Quit() }
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($excel)
    }
    return $source
}

function Get-FeatureRange {
    $range = $script:Document.Paragraphs.Item(2).Range.Duplicate
    $range.Collapse(1)
    return ,$range
}

function Add-Ole($Range, [string]$ClassType = 'Excel.Sheet.12', [string]$FileName = '', [bool]$Link = $false, [bool]$Icon = $false) {
    $missing = [Type]::Missing
    $class = if ($FileName.Length -gt 0) { $missing } else { $ClassType }
    $file = if ($FileName.Length -gt 0) { $FileName } else { $missing }
    $inline = $script:Document.InlineShapes.AddOLEObject($class, $file, $Link, $Icon, $missing, $missing, $missing, $Range)
    $script:Details['progId'] = [string]$inline.OLEFormat.ProgID
    $script:Details['displayAsIcon'] = [bool]$inline.OLEFormat.DisplayAsIcon
    $script:Details['linked'] = $Link
    if ($FileName.Length -gt 0) { $script:Details['sourceWorkbook'] = [IO.Path]::GetRelativePath($OutputRoot, $FileName).Replace('\', '/') }
    return ,$inline
}

function Open-Ole($Inline) {
    try {
        $Inline.OLEFormat.DoVerb(0)
        $object = $Inline.OLEFormat.Object
        $script:Details['doVerb0'] = 'succeeded'
        $script:Details['oleObjectDispatchType'] = if ($null -eq $object) { 'not exposed' } else { $object.GetType().FullName }
        return ,$object
    }
    catch {
        $script:Details['doVerb0'] = 'failed'
        $script:Details['doVerb0Error'] = $_.Exception.ToString()
        throw
    }
}

function Hide-Ole($Inline) {
    try { $Inline.OLEFormat.DoVerb(-3); $script:Details['hideVerb'] = 'succeeded' }
    catch {
        $script:Details['hideVerb'] = 'unsupported; document activated'
        $script:Details['hideVerbError'] = $_.Exception.Message
        $script:Document.Activate()
        $script:Document.Range(0, 0).Select()
    }
}

function Add-ExcelObject($Range) {
    $inline = Add-Ole $Range
    $book = Open-Ole $inline
    $sheet = $book.Worksheets.Item(1)
    $sheet.Cells.Item(1, 1).Value2 = 12
    $sheet.Cells.Item(1, 2).Value2 = 34
    $sheet.Range('A1:B1').Font.Size = 14
    $sheet.Range('A:B').ColumnWidth = 10
    $script:Details['cells'] = [ordered]@{ A1 = [double]$sheet.Cells.Item(1, 1).Value2; B1 = [double]$sheet.Cells.Item(1, 2).Value2 }
    Hide-Ole $inline
    $inline.Width = 240
    $inline.Height = 140
    return ,$inline
}

function Test-CopiedPackage([string]$Path, [string]$Stem) {
    $hash = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
    $copy = Get-UnusedPath ('_checks/ole/' + [IO.Path]::GetFileNameWithoutExtension($Path) + '.zip')
    Copy-Item -LiteralPath $Path -Destination $copy
    $archive = [IO.Compression.ZipFile]::OpenRead($copy)
    try {
        $entry = $archive.GetEntry('word/document.xml')
        $reader = [IO.StreamReader]::new($entry.Open())
        try { [xml]$xml = $reader.ReadToEnd() } finally { $reader.Dispose() }
        $ns = [Xml.XmlNamespaceManager]::new($xml.NameTable)
        $ns.AddNamespace('w', 'http://schemas.openxmlformats.org/wordprocessingml/2006/main')
        $ns.AddNamespace('v', 'urn:schemas-microsoft-com:vml')
        $ns.AddNamespace('o', 'urn:schemas-microsoft-com:office:office')
        $objects = $xml.SelectNodes('//w:object[v:shape and o:OLEObject]', $ns)
        if ($objects.Count -ne 1) { throw "Expected exactly one w:object with preview shape and OLEObject; got $($objects.Count)." }
        $ole = $objects[0].SelectSingleNode('o:OLEObject', $ns)
        $previews = @($archive.Entries | Where-Object FullName -match '^word/media/.*\.(emf|wmf)$' | Select-Object -ExpandProperty FullName)
        if ($previews.Count -eq 0) { throw 'Missing EMF/WMF OLE preview; this case does not satisfy the required preview.' }
        if ($objects[0].SelectNodes('v:shape/v:imagedata', $ns).Count -eq 0) { throw 'OLE shape does not reference its preview image.' }
        $embeddings = @($archive.Entries | Where-Object FullName -match '^word/embeddings/' | Select-Object -ExpandProperty FullName)
        if ($Stem -eq 'ole-excel-linked') {
            if ($ole.GetAttribute('Type') -ne 'Link') { throw 'Expected linked OLE Type=Link.' }
        }
        elseif ($ole.GetAttribute('Type') -ne 'Embed' -or $embeddings.Count -eq 0) { throw 'Expected an embedded OLE object and embedded package.' }
        if ($Stem -eq 'ole-icon' -and $ole.GetAttribute('DrawAspect') -ne 'Icon') { throw 'Expected the icon OLE draw aspect.' }
        if ($Stem -eq 'ole-in-table' -and $xml.SelectNodes('//w:tbl//w:object', $ns).Count -ne 1) { throw 'OLE object is not inside the table.' }
        if ($Stem -eq 'ole-with-text' -and $xml.SelectNodes('//w:p[w:r/w:t[contains(.,"末尾")]]//w:object', $ns).Count -ne 1) { throw 'OLE object does not share the requested text paragraph.' }
        $bodyText = @($xml.SelectNodes('/w:document/w:body/w:p', $ns) | ForEach-Object { ($_.SelectNodes('.//w:t', $ns) | ForEach-Object InnerText) -join '' })
        if ('before 前文' -notin $bodyText -or 'after 后文' -notin $bodyText) { throw 'Marker paragraphs missing from DOCX.' }
        if ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ne $hash) { throw 'Original DOCX changed during verification.' }
        return [ordered]@{ status = 'passed'; copiedZip = [IO.Path]::GetRelativePath($OutputRoot, $copy).Replace('\', '/'); originalSha256 = $hash; originalUnchanged = $true; progId = $ole.GetAttribute('ProgID'); type = $ole.GetAttribute('Type'); drawAspect = $ole.GetAttribute('DrawAspect'); previewParts = $previews; embeddingParts = $embeddings }
    }
    finally { $archive.Dispose() }
}

$cases = @(
    @{ Name = 'ole-excel-embedded'; Steps = '新建Excel.Sheet.12嵌入对象，A1=12、B1=34，退出编辑后保存'; Action = {
        [void](Add-ExcelObject (Get-FeatureRange))
        $script:Details['previewExpectation'] = 'Excel worksheet content containing 12 and 34'
    } },
    @{ Name = 'ole-excel-linked'; Steps = '由真实Excel生成的本地xlsx创建链接OLE对象'; Action = {
        $inline = Add-Ole (Get-FeatureRange) -FileName (Get-SourceWorkbook) -Link $true
        [void](Open-Ole $inline)
        $script:Details['sourceCells'] = $script:SourceWorkbookValues
        $script:Details['cellEvidence'] = 'Read back from the actual Excel source workbook immediately after its save; linked OLE dispatch does not expose worksheet cells.'
        Hide-Ole $inline
        $script:Details['previewExpectation'] = 'Linked Excel worksheet content containing 12 and 34'
    } },
    @{ Name = 'ole-icon'; Steps = '由真实Excel生成的xlsx插入嵌入OLE，并显示为图标'; Action = {
        $inline = Add-Ole (Get-FeatureRange) -FileName (Get-SourceWorkbook) -Icon $true
        [void](Open-Ole $inline)
        Hide-Ole $inline
        $script:Details['previewExpectation'] = 'Excel document icon'
    } },
    @{ Name = 'ole-ppt'; Steps = '新建PowerPoint.Slide.12对象，写入示例幻灯片文字'; Action = {
        $inline = Add-Ole (Get-FeatureRange) -ClassType 'PowerPoint.Slide.12'
        $slide = Open-Ole $inline
        $text = $slide.Shapes.AddTextbox(1, 20, 20, 350, 80)
        $text.TextFrame.TextRange.Text = '嵌入幻灯片'
        $text.TextFrame.TextRange.Font.Size = 28
        Hide-Ole $inline
        $inline.Width = 300
        $inline.Height = 180
        $script:Details['previewExpectation'] = 'PowerPoint slide showing 嵌入幻灯片'
    } },
    @{ Name = 'ole-with-text'; Steps = '同一个正文段落中，文字末尾插Excel嵌入对象'; Action = {
        $prefix = '对象位于这句话的末尾：'
        $range = Get-FeatureRange
        $start = [int]$range.Start
        $range.Text = $prefix
        $range.SetRange($start + $prefix.Length, $start + $prefix.Length)
        $inline = Add-ExcelObject $range
        $inline.Width = 180
        $inline.Height = 105
        $script:Details['previewExpectation'] = 'Text and worksheet preview in one paragraph'
    } },
    @{ Name = 'ole-in-table'; Steps = '2×2表格左上格插Excel嵌入对象'; Action = {
        $table = $script:Document.Tables.Add((Get-FeatureRange), 2, 2)
        $table.Borders.Enable = 1
        $table.Cell(1, 2).Range.Text = '右上'
        $table.Cell(2, 1).Range.Text = '左下'
        $table.Cell(2, 2).Range.Text = '右下'
        $range = $table.Cell(1, 1).Range.Duplicate
        $range.Collapse(1)
        $inline = Add-ExcelObject $range
        $inline.Width = 180
        $inline.Height = 105
        $script:Details['previewExpectation'] = 'Worksheet preview in the upper-left table cell'
    } }
)

if ($VerifyOnly) {
    $checks = [Collections.Generic.List[object]]::new()
    foreach ($file in Get-ChildItem -LiteralPath (Join-Path $OutputRoot 'ole') -Filter '*.docx' -File) {
        if ($file.Name.StartsWith('~$')) { continue }
        $check = [ordered]@{ file = "ole/$($file.Name)"; status = 'failed'; details = $null; error = $null }
        try {
            $check['details'] = Test-CopiedPackage $file.FullName ($file.BaseName -replace '-\d+$', '')
            $check['status'] = 'passed'
        }
        catch { $check['error'] = $_.Exception.Message }
        $checks.Add([pscustomobject]$check)
    }
    $checks | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $OutputRoot '_scripts/ole-selfcheck.json') -Encoding utf8
    $checks | Select-Object file,status,error | Format-Table -AutoSize
    exit 0
}

try {
    foreach ($case in $cases) {
        if ($Only.Count -gt 0 -and $case.Name -notin $Only) { continue }
        $script:Document = $null
        $script:Details = [ordered]@{}
        $path = Get-UnusedPath ('ole/' + $case.Name + '.docx')
        $record = [ordered]@{ runId = $runId; requestedFile = "ole/$($case.Name).docx"; file = [IO.Path]::GetRelativePath($OutputRoot, $path).Replace('\', '/'); status = 'incomplete'; method = '_scripts/create-ole.ps1'; steps = $case.Steps; wordVersion = $null; wordBuild = $null; wordProcessId = $null; details = $script:Details; savedOnce = $false; visualInspection = 'pending'; selfCheck = [ordered]@{ status = 'pending' }; pdfPreview = $null; error = $null }
        try {
            Initialize-WordInstance
            $record['wordVersion'] = [string]$script:Word.Version
            $record['wordBuild'] = [string]$script:Word.Build
            $record['wordProcessId'] = $script:WordProcessId
            $script:Document = $script:Word.Documents.Add()
            $script:Document.Content.Text = "before 前文`r`rafter 后文`r"
            & $case.Action
            $script:Document.Repaginate()
            if ($script:Document.CompatibilityMode -lt 15) { $script:Document.SetCompatibilityMode(15) }
            $script:Document.SaveAs2($path, 12)
            $record['savedOnce'] = $true
            $record['status'] = 'created-pending-verification'
            if ($ExportPdf) {
                try {
                    $pdf = Get-UnusedPath ('_previews/ole/' + [IO.Path]::GetFileNameWithoutExtension($path) + '.pdf')
                    $script:Document.ExportAsFixedFormat($pdf, 17, $false)
                    $record['pdfPreview'] = [IO.Path]::GetRelativePath($OutputRoot, $pdf).Replace('\', '/')
                }
                catch { $record['pdfError'] = $_.Exception.ToString() }
            }
            $script:Document.Close(0)
            [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($script:Document)
            $script:Document = $null
            $record['selfCheck'] = Test-CopiedPackage $path $case.Name
            $record['status'] = 'passed-package-check-pending-visual-inspection'
        }
        catch {
            $record['error'] = $_.Exception.ToString()
            $record['errorPosition'] = $_.InvocationInfo.PositionMessage
            if ($record['savedOnce']) { $record['status'] = 'saved-but-verification-failed' }
            Write-Warning "$($case.Name): $($_.Exception.Message)"
        }
        finally {
            if ($null -ne $script:Document) {
                try { $script:Document.Close(0) } catch { $record['closeError'] = $_.Exception.Message }
                try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($script:Document) } catch { }
                $script:Document = $null
            }
            if ($null -ne $script:Word) {
                if ($script:WordOwned) { try { $script:Word.Quit(0) } catch { $record['quitError'] = $_.Exception.Message } }
                try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($script:Word) } catch { }
                $script:Word = $null
                $script:WordOwned = $false
            }
            [GC]::Collect()
            [GC]::WaitForPendingFinalizers()
            $rows.Add([pscustomobject]$record)
            [ordered]@{ generatedAt = [DateTimeOffset]::Now.ToString('o'); evidence = 'Actual desktop Word/Excel/PowerPoint object models; PDF visual inspection remains separate'; cases = @($rows.ToArray()) } | ConvertTo-Json -Depth 15 | Set-Content -LiteralPath $logPath -Encoding utf8
        }
    }
}
finally {
    if ($null -ne $script:Word) {
        if ($script:WordOwned) { try { $script:Word.Quit(0) } catch { Write-Warning $_.Exception.Message } }
        try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($script:Word) } catch { }
    }
    [GC]::Collect()
    [GC]::WaitForPendingFinalizers()
}
Write-Output "Results: $logPath"
