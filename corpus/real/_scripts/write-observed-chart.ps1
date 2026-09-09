#requires -Version 7.0
$ErrorActionPreference = 'Stop'
$report = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'chart-results.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$types = @{
    'chart-3d'='三维簇状柱形图'; 'chart-area'='面积图'; 'chart-bar'='簇状条形图'; 'chart-bubble'='气泡图'
    'chart-column'='簇状柱形图'; 'chart-combo'='组合图：系列1为簇状柱形，系列2为折线并使用次坐标轴'
    'chart-dates'='日期类别折线图'; 'chart-doughnut'='圆环图，双环，内径30%'; 'chart-floating'='簇状柱形图'
    'chart-in-table'='2×2表格左上单元格内的簇状柱形图'; 'chart-legend'='顶部图例簇状柱形图'
    'chart-line-plain'='无数据标记折线图'; 'chart-line'='带圆形数据标记折线图'; 'chart-no-legend'='无图例簇状柱形图'
    'chart-no-title'='无标题簇状柱形图'; 'chart-percent-stacked'='百分比堆积柱形图'; 'chart-pie'='饼图'
    'chart-point-color'='第二扇区红色的饼图'; 'chart-scatter-lines'='平滑线和标记散点图'; 'chart-scatter'='仅标记散点图'
    'chart-stacked'='堆积柱形图'; 'chart-style-gray'='灰度簇状柱形图'; 'chart-style'='单色簇状柱形图'
}
function RgbText([int]$oleRgb) {
    return '#{0:X2}{1:X2}{2:X2}' -f ($oleRgb -band 255),(($oleRgb -shr 8) -band 255),(($oleRgb -shr 16) -band 255)
}
function Cell([string]$value) { return $value.Replace('|','\|').Replace("`r",'').Replace("`n",'<br>') }
$version = '16.0.14334.20848 / Windows 11 x64（Office LTSC 2021，非 Microsoft 365）'
$observationMap = [ordered]@{}
$lines = [Collections.Generic.List[string]]::new()
$lines.Add('# 图表观察记录')
$lines.Add('')
$lines.Add('23份成功项已由主任务查看 Word 从创建实例导出的 PDF，核对类型、标题、图例和数据；原件仅保存一次。只读包检查同时验证了图表部件、关系、嵌入 xlsx、externalData 和保存后的数值缓存。具体 COM 回读、日志路径及 PDF 观察补充见 `chart-results.json`。')
$lines.Add('')
$lines.Add('| 文件 | Word 版本（build）/ 平台 | 制作方式（UI / 脚本名） | 步骤要点 | 看到什么 |')
$lines.Add('| --- | --- | --- | --- | --- |')
foreach ($item in $report.Generated) {
    $name = [IO.Path]::GetFileNameWithoutExtension($item.File)
    $actual = $item.Actual
    $categories = if ($name -eq 'chart-dates') { '2024/1/1、2024/2/1、2024/3/1（Word实际轴标签）' } else { $item.Worksheet.Categories -join '、' }
    $series = @($actual.Series | ForEach-Object { $_.Name + ' = [' + ($_.Values -join ', ') + ']' }) -join '；'
    $legend = if (-not $actual.HasLegend) { '无' } elseif ($actual.LegendPosition -eq -4160) { '顶部' } else { '右侧' }
    $title = if ($actual.HasTitle) { '“' + $actual.Title + '”' } else { '无标题' }
    $colors = '彩色，主题蓝/橙（颜色ID ' + $actual.ChartColor + '）'
    if ($name -in @('chart-pie','chart-doughnut','chart-point-color')) { $colors = '彩色，按类别扇区配色（蓝/橙/灰）' }
    if ($name -eq 'chart-point-color') { $colors += '；第二扇区单独红色 #FF0000' }
    if ($name -in @('chart-style','chart-style-gray')) {
        $label = if ($name -eq 'chart-style-gray') { '灰度' } else { '单色蓝' }
        $fills = @($actual.Series | ForEach-Object { RgbText $_.FillRgb }) -join '、'
        $colors = $label + '，系列填充 ' + $fills + '（颜色ID ' + $actual.ChartColor + '）'
    }
    $floating = if ($actual.Layout.Floating) { '是，正文右侧，四周型环绕' } else { '否，嵌入型' }
    $details = @($types[$name],('标题：'+$title),('类别：'+$categories),$series,('图例：'+$legend),('颜色：'+$colors),('样式ID：'+$actual.ChartStyle),('浮动：'+$floating))
    if ($name -eq 'chart-area') { $details += '橙色Series 2覆盖蓝色Series 1' }
    if ($name -eq 'chart-bubble') { $details += 'X值均为1、2、3；气泡大小Series 1=[3,6,9]，Series 2=[4,7,10]' }
    if ($name -eq 'chart-dates') { $details += 'COM格式回读m/d/yyyy，记录以Word实际显示为准' }
    $steps = 'Word原生插图；编辑嵌入工作簿；before 前文 / 图表 / after 后文；SaveAs2格式12保存一次；导出PDF并只读自检'
    $method = 'create-charts.ps1；run-charts-isolated.ps1（Word COM）'
    $observation = $details -join '；'
    $observationMap[$item.File] = $observation
    $lines.Add('| `' + $item.File + '` | ' + $version + ' | ' + $method + ' | ' + (Cell $steps) + ' | ' + (Cell $observation) + ' |')
}
foreach ($item in $report.NotGenerated) {
    $steps = '尝试AddChart2；再尝试先插柱形图并将ChartType设为' + $item.Requested.Type
    $observation = '未生成、未观察。直接AddChart2失败；转换图表类型再次失败：' + $item.Error.Message + '，' + $item.Error.HResult + '。这只说明当前COM入口失败，不表示Word UI不支持；待主任务UI尝试。'
    $lines.Add('| `' + $item.File + '` | ' + $version + ' | create-charts.ps1（COM尝试失败） | ' + (Cell $steps) + ' | ' + (Cell $observation) + ' |')
}
foreach ($paste in @(
    @('chart-pasted-embedded','Excel复制图表；Word粘贴“使用目标主题和嵌入工作簿”'),
    @('chart-pasted-linked','Excel复制图表；Word粘贴并选择“链接数据”'),
    @('chart-pasted-picture','Excel复制图表；Word粘贴为图片')
)) {
    $lines.Add('| `chart/' + $paste[0] + '.docx` | ' + $version + ' | 待UI制作 | ' + $paste[1] + ' | 未制作、未观察；待主任务UI流程。 |')
}
$output = Join-Path $PSScriptRoot 'observed-chart.md'
$lines | Set-Content -LiteralPath $output -Encoding UTF8
$observationMap | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'chart-observation-text.json') -Encoding UTF8
Write-Host ('Wrote ' + $output + ' (23 verified + 6 failed + 3 pending UI).')
