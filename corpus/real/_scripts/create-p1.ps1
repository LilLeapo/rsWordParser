param(
    [string]$Root = (Split-Path -Parent $PSScriptRoot),
    [string[]]$Only = @()
)
$ErrorActionPreference = 'Stop'
$missing = [Type]::Missing
$cases = @('text/text-basic','text/text-custom-styles','table/table-styled','hf/hf-variants','sections/sections-three','fields/fields-toc','revisions/revisions-comments','shapes/textbox-shapes','sdt/content-controls','links/hyperlinks-bookmarks','strict/strict-basic','misc/large-report')
if ($Only.Count) { $cases = @($cases | Where-Object { ($_ -split '/')[-1] -in $Only -or $_ -in $Only }) }
$results = [Collections.Generic.List[object]]::new()
$runId = Get-Date -Format 'yyyyMMdd-HHmmss'
$logPath = Join-Path $PSScriptRoot "p1-results-$runId.json"

function UniquePath([string]$path) {
    $candidate = $path
    $number = 2
    while (Test-Path -LiteralPath $candidate) {
        $candidate = Join-Path (Split-Path $path) (([IO.Path]::GetFileNameWithoutExtension($path)) + "-$number" + [IO.Path]::GetExtension($path))
        $number++
    }
    return [string]$candidate
}
function BodyRange($doc) {
    $range = $doc.Paragraphs.Item(2).Range.Duplicate
    $range.Collapse(1)
    return $range
}
function FindRange($doc, [string]$text) {
    $range = $doc.Content.Duplicate
    if (-not $range.Find.Execute($text)) { throw "Text not found: $text" }
    return $range
}
function FillBody($doc, [string]$text) {
    $range = BodyRange $doc
    $range.InsertBefore($text + "`r")
}
function BasicText($doc) {
    FillBody $doc "一级标题`r二级标题`r这是正文 Mixed English and 中文。`r项目符号条目`r编号一级`r编号二级`r编号三级`r粗体 斜体 下划线 删除线 上标 彩色文字"
    $doc.Paragraphs.Item(2).Range.Style = -2
    $doc.Paragraphs.Item(3).Range.Style = -3
    $doc.Paragraphs.Item(5).Range.ListFormat.ApplyBulletDefault()
    $template = $doc.ListTemplates.Add($true)
    for ($level=1; $level -le 3; $level++) {
        $parts = for ($part=1; $part -le $level; $part++) { "%$part" }
        $template.ListLevels.Item($level).NumberFormat = ($parts -join '.') + '.'
        $template.ListLevels.Item($level).NumberStyle = 0
        $template.ListLevels.Item($level).NumberPosition = ($level-1)*18
        $template.ListLevels.Item($level).TextPosition = $level*18
        $template.ListLevels.Item($level).StartAt = 1
    }
    $numbered = $doc.Range($doc.Paragraphs.Item(6).Range.Start,$doc.Paragraphs.Item(8).Range.End)
    $numbered.ListFormat.ApplyListTemplateWithLevel($template,$false,2,0,1)
    for ($level=1; $level -le 3; $level++) {
        $doc.Paragraphs.Item(5+$level).Range.ListFormat.ListLevelNumber = $level
        if($doc.Paragraphs.Item(5+$level).Range.ListFormat.ListLevelNumber -ne $level){ throw "Failed to apply numbering level $level" }
    }
    (FindRange $doc '粗体').Font.Bold = -1
    (FindRange $doc '斜体').Font.Italic = -1
    (FindRange $doc '下划线').Font.Underline = 1
    (FindRange $doc '删除线').Font.StrikeThrough = -1
    (FindRange $doc '上标').Font.Superscript = -1
    $colored = FindRange $doc '彩色文字'
    $colored.Font.Color = 255
    $colored.Font.Size = 18
}

