param([Parameter(Mandatory=$true)][object]$Word, [string]$Root='C:\word\real-word-round2-20260907')
$ErrorActionPreference='Stop'
function Get-SavedHash([string]$Path) {
    $stream=[IO.File]::Open($Path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
    $hasher=[Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($hasher.ComputeHash($stream)).Replace('-','') }
    finally { $hasher.Dispose(); $stream.Dispose() }
}
$setting=Get-Content -LiteralPath (Join-Path $Root '_readouts/task-d-local-user-setting.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$before=[bool]$Word.Options.UseLocalUserInfo
$Word.Options.UseLocalUserInfo=[bool]$setting.before
$expected=@(
    'ink-to-shape--ink-resaved-by-word.docx',
    'ink-to-shape--newchart-resaved-by-word.docx',
    'ink-to-shape--newimage-resaved-by-word.docx',
    'ink-highlighter--ink-resaved-by-word.docx',
    'ink-highlighter--newchart-resaved-by-word.docx',
    'ink-highlighter--newimage-resaved-by-word.docx',
    'ink-pen--ink-resaved-by-word.docx',
    'ink-pen--newimage-resaved-by-word.docx',
    'ink-pen--newchart-resaved-by-word.docx',
    'fields-toc--split-resaved-by-word.docx'
) | ForEach-Object { [IO.Path]::GetFullPath((Join-Path $Root ('_resaved/'+$_))) }
$closed=@()
for ($i=$Word.Documents.Count; $i -ge 1; $i--) {
    $document=$Word.Documents.Item($i)
    $path=[string]$document.FullName
    if ($path -notin $expected) { continue }
    $sha256=Get-SavedHash $path
    $saved=[bool]$document.Saved
    $document.Close(0)
    $afterHash=Get-SavedHash $path
    if ($sha256 -ne $afterHash) { throw "DOCX changed on close: $path" }
    $closed+=@{path=$path;savedFlagBeforeClose=$saved;closeMethod='wdDoNotSaveChanges';sha256=$sha256;hashRetained=$true}
}
$record=[ordered]@{
    date=(Get-Date).ToString('o')
    useLocalUserInfoBeforeCleanup=$before
    useLocalUserInfoRestored=[bool]$Word.Options.UseLocalUserInfo
    originalUseLocalUserInfo=[bool]$setting.before
    userName=[string]$Word.UserName
    userInitials=[string]$Word.UserInitials
    closed=$closed
    remainingDocuments=@(foreach($document in $Word.Documents){[string]$document.FullName})
    note='Only the ten explicitly inventoried task-owned documents were closed. Their saved bytes were unchanged. Other Word and Excel processes were not controlled.'
}
if ($record.useLocalUserInfoRestored -ne $record.originalUseLocalUserInfo) { throw 'UseLocalUserInfo restore failed.' }
ConvertTo-Json -InputObject $record -Depth 12 | Set-Content -LiteralPath (Join-Path $Root '_readouts/word-session-finish.json') -Encoding UTF8
ConvertTo-Json -InputObject $record -Depth 12
