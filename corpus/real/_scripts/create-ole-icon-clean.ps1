#requires -Version 7.0
[CmdletBinding()]
param(
    [string]$OutputRoot = (Split-Path -Parent $PSScriptRoot),
    [string]$SourceWorkbook = '',
    [string]$IconFileName = 'C:\Program Files\Microsoft Office\root\Office16\EXCEL.EXE',
    [int]$IconIndex = 0,
    [string]$IconLabel = '示例工作表',
    [bool]$ShowWord = $true
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$OutputRoot = [IO.Path]::GetFullPath($OutputRoot)
if ([string]::IsNullOrEmpty($SourceWorkbook)) { $SourceWorkbook = Join-Path $OutputRoot '_assets/ole-excel-source.xlsx' }
$SourceWorkbook = (Get-Item -LiteralPath $SourceWorkbook).FullName
$IconFileName = (Get-Item -LiteralPath $IconFileName).FullName
$firstVersion = Join-Path $OutputRoot 'ole/ole-icon.docx'
$firstVersionHash = if (Test-Path -LiteralPath $firstVersion) { (Get-FileHash -LiteralPath $firstVersion -Algorithm SHA256).Hash } else { $null }
foreach ($directory in @('ole', '_previews/ole', '_checks/ole', '_scripts')) {
    [void][IO.Directory]::CreateDirectory((Join-Path $OutputRoot $directory))
}

for ($suffix = 2; ; $suffix++) {
    $stem = "ole-icon-$suffix"
    $targetPath = Join-Path $OutputRoot "ole/$stem.docx"
    $previewPath = Join-Path $OutputRoot "_previews/ole/$stem.pdf"
    $copyZipPath = Join-Path $OutputRoot "_checks/ole/$stem.zip"
    $inspectionPath = Join-Path $OutputRoot "_checks/ole/$stem-readonly-inspection.docx"
    $logPath = Join-Path $OutputRoot "_scripts/$stem-results.json"
    if (@($targetPath, $previewPath, $copyZipPath, $inspectionPath, $logPath | Where-Object { Test-Path -LiteralPath $_ }).Count -eq 0) { break }
}

$record = [ordered]@{
    createdAt = [DateTimeOffset]::Now.ToString('o')
    file = "ole/$stem.docx"
    method = '_scripts/create-ole-icon-clean.ps1'
    status = 'incomplete'
    wordVersion = $null
    wordBuild = $null
    wordProcessId = $null
    sourceWorkbook = [IO.Path]::GetRelativePath($OutputRoot, $SourceWorkbook).Replace('\', '/')
    requestedIconFileName = $IconFileName
    requestedIconIndex = $IconIndex
    requestedIconLabel = $IconLabel
    savedOnce = $false
    activationBeforeSave = 'No OLEFormat.DoVerb, OLEFormat.Activate, OLEFormat.Object access, or explicit Excel COM calls; only the containing document is activated and its ordinary text selected.'
    originalFirstVersionSha256 = $firstVersionHash
    firstVersionUnchanged = $null
    pdfPreview = $null
    visualInspection = 'pending; verify the Excel icon and label, with no hatching'
    activationVerification = 'pending; use only the read-only inspection copy after reviewing the initial PDF'
    error = $null
}

Add-Type -AssemblyName System.IO.Compression.FileSystem
if (-not ('CorpusCleanIconNative' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CorpusCleanIconNative {
    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
}
'@
}

$word = $null
$document = $null
$owned = $false
try {
    $existingPids = @(Get-Process -Name WINWORD -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)
    $word = New-Object -ComObject Word.Application
    $document = $word.Documents.Add()
    $owner = [uint32]0
    [void][CorpusCleanIconNative]::GetWindowThreadProcessId([IntPtr]$document.Windows.Item(1).Hwnd, [ref]$owner)
    if ($owner -eq 0 -or [int]$owner -in $existingPids) { throw "The created document uses pre-existing or unknown Word PID $owner; refusing to control that application." }
    $owned = $true
    $record['wordProcessId'] = [int]$owner
    $record['wordVersion'] = [string]$word.Version
    $record['wordBuild'] = [string]$word.Build
    $word.Visible = $ShowWord
    $document.Content.Text = "before 前文`r`rafter 后文`r"
    $range = $document.Paragraphs.Item(2).Range.Duplicate
    $range.Collapse(1)

    # Explicit icon arguments avoid the system's generic default icon.
    $inline = $document.InlineShapes.AddOLEObject([Type]::Missing, $SourceWorkbook, $false, $true, $IconFileName, $IconIndex, $IconLabel, $range)
    $record['progId'] = [string]$inline.OLEFormat.ProgID
    $record['displayAsIcon'] = [bool]$inline.OLEFormat.DisplayAsIcon
    $record['actualIconName'] = [string]$inline.OLEFormat.IconName
    $record['actualIconPath'] = [string]$inline.OLEFormat.IconPath
    $record['actualIconIndex'] = [int]$inline.OLEFormat.IconIndex
    $record['actualIconLabel'] = [string]$inline.OLEFormat.IconLabel
    if (-not $record['displayAsIcon'] -or $record['progId'] -ne 'Excel.Sheet.12') { throw 'Word did not create the requested Excel icon OLE object.' }

    # Select ordinary text without ever activating the embedded server.
    $document.Activate()
    $document.Range(0, 0).Select()
    $document.Repaginate()
    if ($document.CompatibilityMode -lt 15) { $document.SetCompatibilityMode(15) }
    $document.SaveAs2($targetPath, 12)
    $record['savedOnce'] = $true
    $record['status'] = 'saved-pending-checks'
    $document.ExportAsFixedFormat($previewPath, 17, $false)
    $record['pdfPreview'] = "_previews/ole/$stem.pdf"
    $document.Close(0)
    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($document)
    $document = $null

    $originalHash = (Get-FileHash -LiteralPath $targetPath -Algorithm SHA256).Hash
    Copy-Item -LiteralPath $targetPath -Destination $copyZipPath
    $archive = [IO.Compression.ZipFile]::OpenRead($copyZipPath)
    try {
        $reader = [IO.StreamReader]::new($archive.GetEntry('word/document.xml').Open())
        try { [xml]$xml = $reader.ReadToEnd() } finally { $reader.Dispose() }
        $ns = [Xml.XmlNamespaceManager]::new($xml.NameTable)
        $ns.AddNamespace('w', 'http://schemas.openxmlformats.org/wordprocessingml/2006/main')
        $ns.AddNamespace('o', 'urn:schemas-microsoft-com:office:office')
        $ns.AddNamespace('v', 'urn:schemas-microsoft-com:vml')
        $objects = $xml.SelectNodes('//w:object[v:shape/v:imagedata and o:OLEObject[@Type="Embed" and @DrawAspect="Icon" and @ProgID="Excel.Sheet.12"]]', $ns)
        if ($objects.Count -ne 1) { throw 'Copied package does not contain exactly one embedded Excel icon OLE object with a preview reference.' }
        $previews = @($archive.Entries | Where-Object FullName -match '^word/media/.*\.(emf|wmf)$' | Select-Object -ExpandProperty FullName)
        $embeddings = @($archive.Entries | Where-Object FullName -match '^word/embeddings/.*\.xlsx$' | Select-Object -ExpandProperty FullName)
        if ($previews.Count -eq 0 -or $embeddings.Count -eq 0) { throw 'The preview metafile or embedded workbook is missing.' }
        $paragraphText = @($xml.SelectNodes('/w:document/w:body/w:p', $ns) | ForEach-Object { ($_.SelectNodes('.//w:t', $ns) | ForEach-Object InnerText) -join '' })
        if ('before 前文' -notin $paragraphText -or 'after 后文' -notin $paragraphText) { throw 'Standalone marker paragraphs are missing.' }
        $record['selfCheck'] = [ordered]@{ status = 'passed'; copiedZip = "_checks/ole/$stem.zip"; previewParts = $previews; embeddingParts = $embeddings }
    }
    finally { $archive.Dispose() }
    Copy-Item -LiteralPath $targetPath -Destination $inspectionPath
    $inspectionFile = Get-Item -LiteralPath $inspectionPath
    $inspectionFile.Attributes = $inspectionFile.Attributes -bor [IO.FileAttributes]::ReadOnly
    $record['readonlyInspectionCopy'] = "_checks/ole/$stem-readonly-inspection.docx"
    $record['originalSha256'] = $originalHash
    $record['originalUnchangedAfterCopies'] = (Get-FileHash -LiteralPath $targetPath -Algorithm SHA256).Hash -eq $originalHash
    if (-not $record['originalUnchangedAfterCopies']) { throw 'The new original changed during copied-package verification.' }
    if ($null -ne $firstVersionHash) {
        $record['firstVersionUnchanged'] = (Get-FileHash -LiteralPath $firstVersion -Algorithm SHA256).Hash -eq $firstVersionHash
        if (-not $record['firstVersionUnchanged']) { throw 'The first-version original changed during this run.' }
    }
    $record['status'] = 'passed-package-check-pending-visual-and-readonly-activation-verification'
}
catch {
    $record['error'] = $_.Exception.ToString()
    $record['errorPosition'] = $_.InvocationInfo.PositionMessage
    if ($record['savedOnce']) { $record['status'] = 'saved-but-checks-incomplete' }
    Write-Warning $_.Exception.Message
}
finally {
    if ($null -ne $document) {
        try { $document.Close(0) } catch { $record['closeError'] = $_.Exception.Message }
        try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($document) } catch { }
    }
    if ($null -ne $word) {
        if ($owned) { try { $word.Quit(0) } catch { $record['quitError'] = $_.Exception.Message } }
        try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($word) } catch { }
    }
    [GC]::Collect()
    [GC]::WaitForPendingFinalizers()
    $record | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $logPath -Encoding utf8
}
Write-Output "Results: $logPath"
