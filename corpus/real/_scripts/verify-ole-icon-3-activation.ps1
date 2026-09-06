#requires -Version 5.1
[CmdletBinding()]
param([string]$OutputRoot = (Split-Path -Parent $PSScriptRoot))

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$OutputRoot = [IO.Path]::GetFullPath($OutputRoot)
$originalPath = Join-Path $OutputRoot 'ole/ole-icon-3.docx'
$inspectionPath = Join-Path $OutputRoot '_checks/ole/ole-icon-3-readonly-inspection.docx'
$expectedHash = '902E32369E40BACDDD54A556A1BE733FF39D2903DC5CF18AB95C77BC152B93A6'
$logPath = Join-Path $OutputRoot ('_scripts/ole-icon-3-activation-' + (Get-Date -Format 'yyyyMMdd-HHmmss-fff') + '.json')
$record = [ordered]@{
    started = [DateTimeOffset]::Now.ToString('o')
    file = 'ole/ole-icon-3.docx'
    inspectionCopy = '_checks/ole/ole-icon-3-readonly-inspection.docx'
    method = '_scripts/verify-ole-icon-3-activation.ps1'
    status = 'started'
    phase = 'checking-copy'
    expectedOriginalSha256 = $expectedHash
    officeSaveMethodsCalled = $false
    cellsWritten = $false
    sourceWorkbookOpened = $false
}

function Write-ActivationRecord {
    $record | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $logPath -Encoding utf8
}

