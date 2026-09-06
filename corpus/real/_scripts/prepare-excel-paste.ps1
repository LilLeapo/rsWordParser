param([string]$Root = (Split-Path -Parent $PSScriptRoot))
$ErrorActionPreference = 'Stop'
$directory = Join-Path $Root '_assets'
New-Item -ItemType Directory -Path $directory -Force | Out-Null
$path = Join-Path $directory 'chart-paste-source.xlsx'
if(Test-Path -LiteralPath $path){ throw 'Paste source exists; reopen the existing source in Excel.' }
$excel = New-Object -ComObject Excel.Application
$excel.Visible = $true
$book = $excel.Workbooks.Add()
$sheet = $book.Worksheets.Item(1)
$sheet.Name = 'ChartData'
$values = New-Object 'object[,]' 4,3
$values[0,0]='Category';$values[0,1]='Series 1';$values[0,2]='Series 2'
for($row=1;$row -le 3;$row++) {
    $values[$row,0]="Category $row"
    $values[$row,1]=[double]($row*10)
    $values[$row,2]=[double]($row*10+5)
}
$sheet.Range('A1:C4').Value2=$values
$sheet.Columns.Item('A:C').ColumnWidth=14
$object=$sheet.ChartObjects().Add(280,20,480,290)
$chart=$object.Chart
$chart.ChartType=51
$chart.SetSourceData($sheet.Range('A1:C4'),2)
$chart.HasTitle=$true
$chart.ChartTitle.Text='销售统计'
$chart.HasLegend=$true
$chart.Legend.Position=-4152
$book.SaveAs([string]$path,51)
$object.Activate()
[ordered]@{path=$path;workbook=$book.Name;title=$chart.ChartTitle.Text;chartType=$chart.ChartType;next='Use the Excel UI to copy the chart, then use actual Word paste options.'} | ConvertTo-Json
