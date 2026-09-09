param([Parameter(Mandatory)][object]$Word,[Parameter(Mandatory)][string]$File,[ValidateSet('edited','fixtures','roundtrip')][string]$Group='edited',[switch]$CloseCurrent)
$inputRoot='C:\word\round2-work-20260907\real-word-round2-inputs'
$folder=switch($Group) { 'edited' { '_roundtrip\edited' }; 'fixtures' { 'fixtures' }; 'roundtrip' { '_roundtrip' } }
if ($File -ne [IO.Path]::GetFileName($File)) { throw 'File must be a basename.' }
if ($CloseCurrent -and $Word.Documents.Count -gt 0) {
    $current=$Word.ActiveDocument
    if (-not $current.FullName.StartsWith('C:\word\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Refusing to close a document outside the task directory.' }
    $current.Close(0)
}
$Word.DisplayAlerts=-1
$Word.Visible=$true
$document=$Word.Documents.Open((Join-Path (Join-Path $inputRoot $folder) $File),$false,$true,$false)
$document.Activate()
$document.ActiveWindow.View.Type=3
$document.ActiveWindow.View.Zoom.Percentage=95
$document.ActiveWindow.View.ShowAll=$false
$document.ActiveWindow.View.ShowHiddenText=$false
$Word.Selection.HomeKey(6) | Out-Null
return @{file=$document.Name;compat=$document.CompatibilityMode;text=$document.Content.Text;paragraphs=$document.Paragraphs.Count;inlineShapes=$document.InlineShapes.Count;shapes=$document.Shapes.Count}
