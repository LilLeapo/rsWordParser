param([Parameter(Mandatory)][object]$Word,[string]$File,[switch]$SaveCurrent,[switch]$ChartData)
$root='C:\word\real-word-round2-20260907'
. (Join-Path $root '_scripts\roundtrip-helpers.ps1')
if($SaveCurrent){ Save-ActiveArtifacts $Word 'edited' }
if(-not $File){return}
& (Join-Path $root '_scripts\ui-case.ps1') -Word $Word -File $File -Group edited -CloseCurrent
$d=$Word.ActiveDocument
$r=Save-RoundtripReadout $Word
$r.comments=@($d.Comments | ForEach-Object { @{author=$_.Author;text=$_.Range.Text;scope=$_.Scope.Text} })
$r.headers=@($d.Sections | ForEach-Object { @{index=$_.Index;primary=$_.Headers.Item(1).Range.Text;first=$_.Headers.Item(2).Range.Text;even=$_.Headers.Item(3).Range.Text} })
$r.tables=@($d.Tables | ForEach-Object { @{rows=$_.Rows.Count;cells=$_.Range.Cells.Count;text=$_.Range.Text} })
if($File -like '*--header.docx') { $d.Sections.Last.Headers.Item(1).Range.Select() }
if($File -like '*--comment.docx') { $d.ActiveWindow.View.ShowComments=$true; $d.ActiveWindow.View.ShowRevisionsAndComments=$true; $d.Comments.Item($d.Comments.Count).Scope.Select() }
$r | ConvertTo-Json -Depth 14 | Set-Content (Join-Path $root ('_readouts\'+[IO.Path]::GetFileNameWithoutExtension($File)+'-ui.json')) -Encoding utf8
if($ChartData) {
 for($i=1;$i -le $d.InlineShapes.Count;$i++) { if($d.InlineShapes.Item($i).HasChart -eq -1){Test-ActiveChartData $Word $i} }
}
return $r