$word = $null
try {
    foreach ($case in $cases) {
        $word = New-Object -ComObject Word.Application
        $word.Visible = $true
        $doc = $null
        $entry = [ordered]@{ case=$case; status='started'; version=$word.Version; build=$word.Build; visualChecked=$false; started=(Get-Date).ToString('o') }
        $results.Add($entry)
        try {
            $domain,$name = $case -split '/'
            $directory = Join-Path $Root $domain
            $pdfDirectory = Join-Path $Root "_previews/$domain"
            New-Item -ItemType Directory -Path $directory,$pdfDirectory -Force | Out-Null
            $output = UniquePath (Join-Path $directory "$name.docx")
            $doc = $word.Documents.Add()
            if ($null -eq $doc) { throw 'Documents.Add returned null.' }
            $doc.Content.Text = "before 前文`r`rafter 后文`r"
            $doc.Content.Font.Size = 11
            switch ($name) {
                { $_ -in 'text-basic','strict-basic' } { BasicText $doc }
                'text-custom-styles' {
                    FillBody $doc "我的标题示例`r自定义标题后的正文"
                    $style = $doc.Styles.Add('我的标题',1)
                    $style.BaseStyle = $doc.Styles.Item(-2)
                    $style.Font.Name = 'Microsoft YaHei'
                    $style.Font.NameFarEast = 'Microsoft YaHei'
                    (FindRange $doc '我的标题示例').Style = $style
                }
                'table-styled' {
                    $table = $doc.Tables.Add((BodyRange $doc),3,3)
                    $tableStyle = @($doc.Styles | Where-Object { $_.NameLocal -in 'Grid Table 4 - Accent 1','网格表 4 - 着色 1','网格表格 4 - 强调文字颜色 1' }) | Select-Object -First 1
                    if($null -eq $tableStyle){ throw 'Required Grid Table 4 - Accent 1 style not found in this Word installation.' }
                    $table.Style = $tableStyle
                    $table.ApplyStyleHeadingRows = $true
                    $table.ApplyStyleRowBands = $true
                    for($r=1;$r -le 3;$r++){for($c=1;$c -le 3;$c++){ $table.Cell($r,$c).Range.Text="R$r C$c" }}
                    $table.Cell(1,1).Merge($table.Cell(1,2))
                    $cellRange = $table.Cell(3,3).Range.Duplicate
                    $cellRange.Collapse(1)
                    $nested = $doc.Tables.Add($cellRange,2,2)
                    for($r=1;$r -le 2;$r++){for($c=1;$c -le 2;$c++){ $nested.Cell($r,$c).Range.Text="N$r$c" }}
                }
                'hf-variants' {
                    FillBody $doc ("首页正文`r"+[char]12+"奇偶页正文`r"+[char]12+"第三页正文")
                    $doc.PageSetup.DifferentFirstPageHeaderFooter = -1
                    $doc.PageSetup.OddAndEvenPagesHeaderFooter = -1
                    $section = $doc.Sections.Item(1)
                    foreach($index in 1,2,3) {
                        $label = @('','奇数页','首页','偶数页')[$index]
                        $section.Headers.Item($index).Range.Text = "$label 页眉"
                        $footer = $section.Footers.Item($index).Range
                        $footer.Text = "$label 页脚 "
                        $footer.Collapse(0)
                        $doc.Fields.Add($footer,33) | Out-Null
                    }
                    $watermark = $section.Headers.Item(1).Shapes.AddTextEffect(0,'CORPUS','Arial',44,$false,$false,70,150)
                    $watermark.Rotation = 315
                    $watermark.WrapFormat.Type = 5
                    $watermark.Fill.ForeColor.RGB = 12632256
                    $watermark.Line.Visible = 0
                }
                'sections-three' {
                    FillBody $doc "第一节正文`r第二节正文`r第三节正文"
                    foreach($text in @('第三节正文','第二节正文')) {
                        $range = FindRange $doc $text
                        $range.Collapse(1)
                        $range.InsertBreak(2)
                    }
                    if($doc.Sections.Count -ne 3){throw 'Expected three sections.'}
                    for($index=1;$index -le 3;$index++) {
                        $section = $doc.Sections.Item($index)
                        $section.PageSetup.TopMargin = 36 + $index*9
                        $section.PageSetup.BottomMargin = 36 + $index*9
                        $section.PageSetup.LeftMargin = 36 + $index*9
                        $section.PageSetup.RightMargin = 36 + $index*9
                    }
                    $doc.Sections.Item(2).PageSetup.Orientation = 1
                    $doc.Sections.Item(3).PageSetup.TextColumns.SetCount(2)
                    $doc.Sections.Item(1).Headers.Item(1).Range.Text = '第一节页眉'
                    $doc.Sections.Item(2).Headers.Item(1).LinkToPrevious = $false
                    $doc.Sections.Item(2).Headers.Item(1).Range.Text = '第二节独立页眉'
                }
                'fields-toc' {
                    FillBody $doc "目录`r一级章节`r二级章节`r三级章节`r交叉引用：`r脚注位置 尾注位置 日期位置"
                    foreach($pair in @(@('一级章节',-2),@('二级章节',-3),@('三级章节',-4))){ (FindRange $doc $pair[0]).Style=$pair[1] }
                    $heading = FindRange $doc '一级章节'
                    $doc.Bookmarks.Add('ChapterOne',$heading) | Out-Null
                    $refRange = FindRange $doc '交叉引用：'
                    $refRange.Collapse(0)
                    $doc.Fields.Add($refRange,-1,'REF ChapterOne \h') | Out-Null
                    $doc.Footnotes.Add((FindRange $doc '脚注位置'),$missing,'这是一条脚注。') | Out-Null
                    $doc.Endnotes.Add((FindRange $doc '尾注位置'),$missing,'这是一条尾注。') | Out-Null
                    $doc.Fields.Add((FindRange $doc '日期位置'),31) | Out-Null
                    $tocRange = FindRange $doc '目录'
                    $tocRange.Collapse(0)
                    $tocRange.InsertParagraphAfter()
                    $tocRange.Collapse(0)
                    $toc = $doc.TablesOfContents.Add($tocRange,$true,1,3)
                    $toc.Update()
                }
                'revisions-comments' {
                    FillBody $doc "原始正文。`r待删除句子。`r格式修改文字。`r批注位置一。批注位置二。"
                    $doc.TrackFormatting = $true
                    $doc.TrackRevisions = $true
                    (FindRange $doc '原始正文。').InsertAfter('新增句子。')
                    (FindRange $doc '待删除句子。').Delete() | Out-Null
                    (FindRange $doc '格式修改文字。').Font.Bold = -1
                    $doc.TrackRevisions = $false
                    $comment = $doc.Comments.Add((FindRange $doc '批注位置一'),'第一条批注')
                    $comment.Replies.Add((FindRange $doc '批注位置一'),'第一条批注的回复') | Out-Null
                    $comment.Done = $true
                    $doc.Comments.Add((FindRange $doc '批注位置二'),'第二条未解决批注') | Out-Null
                    $entry.revisions = $doc.Revisions.Count
                    $entry.comments = $doc.Comments.Count
                }
                'textbox-shapes' {
                    $doc.Content.Text = "`rafter 后文`r"
                    $anchor = $doc.Paragraphs.Item(1).Range.Duplicate
                    $anchor.Collapse(1)
                    $textbox = $doc.Shapes.AddTextbox(1,30,25,210,70,$anchor)
                    $textbox.TextFrame.TextRange.Text = "文本框第一段`r文本框第二段"
                    $round = $doc.Shapes.AddShape(5,270,25,150,70,$anchor)
                    $round.TextFrame.TextRange.Text = '圆角矩形'
                    $one = $doc.Shapes.AddShape(1,30,125,85,55,$anchor)
                    $two = $doc.Shapes.AddShape(9,125,125,85,55,$anchor)
                    $anchor.Select()
                    $group = $doc.Shapes.Range(@($one.Name,$two.Name)).Group()
                    $entry.groupAnchorAfterGrouping = [int]$group.Anchor.Start
                    $anchor = $doc.Paragraphs.Item(1).Range.Duplicate
                    $anchor.Collapse(1)
                    $art = $doc.Shapes.AddTextEffect(0,'艺术字','Arial',28,$false,$false,30,220,$anchor)
                    $doc.Paragraphs.Item(1).SpaceAfter = 300
                    $doc.Paragraphs.Item(1).Range.InsertParagraphBefore()
                    $doc.Paragraphs.Item(1).Range.Text = "before 前文`r"
                    $doc.Paragraphs.Item(1).SpaceBefore = 0
                    $doc.Paragraphs.Item(1).SpaceAfter = 0
                    $doc.Paragraphs.Item(2).SpaceAfter = 300
                    $featureStart = [int]$doc.Paragraphs.Item(2).Range.Start
                    $entry.featureParagraphStart = $featureStart
                    $placements = @(
                        @{ label='textbox'; shape=$textbox; left=30; top=25 },
                        @{ label='rounded-rectangle'; shape=$round; left=270; top=25 },
                        @{ label='group'; shape=$group; left=30; top=125 },
                        @{ label='wordart'; shape=$art; left=30; top=220 }
                    )
                    $layout = [Collections.Generic.List[object]]::new()
                    $entry.shapeLayout = $layout
                    foreach($placement in $placements) {
                        $shape = $placement.shape
                        $anchorBeforeRepair = [int]$shape.Anchor.Start
                        if($anchorBeforeRepair -ne $featureStart) {
                            # Grouping can move an anchor; use Word's own clipboard to relocate it.
                            $shape.Select()
                            $word.Selection.Cut()
                            $feature = $doc.Paragraphs.Item(2).Range.Duplicate
                            $feature.Collapse(1)
                            $feature.Select()
                            $word.Selection.Paste()
                            if($word.Selection.ShapeRange.Count -ne 1) { throw "Expected one pasted shape for $($placement.label)." }
                            $shape = $word.Selection.ShapeRange.Item(1)
                            $placement.shape = $shape
                        }
                        $shape.RelativeHorizontalPosition = 0
                        $shape.RelativeVerticalPosition = 2
                        $shape.WrapFormat.Type = 3
                        $shape.Left = $placement.left
                        $shape.Top = $placement.top
                        $shape.LockAnchor = -1
                        $layout.Add([ordered]@{
                            object=$placement.label; name=$shape.Name; type=[int]$shape.Type
                            anchorBeforeRepair=$anchorBeforeRepair; anchorStart=[int]$shape.Anchor.Start
                            anchorParagraphStart=[int]$shape.Anchor.Paragraphs.Item(1).Range.Start
                            relativeHorizontalPosition=[int]$shape.RelativeHorizontalPosition
                            relativeVerticalPosition=[int]$shape.RelativeVerticalPosition
                            left=[double]$shape.Left; top=[double]$shape.Top
                        })
                    }
                    if($doc.Shapes.Count -ne 4) { throw "Expected four top-level shapes, found $($doc.Shapes.Count)." }
                    if($placements[2].shape.Type -ne 6 -or $placements[2].shape.GroupItems.Count -ne 2) { throw 'Expected a group containing the rectangle and ellipse.' }
                    foreach($item in $layout) {
                        if($item.anchorStart -ne $featureStart -or $item.anchorParagraphStart -ne $featureStart) {
                            throw "Shape $($item.object) is anchored at $($item.anchorStart), expected feature paragraph $featureStart."
                        }
                    }
                }
                'content-controls' {
                    FillBody $doc "富文本：`r下拉列表：`r日期：`r复选框："
                    foreach($pair in @(@('富文本：',0),@('下拉列表：',4),@('日期：',6),@('复选框：',8))) {
                        $range = FindRange $doc $pair[0]
                        $range.Collapse(0)
                        $control = $doc.ContentControls.Add($pair[1],$range)
                        $control.Title = $pair[0]
                        switch($pair[1]) {
                            0 { $control.Range.Text='已锁定富文本';$control.LockContents=$true;$control.LockContentControl=$true }
                            4 { $control.DropdownListEntries.Add('选项一') | Out-Null;$control.DropdownListEntries.Add('选项二') | Out-Null;$control.DropdownListEntries.Item(2).Select() }
                            6 { $control.DateDisplayFormat='yyyy-MM-dd';$control.Range.Text='2026-09-07' }
                            8 { $control.Checked=$true }
                        }
                    }
                }
                'hyperlinks-bookmarks' {
                    FillBody $doc "书签目标`r外部链接`r返回书签`r邮件链接"
                    $doc.Bookmarks.Add('Target',(FindRange $doc '书签目标')) | Out-Null
                    $doc.Hyperlinks.Add((FindRange $doc '外部链接'),'https://example.com') | Out-Null
                    $doc.Hyperlinks.Add((FindRange $doc '返回书签'),'','Target') | Out-Null
                    $doc.Hyperlinks.Add((FindRange $doc '邮件链接'),'mailto:test@example.com') | Out-Null
                }
                'large-report' {
                    $sources = @()
                    foreach($sourceDomain in @('table','chart','smartart','math','canvas','image','text')) {
                        $sourceDirectory = Join-Path $Root $sourceDomain
                        if(Test-Path -LiteralPath $sourceDirectory) {
                            $sources += Get-ChildItem -LiteralPath $sourceDirectory -Filter '*.docx' | Where-Object { $_.Name -notlike '~$*' -and $_.BaseName -notmatch '-\d+$' } | Sort-Object Name
                        }
                    }
                    $sources = @($sources | Select-Object -First 24)
                    if($sources.Count -lt 20){throw 'At least twenty existing Word corpus documents are required for the actual corpus report.'}
                    $doc.Content.Text = "before 前文`rWindows Word 语料验证报告`r2026-09-07`r本报告汇集本次由桌面 Word 制作的实际语料，用于检查图表、表格、目录和分页。Office LTSC 2021，build 16.0.14334.20848；不代表 Microsoft 365 的验证结果。`r目录`r"
                    $doc.Paragraphs.Item(2).Range.Style = -63
                    $tocRange = $doc.Range($doc.Content.End-1,$doc.Content.End-1)
                    $toc = $doc.TablesOfContents.Add($tocRange,$true,1,1)
                    $range = $doc.Range($doc.Content.End-1,$doc.Content.End-1)
                    $range.InsertBreak(7)
                    $summaryStart = $doc.Content.End-1
                    $doc.Range($summaryStart,$summaryStart).InsertBefore("验证方法与往返结果`r本报告记录对真实桌面 Word 文件形态的检查。样本由 Windows Word 自己创建和保存；原始输入不改写，结构检查在副本上进行。图表的显示还与实际内嵌 Excel 数据核对。`r任务 B 共检查 9 份输入：3 份正常显示，2 份需要恢复，4 份普通打开失败。已经产生 5 份 Word 另存副本。9 份原件的 SHA256 与提供的压缩包一致。`r05-ink-insert 的 PNG 内容类型声明重复；06-chart-insert-line-pie 的 XLSX 声明重复，两份在恢复后可显示。02 两份图表引用了未定义的轴且没有内嵌工作簿，无法执行旧数据刷新实验。04 两份图片缺少必需结构，媒体字节彼此相同。详尽错误提示和截图见随附 ROUNDTRIP.md。`r局限：本次运行于 Office LTSC Professional Plus 2021，16.0.14334.20848，Windows 11 build 22631。不能据此声称通过 Microsoft 365、macOS Word 或 WPS 的验证。`r下列章节保留本次实际生成的图表与表格，并记录检查时的显示结果。普通图表统一使用三个类别，Series 1 为10、20、30，Series 2 为15、25、35；散点、气泡和日期轴案例使用各自记录的数据。`r")
                    $doc.Range($summaryStart,$summaryStart+10).Paragraphs.Item(1).Range.Style = -2
                    $doc.Range($doc.Content.End-1,$doc.Content.End-1).InsertBefore("饼图是单系列例外：实际显示的只有 Series 1，三个扇区数值为10、20、30。面积图中较高的橙色系列覆盖蓝色系列；日期图显示标签为2024/1/1、2024/2/1、2024/3/1。`r")
                    $observationPath = Join-Path $PSScriptRoot 'chart-observation-text.json'
                    $observations = if(Test-Path -LiteralPath $observationPath){ Get-Content -Raw -LiteralPath $observationPath | ConvertFrom-Json }else{$null}
                    foreach($source in $sources) {
                        $range = $doc.Range($doc.Content.End-1,$doc.Content.End-1)
                        $range.InsertBreak(7)
                        $range = $doc.Range($doc.Content.End-1,$doc.Content.End-1)
                        $range.InsertBefore($source.BaseName + "`r")
                        $doc.Paragraphs.Item($doc.Paragraphs.Count-1).Range.Style = -2
                        $range = $doc.Range($doc.Content.End-1,$doc.Content.End-1)
                        $key = (Split-Path (Split-Path $source.FullName) -Leaf) + '/' + $source.Name
                        $observation = if($null -ne $observations){ $observations.PSObject.Properties[$key].Value }else{$null}
                        if($observation) {
                            $range.InsertBefore("观察记录：$observation`r")
                            $range = $doc.Range($doc.Content.End-1,$doc.Content.End-1)
                        }
                        $range.InsertFile($source.FullName)
                    }
                    $doc.Range($doc.Content.End-1,$doc.Content.End-1).InsertAfter("`rafter 后文`r")
                    $doc.Sections.Item(1).Headers.Item(1).Range.Text = 'Windows Word 语料验证报告 / 2026-09-07'
                    $footer = $doc.Sections.Item(1).Footers.Item(1).Range
                    $doc.Fields.Add($footer,33) | Out-Null
                    $doc.Repaginate()
                    $toc.Update()
                    $doc.Repaginate()
                    $toc.UpdatePageNumbers()
                    $entry.sourceDocuments = @($sources | ForEach-Object FullName)
                    $entry.reportOrigin = 'A report authored in desktop Word from this session''s real Word corpus documents; not a downloaded public report.'
                    if($doc.ComputeStatistics(2) -lt 21){throw 'The corpus report is shorter than twenty-one pages.'}
                    if($doc.Tables.Count -lt 1){throw 'The corpus report has no table.'}
                }
            }
            if ($doc.Content.Text -notlike '*before 前文*' -or $doc.Content.Text -notlike '*after 后文*') { throw 'Missing before/after markers.' }
            $doc.Repaginate()
            $entry.pages = $doc.ComputeStatistics(2)
            $entry.text = $doc.Content.Text
            $format = if($name -eq 'strict-basic'){24}else{12}
            $doc.SaveAs2([string]$output,$format)
            $entry.file = $output
            $entry.status = 'saved-needs-visual-and-package-check'
            $pdf = Join-Path $pdfDirectory (([IO.Path]::GetFileNameWithoutExtension($output))+'.pdf')
            try { $doc.ExportAsFixedFormat([string]$pdf,17,$false); $entry.pdf=$pdf } catch { $entry.pdfError=$_.Exception.Message }
            Write-Output "$case : $($entry.status)"
        } catch { $entry.status='failed';$entry.error=$_.Exception.Message;$entry.trace=$_.ScriptStackTrace;Write-Warning "$case : $($entry.error)" }
        finally {
            if($null -ne $doc){try{$doc.Close(0)}catch{$entry.closeError=$_.Exception.Message}}
            if($null -ne $word){try{$word.Quit(0)}catch{$entry.quitError=$_.Exception.Message}}
            $word = $null
            $entry.finished=(Get-Date).ToString('o')
            $results | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $logPath -Encoding utf8
        }
    }
} finally { if($null -ne $word){try{$word.Quit(0)}catch{Write-Warning $_.Exception.Message}} }
Write-Output "Log: $logPath"
