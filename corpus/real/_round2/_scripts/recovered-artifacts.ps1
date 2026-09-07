param([Parameter(Mandatory)][object]$Word,[Parameter(Mandatory)][string]$Source,[switch]$ActivateChartData)
$root='C:\word\real-word-round2-20260907'
if($Source -ne [IO.Path]::GetFileName($Source)){throw 'Expected basename'}
$d=$Word.ActiveDocument
$stem=[IO.Path]::GetFileNameWithoutExtension($Source)
function Stage([string]$Name){$Name | Set-Content (Join-Path $root '_control\recovery-stage.txt')}
Stage 'active-document'
. (Join-Path $root '_scripts\roundtrip-helpers.ps1')
$r=[ordered]@{source=$Source;recoveredName=$d.Name;compat=$d.CompatibilityMode;text=$d.Content.Text;paragraphs=$d.Paragraphs.Count;inlineShapes=$d.InlineShapes.Count;shapes=$d.Shapes.Count;charts=@();shapeDetails=@();method='Word UI open, accepted unreadable-content recovery, Word SaveAs2 and PDF export'}
Stage 'basic-readout'
for($si=1;$si -le $d.Shapes.Count;$si++){$s=$d.Shapes.Item($si);$r.shapeDetails+=@{name=$s.Name;type=$s.Type;width=$s.Width;height=$s.Height}}
Stage 'shape-readout'
for($i=1;$i -le $d.InlineShapes.Count;$i++){
 $s=$d.InlineShapes.Item($i)
 if($s.HasChart -eq -1){
  $c=$s.Chart
  Stage 'chart-readout'
  $cr=@{index=$i;before=(Get-ChartReadout $c)}
  if($ActivateChartData){try{$c.ChartData.Activate();$wb=$c.ChartData.Workbook;$cr.cells=$wb.Worksheets.Item(1).UsedRange.Value2;$wb.Close($false);$cr.after=Get-ChartReadout $c;$cr.activation='ok'}catch{$cr.error=$_.ToString()}}else{$cr.activation='not_attempted_in_this_pass'}
  $r.charts+=$cr
 }
}
Stage 'before-save'
$savePath=[string](Join-Path $root ('_resaved\'+$stem+'-resaved-by-word.docx'))
$d.SaveAs2($savePath,12)
Stage 'before-pdf'
$pdfPath=[string](Join-Path $root ('_previews\edited\'+$stem+'.pdf'))
$d.ExportAsFixedFormat($pdfPath,17)
Stage 'before-json'
$r | ConvertTo-Json -Depth 15 | Set-Content (Join-Path $root ('_readouts\'+$stem+'-recovered.json')) -Encoding utf8
$r
