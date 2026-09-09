param(
    [Parameter(Mandatory=$true)][ValidateSet('Prepare','Save','Inspect','Close','SelectBody','OpenReadOnly','ShowMarkup')][string]$Action,
    [Parameter(Mandatory=$true)][string]$Case,
    [string]$Root = (Split-Path -Parent $PSScriptRoot)
)
$ErrorActionPreference = 'Stop'
if($Case -notmatch '^[a-z]+/[a-z0-9-]+$'){ throw 'Case must be a domain/name identifier.' }
try { $word = [Runtime.InteropServices.Marshal]::GetActiveObject('Word.Application') }
catch {
    if($Action -notin 'Prepare','OpenReadOnly'){ throw }
    $word = New-Object -ComObject Word.Application
}
$word.Visible = $true
$doc = $null
foreach($candidate in $word.Documents) {
    try {
        if($candidate.Variables.Item('CorpusUICase').Value -eq $Case){ $doc=$candidate;break }
    } catch {}
}
if($Action -eq 'Prepare') {
    if($null -ne $doc){ throw 'This UI case is already open; inspect it before making another.' }
    $doc = $word.Documents.Add()
    $doc.Variables.Add('CorpusUICase',$Case) | Out-Null
    $doc.Content.Text = "before 前文`r`rafter 后文`r"
    $doc.Content.Font.Size = 11
    if($Case.StartsWith('ink/')) {
        $doc.Paragraphs.Item(2).Range.InsertBefore('这是一段用于观察原生墨迹的文字。')
        $doc.Paragraphs.Item(2).SpaceAfter = 210
    }
    $doc.Activate()
    $doc.ActiveWindow.View.Type = 3
    $doc.ActiveWindow.View.Zoom.Percentage = 110
}
if($Action -eq 'OpenReadOnly') {
    if($null -eq $doc) {
        $source=Join-Path $Root ($Case+'.docx')
        $doc=$word.Documents.Open([string]$source,$false,$true,$false)
        $doc.Variables.Add('CorpusUICase',$Case) | Out-Null
    }
    $doc.Activate()
    $doc.ActiveWindow.View.Type=3
    $doc.ActiveWindow.View.Zoom.Percentage=100
}
if($null -eq $doc){ throw "No tagged document is open for $Case" }
if($Action -eq 'ShowMarkup') {
    $doc.Activate()
    $view=$doc.ActiveWindow.View
    $view.ShowRevisionsAndComments=$true
    $view.ShowInsertionsAndDeletions=$true
    $view.ShowFormatChanges=$true
    $view.ShowComments=$true
    $view.RevisionsFilter.Markup=2
    $view.RevisionsFilter.View=0
    $view.RevisionsMode=0
    $pdf=Join-Path $Root ('_previews/'+$Case+'-markup.pdf')
    if(Test-Path -LiteralPath $pdf){ throw 'The markup PDF already exists.' }
    $doc.ExportAsFixedFormat([string]$pdf,17,$false,0,0,1,1,7)
}
if($Action -in 'Prepare','SelectBody') {
    $doc.Activate()
    $range=$doc.Paragraphs.Item(2).Range.Duplicate
    $range.Collapse(1)
    $range.Select()
}
if($Action -eq 'Save') {
    if($doc.Path -ne ''){ throw 'This helper saves newly authored UI cases once only.' }
    if($doc.Content.Text -notlike '*before 前文*' -or $doc.Content.Text -notlike '*after 后文*'){ throw 'Missing before/after markers.' }
    $domain,$name=$Case -split '/'
    $directory=Join-Path $Root $domain
    $pdfDirectory=Join-Path $Root "_previews/$domain"
    New-Item -ItemType Directory -Path $directory,$pdfDirectory -Force | Out-Null
    $path=Join-Path $directory "$name.docx"
    $number=2
    while(Test-Path -LiteralPath $path){ $path=Join-Path $directory "$name-$number.docx";$number++ }
    $doc.SaveAs2([string]$path,12)
    $pdf=Join-Path $pdfDirectory (([IO.Path]::GetFileNameWithoutExtension($path))+'.pdf')
    $doc.ExportAsFixedFormat([string]$pdf,17,$false)
}
if($Action -eq 'Close'){ $doc.Close(0); exit }
$objects=@()
foreach($shape in $doc.InlineShapes) {
    $item=[ordered]@{ placement='inline';type=$shape.Type;width=$shape.Width;height=$shape.Height;alt=$shape.AlternativeText }
    if($shape.HasChart -eq -1) {
        $chart=$shape.Chart
        $item.chartType=$chart.ChartType
        $item.title=if($chart.HasTitle){$chart.ChartTitle.Text}else{$null}
        $item.series=@()
        for($index=1;$index -le $chart.SeriesCollection().Count;$index++) {
            $series=$chart.SeriesCollection($index)
            $item.series+=@{name=$series.Name;x=@($series.XValues);values=@($series.Values)}
        }
    }
    $objects+=$item
}
foreach($shape in $doc.Shapes) {
    $objects+=@{placement='floating';type=$shape.Type;width=$shape.Width;height=$shape.Height;wrap=$shape.WrapFormat.Type;alt=$shape.AlternativeText}
}
$result=[ordered]@{action=$Action;case=$Case;file=$doc.FullName;version=$word.Version;build=$word.Build;text=$doc.Content.Text;objects=$objects;mathCount=$doc.OMaths.Count}
if($Action -eq 'Save') {
    $result.pdf=$pdf
    $result | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $PSScriptRoot (([IO.Path]::GetFileNameWithoutExtension($path))+'-ui-result.json')) -Encoding utf8
}
$result | ConvertTo-Json -Depth 12
