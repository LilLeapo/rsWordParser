function Get-ChartReadout($chart) {
    $r = [ordered]@{ title=$null; type=$null; series=@() }
    try { $r.type = $chart.ChartType; if ($chart.HasTitle) { $r.title=$chart.ChartTitle.Text } } catch { $r.error=$_.ToString() }
    try {
        $sc=$chart.SeriesCollection()
        for($i=1;$i -le $sc.Count;$i++) {
            $s=$sc.Item($i)
            $r.series += @{ name=$s.Name; values=@($s.Values); categories=@($s.XValues) }
        }
    } catch { $r.seriesError=$_.ToString() }
    return $r
}
function Save-RoundtripReadout($word, [string]$Root='C:\word\real-word-round2-20260907') {
    $doc=$word.ActiveDocument
    $name=[IO.Path]::GetFileNameWithoutExtension($doc.Name)
    $r=[ordered]@{ file=$doc.Name; compat=$doc.CompatibilityMode; paragraphs=$doc.Paragraphs.Count; text=$doc.Content.Text; inlineShapes=$doc.InlineShapes.Count; shapes=$doc.Shapes.Count; charts=@(); shapeDetails=@() }
    for($i=1;$i -le $doc.InlineShapes.Count;$i++) {
        $item=$doc.InlineShapes.Item($i)
        if ($item.HasChart -eq -1) { $r.charts += Get-ChartReadout $item.Chart }
    }
    for($i=1;$i -le $doc.Shapes.Count;$i++) {
        $s=$doc.Shapes.Item($i)
        $r.shapeDetails += @{name=$s.Name; type=$s.Type; width=$s.Width; height=$s.Height; left=$s.Left; top=$s.Top; wrap=$s.WrapFormat.Type; relativeHorizontal=$s.RelativeHorizontalPosition}
    }
    New-Item -ItemType Directory -Path (Join-Path $Root '_readouts') -Force | Out-Null
    $r | ConvertTo-Json -Depth 12 | Set-Content (Join-Path $Root ('_readouts\'+$name+'.json')) -Encoding utf8
    return $r
}
function Test-ActiveChartData($word,[int]$Index=1,[string]$Root='C:\word\real-word-round2-20260907') {
    $doc=$word.ActiveDocument
    $name=[IO.Path]::GetFileNameWithoutExtension($doc.Name)
    $chart=$doc.InlineShapes.Item($Index).Chart
    $r=[ordered]@{ file=$doc.Name; index=$Index; before=(Get-ChartReadout $chart); activation='not_attempted'; cells=$null }
    try {
        $chart.ChartData.Activate()
        $r.activation='ok'
        $wb=$chart.ChartData.Workbook
        $r.cells=$wb.Worksheets.Item(1).UsedRange.Value2
        $wb.Close($false)
        $r.after=Get-ChartReadout $chart
    } catch { $r.activation='error'; $r.error=$_.ToString() }
    $r | ConvertTo-Json -Depth 15 | Set-Content (Join-Path $Root ('_readouts\'+$name+'-chartdata-'+$Index+'.json')) -Encoding utf8
    return $r
}
function Save-ActiveArtifacts($word,[string]$Subdir='roundtrip',[string]$Root='C:\word\real-word-round2-20260907',[bool]$Pdf=$true) {
    $doc=$word.ActiveDocument
    $name=[IO.Path]::GetFileNameWithoutExtension($doc.Name)
    $save=Join-Path $Root '_resaved'
    $preview=Join-Path $Root ('_previews\'+$Subdir)
    New-Item -ItemType Directory -Path $save,$preview -Force | Out-Null
    'before-save' | Set-Content (Join-Path $Root '_control\artifact-stage.txt')
    $savePath=[string](Join-Path $save ($name+'-resaved-by-word.docx'))
    $doc.SaveAs2($savePath,12)
    'after-save' | Set-Content (Join-Path $Root '_control\artifact-stage.txt')
    if ($Pdf) {
        $doc.ExportAsFixedFormat([string](Join-Path $preview ($name+'.pdf')),17)
    }
    return @{saved=$name; pdf=$Pdf}
}
