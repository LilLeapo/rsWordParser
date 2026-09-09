param(
    [string]$Root = 'C:\word\real-word-round2-20260907',
    [string]$Destination = 'C:\word\real-word-round2-20260907.zip'
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$resolvedRoot = (Resolve-Path -LiteralPath $Root).Path.TrimEnd('\')
if (Test-Path -LiteralPath $Destination) { throw "Refusing to overwrite existing archive: $Destination" }
$files = @(Get-ChildItem -LiteralPath $resolvedRoot -File -Recurse | Where-Object {
    $relative = $_.FullName.Substring($resolvedRoot.Length + 1).Replace('\','/')
    $relative -notmatch '(^|/)_control(/|$)' -and $_.Name -notlike '~$*' -and
    $relative -notin @('01-source-test.docx','edited-current-item.json')
} | Sort-Object FullName)
$archive = [IO.Compression.ZipFile]::Open($Destination,[IO.Compression.ZipArchiveMode]::Create)
try {
    foreach ($file in $files) {
        $relative = $file.FullName.Substring($resolvedRoot.Length + 1).Replace('\','/')
        $entry = (Split-Path -Leaf $resolvedRoot) + '/' + $relative
        [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive,$file.FullName,$entry,[IO.Compression.CompressionLevel]::Optimal)
    }
} finally { $archive.Dispose() }
[ordered]@{
    path = $Destination
    files = $files.Count
    bytes = (Get-Item -LiteralPath $Destination).Length
    sha256 = (Get-FileHash -LiteralPath $Destination -Algorithm SHA256).Hash
} | ConvertTo-Json