if (-not ('CorpusIconActivationNative' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CorpusIconActivationNative {
    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
}
'@
}

function Get-WindowOwner([long]$WindowHandle) {
    $owner = [uint32]0
    [void][CorpusIconActivationNative]::GetWindowThreadProcessId([IntPtr]$WindowHandle, [ref]$owner)
    return [int]$owner
}

$word = $null
$document = $null
$inline = $null
$ole = $null
$book = $null
$sheet = $null
$excel = $null
$wordOwned = $false
$excelOwned = $false
$activated = $false
$documentClosed = $false
$bookClosed = $false
$cleanupErrors = [Collections.Generic.List[string]]::new()
try {
    $record.originalSha256Before = (Get-FileHash -LiteralPath $originalPath -Algorithm SHA256).Hash
    if ($record.originalSha256Before -ne $expectedHash) { throw 'The original hash differs from the reviewed ole-icon-3 original.' }
    if (-not (Test-Path -LiteralPath $inspectionPath)) { Copy-Item -LiteralPath $originalPath -Destination $inspectionPath }
    $record.copySha256Before = (Get-FileHash -LiteralPath $inspectionPath -Algorithm SHA256).Hash
    if ($record.copySha256Before -ne $expectedHash) { throw 'The inspection copy is not identical to the reviewed original.' }
    $inspectionFile = Get-Item -LiteralPath $inspectionPath
    $inspectionFile.Attributes = $inspectionFile.Attributes -bor [IO.FileAttributes]::ReadOnly
    $existingWordPids = @(Get-Process -Name WINWORD -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)
    $existingExcelPids = @(Get-Process -Name EXCEL -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)
    $record.phase = 'opening-readonly-copy'
    Write-ActivationRecord

    $word = New-Object -ComObject Word.Application
    $openPath = [object]$inspectionPath
    $confirmConversions = [object]$false
    $readOnly = [object]$true
    $addToRecentFiles = [object]$false
    $document = $word.Documents.Open([string]$inspectionPath, $false, $true, $false)
    $wordPid = Get-WindowOwner $document.Windows.Item(1).Hwnd
    if ($wordPid -eq 0 -or $wordPid -in $existingWordPids) { throw "The new Word application uses a pre-existing or unknown process: $wordPid." }
    $wordOwned = $true
    $record.wordProcessId = $wordPid
    $record.wordVersion = [string]$word.Version
    $record.wordBuild = [string]$word.Build
    $word.Visible = $true
    $record.openedDocumentReadOnly = [bool]$document.ReadOnly
    if (-not $record.openedDocumentReadOnly) { throw 'Word did not open the inspection document read-only.' }
    if ($document.InlineShapes.Count -ne 1) { throw 'Expected exactly one inline OLE icon.' }
    $inline = $document.InlineShapes.Item(1)
    $ole = $inline.OLEFormat
    $record.progId = [string]$ole.ProgID
    $record.displayAsIcon = [bool]$ole.DisplayAsIcon
    if ($record.progId -ne 'Excel.Sheet.12' -or -not $record.displayAsIcon) { throw 'The inspection object is not the expected Excel icon.' }
    $document.Activate()
    $inline.Range.Select()
    $record.phase = 'activating-embedded-excel'
    Write-ActivationRecord
    $primaryVerb = [object]0
    $ole.DoVerb(0)
    $activated = $true
    $record.primaryVerb = 'DoVerb(0) succeeded'
    $book = $ole.Object
    if ($null -eq $book) { throw 'The activated OLE object did not expose its embedded workbook.' }
    $sheet = $book.Worksheets.Item(1)
    $excel = $book.Application
    $excelPid = Get-WindowOwner $excel.Hwnd
    $excelOwned = $excelPid -ne 0 -and $excelPid -notin $existingExcelPids
    $record.excelProcessId = $excelPid
    $record.excelProcessCreatedDuringActivation = $excelOwned
    $record.excelVersion = [string]$excel.Version
    $record.workbookName = [string]$book.Name
    $record.worksheetName = [string]$sheet.Name
    $record.cells = [ordered]@{
        A1 = [double]$sheet.Cells.Item(1, 1).Value2
        B1 = [double]$sheet.Cells.Item(1, 2).Value2
    }
    $record.cellEvidence = 'Read directly from the workbook exposed by the activated embedded OLE object; no external XLSX was opened.'
    if ($record.cells.A1 -ne 12 -or $record.cells.B1 -ne 34) { throw 'Embedded workbook values differ from A1=12 and B1=34.' }
    $record.status = 'values-verified-pending-close-and-hash'
    $record.phase = 'closing-without-saving'
    Write-ActivationRecord
}
catch {
    $record.status = 'failed'
    $record.error = $_.Exception.ToString()
    $record.errorPosition = $_.InvocationInfo.PositionMessage
}
finally {
    if ($activated -and $null -ne $ole) {
        try {
            $hideVerb = [object](-3)
            $ole.DoVerb(-3)
            $record.hideVerb = 'DoVerb(-3) succeeded'
        }
        catch { $record.hideVerb = 'unsupported: ' + $_.Exception.Message }
    }
    if ($null -ne $book) {
        try { $book.Close($false); $bookClosed = $true }
        catch { $cleanupErrors.Add('Embedded workbook Close(false): ' + $_.Exception.Message) }
    }
    if ($null -ne $document) {
        try {
            $noSave = [object]0
            $document.Close(0)
            $documentClosed = $true
        }
        catch { $cleanupErrors.Add('Word document Close(wdDoNotSaveChanges): ' + $_.Exception.Message) }
    }
    if ($wordOwned -and $null -ne $word) {
        try {
            if ($word.Documents.Count -eq 0) {
                $noSave = [object]0
                $word.Quit([ref]$noSave)
                $record.wordQuit = $true
            }
            else { $record.wordQuit = 'Skipped because another document is open in this instance.' }
        }
        catch { $cleanupErrors.Add('Owned Word Quit: ' + $_.Exception.Message) }
    }
    if ($excelOwned -and $null -ne $excel) {
        try {
            if ($excel.Workbooks.Count -eq 0) { $excel.Quit(); $record.excelQuit = $true }
            else { $record.excelQuit = 'Skipped because another workbook remains open in this instance.' }
        }
        catch { $record.excelQuit = 'Server already closed or unavailable: ' + $_.Exception.Message }
    }
    foreach ($comObject in @($sheet, $book, $excel, $ole, $inline, $document, $word)) {
        if ($null -ne $comObject -and [Runtime.InteropServices.Marshal]::IsComObject($comObject)) {
            try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($comObject) } catch { }
        }
    }
    [GC]::Collect()
    [GC]::WaitForPendingFinalizers()
    $record.documentClosedWithoutSaving = $documentClosed
    $record.embeddedWorkbookClosedWithoutSaving = $bookClosed
    $record.cleanupErrors = @($cleanupErrors.ToArray())
    try {
        $record.originalSha256After = (Get-FileHash -LiteralPath $originalPath -Algorithm SHA256).Hash
        $record.copySha256After = (Get-FileHash -LiteralPath $inspectionPath -Algorithm SHA256).Hash
        $record.originalUnchanged = $record.originalSha256After -eq $expectedHash
        $record.copyUnchanged = $record.copySha256After -eq $expectedHash
        if ($record.status -eq 'values-verified-pending-close-and-hash' -and $documentClosed -and $bookClosed -and $cleanupErrors.Count -eq 0 -and $record.originalUnchanged -and $record.copyUnchanged) {
            $record.status = 'passed'
        }
        elseif ($record.status -ne 'failed') { $record.status = 'verification-or-cleanup-incomplete' }
    }
    catch { $record.status = 'failed'; $record.hashCheckError = $_.Exception.Message }
    $record.phase = 'finished'
    $record.finished = [DateTimeOffset]::Now.ToString('o')
    Write-ActivationRecord
}
$record | ConvertTo-Json -Depth 10
Write-Output "Log: $logPath"
if ($record.status -ne 'passed') { exit 1 }
