#requires -Version 7.0
[CmdletBinding()]
param(
    [string]$OutputRoot = (Split-Path -Parent $PSScriptRoot),
    [string[]]$Only = @(),
    [bool]$ShowWord = $true,
    [switch]$ExportPdf
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$OutputRoot = [IO.Path]::GetFullPath($OutputRoot)
$script:Rows = [Collections.Generic.List[object]]::new()
$script:Word = $null
$script:Document = $null
$script:WordOwned = $false
$script:WordProcessId = $null
$script:MathParser = 'unknown'
$script:Details = [ordered]@{}
$script:AssetPath = Join-Path $OutputRoot '_assets/tiny.png'
$logPath = Join-Path $OutputRoot '_scripts/smartart-math-results.json'
$runId = [DateTimeOffset]::Now.ToString('o')
foreach ($folder in @('_scripts', '_assets', 'smartart', 'math')) {
    [void][IO.Directory]::CreateDirectory((Join-Path $OutputRoot $folder))
}
if (Test-Path -LiteralPath $logPath) {
    $previous = Get-Content -LiteralPath $logPath -Raw -Encoding utf8 | ConvertFrom-Json
    foreach ($row in $previous.cases) { $script:Rows.Add($row) }
}

function Get-UnusedPath([string]$RelativePath) {
    $candidate = Join-Path $OutputRoot $RelativePath
    if (-not (Test-Path -LiteralPath $candidate)) { return $candidate }
    $directory = [IO.Path]::GetDirectoryName($candidate)
    $stem = [IO.Path]::GetFileNameWithoutExtension($candidate)
    $extension = [IO.Path]::GetExtension($candidate)
    for ($suffix = 2; ; $suffix++) {
        $candidate = Join-Path $directory "$stem-$suffix$extension"
        if (-not (Test-Path -LiteralPath $candidate)) { return $candidate }
    }
}

function Write-Results {
    $result = [ordered]@{
        generatedAt = [DateTimeOffset]::Now.ToString('o')
        source = 'Windows desktop Word COM; no package/XML editing'
        evidence = 'Object-model observations only. Visual inspection and copied-package self-check are separate required steps.'
        cases = @($script:Rows.ToArray())
    }
    $result | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $logPath -Encoding utf8
}

function Get-TinyPng {
    if (Test-Path -LiteralPath $script:AssetPath) { return $script:AssetPath }
    Add-Type -AssemblyName System.Drawing
    $bitmap = [Drawing.Bitmap]::new(64, 32)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.Clear([Drawing.Color]::FromArgb(36, 146, 126))
        $graphics.FillRectangle([Drawing.Brushes]::Gold, 32, 0, 32, 32)
        $bitmap.Save($script:AssetPath, [Drawing.Imaging.ImageFormat]::Png)
    }
    finally { $graphics.Dispose(); $bitmap.Dispose() }
    return $script:AssetPath
}

function Get-FeatureRange {
    $range = $script:Document.Paragraphs.Item(2).Range.Duplicate
    $range.Collapse(1)
    return ,$range
}

function Initialize-WordInstance {
    $script:WordOwned = $false
    $script:WordProcessId = $null
    if (-not ('CorpusWordNative' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CorpusWordNative {
    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
}
'@
    }
    $existingPids = @(Get-Process -Name WINWORD -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)
    $script:Word = New-Object -ComObject Word.Application
    $probe = $script:Word.Documents.Add()
    try {
        $owner = [uint32]0
        [void][CorpusWordNative]::GetWindowThreadProcessId([IntPtr]$probe.Windows.Item(1).Hwnd, [ref]$owner)
        if ($owner -eq 0 -or [int]$owner -in $existingPids) {
            throw "The COM document belongs to pre-existing or unidentifiable Word PID $owner; refusing to control that application."
        }
        $script:WordProcessId = [int]$owner
        $script:WordOwned = $true
        $script:Word.Visible = $ShowWord
        $script:Word.DisplayAlerts = 0
        $script:Word.AutomationSecurity = 3
        $script:Word.Options.SaveNormalPrompt = $false
        foreach ($syntax in @(
            @{ Name = 'UnicodeMath'; Text = '1/2' },
            @{ Name = 'LaTeX'; Text = '\frac{1}{2}' }
        )) {
            $probe.Content.Text = $syntax.Text
            $range = $probe.Content.Duplicate
            $range.End--
            $mathRange = $probe.OMaths.Add($range)
            $math = $mathRange.OMaths.Item(1)
            $math.BuildUp()
            if (7 -in @(Get-FunctionTypes $math)) { $script:MathParser = $syntax.Name; break }
        }
        if ($script:MathParser -eq 'unknown') { throw 'Neither UnicodeMath nor LaTeX produced a real fraction in the unsaved parser probe.' }
    }
    finally {
        if ($null -ne $probe) { $probe.Close(0); [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($probe) }
    }
}

function Assert-MarkerParagraphs {
    $beforeMarkers = [Collections.Generic.List[object]]::new()
    $afterMarkers = [Collections.Generic.List[object]]::new()
    for ($i = 1; $i -le $script:Document.Paragraphs.Count; $i++) {
        $range = $script:Document.Paragraphs.Item($i).Range
        $text = ([string]$range.Text).Trim([char[]]@(13, 7))
        $position = [pscustomobject]@{ paragraph = $i; start = [int]$range.Start; end = [int]$range.End }
        if ($text -ceq 'before 前文') { $beforeMarkers.Add($position) }
        if ($text -ceq 'after 后文') { $afterMarkers.Add($position) }
    }
    if ($beforeMarkers.Count -ne 1 -or $afterMarkers.Count -ne 1) {
        throw 'The document must retain one standalone before paragraph and one standalone after paragraph.'
    }
    if ($beforeMarkers[0].end -ge $afterMarkers[0].start) {
        throw 'There must be feature content between the before and after paragraphs.'
    }
    $script:Details['markerParagraphs'] = [ordered]@{ before = $beforeMarkers[0]; after = $afterMarkers[0] }
}

function Get-CellFeatureRange {
    $table = $script:Document.Tables.Add((Get-FeatureRange), 2, 2)
    $table.Borders.Enable = 1
    $table.Cell(1, 2).Range.Text = '右上'
    $table.Cell(2, 1).Range.Text = '左下'
    $table.Cell(2, 2).Range.Text = '右下'
    $range = $table.Cell(1, 1).Range.Duplicate
    $range.Collapse(1)
    return ,$range
}

function Find-Layout([string[]]$Names, [string[]]$IdFragments = @()) {
    $layouts = $script:Word.SmartArtLayouts
    $inventory = [Collections.Generic.List[object]]::new()
    for ($i = 1; $i -le $layouts.Count; $i++) {
        $layout = $layouts.Item($i)
        $inventory.Add([pscustomobject]@{ Name = [string]$layout.Name; Id = [string]$layout.Id })
        if ([string]$layout.Name -in $Names) { return ,$layout }
    }
    foreach ($fragment in $IdFragments) {
        for ($i = 1; $i -le $layouts.Count; $i++) {
            $layout = $layouts.Item($i)
            if ([string]$layout.Id -match $fragment) { return ,$layout }
        }
    }
    $inventory | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $OutputRoot '_scripts/smartart-layouts.json') -Encoding utf8
    throw "Requested SmartArt layout not found: $($Names -join ', '). Available layouts written to smartart-layouts.json."
}

function Add-Diagram([string]$Kind, $Range = $null) {
    $layout = switch ($Kind) {
        'list' { Find-Layout @('基本列表', '基本块列表', 'Basic Block List', 'Basic List') @('/blockList$', '/basicBlockList$') }
        'hierarchy' { Find-Layout @('组织结构图', 'Organization Chart') @('/orgChart$') }
        'process' { Find-Layout @('基本流程', 'Basic Process') @('/process$', '/basicProcess$') }
        'cycle' { Find-Layout @('基本循环', 'Basic Cycle') @('/cycle$', '/basicCycle$') }
        'picture' { Find-Layout @('图片列表', 'Picture List', '垂直图片列表', 'Vertical Picture List', '图片题注列表', 'Picture Caption List') @('/pictureList$', '/pictureCaptionList$') }
        default { throw "Unknown layout kind: $Kind" }
    }
    if ($null -eq $Range) { $Range = Get-FeatureRange }
    $inline = $script:Document.InlineShapes.AddSmartArt($layout, $Range)
    $inline.Width = 420
    $inline.Height = if ($Kind -eq 'hierarchy') { 260 } else { 200 }
    return [pscustomobject]@{ Inline = $inline; Art = $inline.SmartArt }
}

function Set-FlatNodes($Art, [string[]]$Texts) {
    while ($Art.AllNodes.Count -gt 1) { $Art.AllNodes.Item($Art.AllNodes.Count).Delete() }
    if ($Art.AllNodes.Count -ne 1) { throw 'Layout did not retain a root node.' }
    $node = $Art.AllNodes.Item(1)
    $node.TextFrame2.TextRange.Text = $Texts[0]
    for ($i = 1; $i -lt $Texts.Count; $i++) {
        # msoSmartArtNodeAfter = 2; msoSmartArtNodeTypeDefault = 1.
        $node = $node.AddNode(2, 1)
        $node.TextFrame2.TextRange.Text = $Texts[$i]
    }
    if ($Art.AllNodes.Count -ne $Texts.Count) { throw 'Unexpected SmartArt node count.' }
}

function Record-Diagram($Art, [string]$Connection, [bool]$Floating = $false) {
    $nodes = [Collections.Generic.List[object]]::new()
    for ($i = 1; $i -le $Art.AllNodes.Count; $i++) {
        $node = $Art.AllNodes.Item($i)
        $nodes.Add([pscustomobject]@{
            readingOrder = $i
            level = [int]$node.Level
            nodeType = [int]$node.Type
            text = [string]$node.TextFrame2.TextRange.Text
        })
    }
    $script:Details['layoutName'] = [string]$Art.Layout.Name
    $script:Details['layoutId'] = [string]$Art.Layout.Id
    $script:Details['colorName'] = [string]$Art.Color.Name
    $script:Details['colorId'] = [string]$Art.Color.Id
    $script:Details['quickStyleName'] = [string]$Art.QuickStyle.Name
    $script:Details['quickStyleId'] = [string]$Art.QuickStyle.Id
    $script:Details['nodesInObjectModelReadingOrder'] = @($nodes.ToArray())
    $script:Details['expectedConnectionAppearance'] = $Connection
    $script:Details['floating'] = $Floating
}

function Add-Equation($Range, [string]$Linear, [bool]$Inline = $false, [bool]$BuildUp = $true) {
    if ($script:MathParser -eq 'LaTeX') {
        $Linear = switch ($Linear) {
            '(a+b)/2+x^2' { '\frac{a+b}{2}+x^2' }
            '[■(a&b@c&d)]' { '\left[\matrix{a&b\\c&d}\right]' }
            '1/2 ∑_(i=1)^n x_i' { '\frac{1}{2}\sum_{i=1}^{n} x_i' }
            default { $Linear }
        }
    }
    $script:Details['detectedMathParser'] = $script:MathParser
    $script:Details['actualMathInput'] = $Linear
    $start = [int]$Range.Start
    $Range.Text = $Linear
    $Range.SetRange($start, $start + $Linear.Length)
    $mathRange = $script:Document.OMaths.Add($Range)
    $math = $mathRange.OMaths.Item(1)
    $math.Type = if ($Inline) { 1 } else { 0 }
    if ($BuildUp) { $math.BuildUp() }
    return ,$math
}

function Get-FunctionTypes($Math) {
    $types = [Collections.Generic.List[int]]::new()
    for ($i = 1; $i -le $Math.Functions.Count; $i++) { $types.Add([int]$Math.Functions.Item($i).Type) }
    return @($types.ToArray())
}

function Require-Function($Math, [int]$Type, [string]$Description) {
    if ($Type -notin @(Get-FunctionTypes $Math)) { throw "Word did not build the requested $Description structure (function type $Type)." }
}

function Record-Math([string]$Reading, [string]$Placement, [string]$Appearance = 'Word default equation color and font size') {
    $maths = [Collections.Generic.List[object]]::new()
    for ($i = 1; $i -le $script:Document.OMaths.Count; $i++) {
        $math = $script:Document.OMaths.Item($i)
        $maths.Add([pscustomobject]@{
            index = $i
            type = [int]$math.Type
            text = [string]$math.Range.Text
            start = [int]$math.Range.Start
            end = [int]$math.Range.End
            fontName = [string]$math.Range.Font.Name
            fontSize = [single]$math.Range.Font.Size
            justification = [int]$math.Justification
            topLevelFunctionTypes = @(Get-FunctionTypes $math)
        })
    }
    $script:Details['reading'] = $Reading
    $script:Details['placement'] = $Placement
    $script:Details['appearance'] = $Appearance
    $script:Details['equations'] = @($maths.ToArray())
}

function Find-BuiltinEquation([string[]]$Names) {
    $script:Word.Templates.LoadBuildingBlocks()
    for ($t = 1; $t -le $script:Word.Templates.Count; $t++) {
        $template = $script:Word.Templates.Item($t)
        for ($i = 1; $i -le $template.BuildingBlockEntries.Count; $i++) {
            $entry = $template.BuildingBlockEntries.Item($i)
            if (([string]$entry.Name).Trim() -in $Names) { return ,$entry }
        }
    }
    throw "Built-in Word equation not found: $($Names -join ', '). No synthesized substitute was used."
}

$cases = @(
    @{ Name = 'smartart/smartart-list.docx'; Steps = '基本列表，五个同级节点'; Action = {
        $diagram = Add-Diagram 'list'
        Set-FlatNodes $diagram.Art @('第一项', '第二项', '第三项', '第四项', '第五项')
        Record-Diagram $diagram.Art '基本列表，无流程连线'
    } },
    @{ Name = 'smartart/smartart-hierarchy.docx'; Steps = '组织结构图，总经理下设部门A、部门B，部门A下设组1，总经理另有助理'; Action = {
        $diagram = Add-Diagram 'hierarchy'
        Set-FlatNodes $diagram.Art @('总经理')
        $root = $diagram.Art.AllNodes.Item(1)
        $departmentA = $root.AddNode(5, 1)
        $departmentA.TextFrame2.TextRange.Text = '部门A'
        $departmentB = $departmentA.AddNode(2, 1)
        $departmentB.TextFrame2.TextRange.Text = '部门B'
        $group = $departmentA.AddNode(5, 1)
        $group.TextFrame2.TextRange.Text = '组1'
        try { $assistant = $root.AddNode(5, 2) }
        catch { $assistant = $root.AddNode(1, 2) }
        $assistant.TextFrame2.TextRange.Text = '助理'
        if ($diagram.Art.AllNodes.Count -ne 5 -or $assistant.Type -ne 2) { throw 'Organization chart must contain five nodes, including a real assistant node.' }
        if ($assistant.ParentNode.TextFrame2.TextRange.Text -ne '总经理') { throw 'The assistant node is not attached to the general manager.' }
        $script:Details['hierarchy'] = @('总经理 > 部门A > 组1', '总经理 > 部门B', '总经理 > 助理（助理节点）')
        Record-Diagram $diagram.Art '组织结构图直线连接；助理为专用助理节点'
    } },
    @{ Name = 'smartart/smartart-process.docx'; Steps = '基本流程，三个步骤'; Action = {
        $diagram = Add-Diagram 'process'
        Set-FlatNodes $diagram.Art @('准备', '执行', '完成')
        Record-Diagram $diagram.Art '基本流程直线箭头'
    } },
    @{ Name = 'smartart/smartart-cycle.docx'; Steps = '基本循环，三个节点'; Action = {
        $diagram = Add-Diagram 'cycle'
        Set-FlatNodes $diagram.Art @('计划', '实施', '检查')
        Record-Diagram $diagram.Art '基本循环弧形箭头'
    } },
    @{ Name = 'smartart/smartart-picture.docx'; Steps = '图片列表，两个节点，各一个小PNG图片填充'; Action = {
        $diagram = Add-Diagram 'picture'
        Set-FlatNodes $diagram.Art @('图片一', '图片二')
        $picture = Get-TinyPng
        $fills = [Collections.Generic.List[object]]::new()
        for ($i = 1; $i -le $diagram.Art.AllNodes.Count; $i++) {
            $node = $diagram.Art.AllNodes.Item($i)
            $candidates = [Collections.Generic.List[object]]::new()
            for ($j = 1; $j -le $node.Shapes.Count; $j++) {
                $shape = $node.Shapes.Item($j)
                if ($shape.Type -eq 9) { continue }
                $hasText = $false
                try { $hasText = ($shape.TextFrame2.HasText -eq -1) } catch { }
                if (-not $hasText) { $candidates.Add($shape) }
            }
            if ($candidates.Count -ne 1) { throw "Picture node $i has $($candidates.Count) candidate image shapes; the placeholder cannot be identified reliably." }
            $pictureShape = $candidates[0]
            $pictureShape.Fill.UserPicture($picture)
            $fills.Add([pscustomobject]@{ node = $i; shapeName = [string]$pictureShape.Name; fillType = [int]$pictureShape.Fill.Type })
        }
        $script:Details['pictureFills'] = @($fills.ToArray())
        Record-Diagram $diagram.Art '图片列表，无流程连线'
    } },
    @{ Name = 'smartart/smartart-styled.docx'; Steps = '基本流程，彩色范围，三维SmartArt样式'; Action = {
        $diagram = Add-Diagram 'process'
        Set-FlatNodes $diagram.Art @('准备', '执行', '完成')
        $chosenColor = $null
        for ($i = 1; $i -le $script:Word.SmartArtColors.Count; $i++) {
            $color = $script:Word.SmartArtColors.Item($i)
            if ([string]$color.Name -match '彩色范围|Colorful Range|Colorful -') { $chosenColor = $color; break }
        }
        $chosenStyle = $null
        for ($i = 1; $i -le $script:Word.SmartArtQuickStyles.Count; $i++) {
            $style = $script:Word.SmartArtQuickStyles.Item($i)
            if ([string]$style.Name -match 'Polished|Inset|Metallic|精致|凹入|金属') { $chosenStyle = $style; break }
        }
        if ($null -eq $chosenColor -or $null -eq $chosenStyle) { throw 'A named colorful range and a recognized 3D style are both required.' }
        $diagram.Art.Color = $chosenColor
        $diagram.Art.QuickStyle = $chosenStyle
        Record-Diagram $diagram.Art '基本流程直线箭头；三维效果'
    } },
    @{ Name = 'smartart/smartart-floating.docx'; Steps = '浮动四周型SmartArt及同段锚定的浮动图片'; Action = {
        $diagram = Add-Diagram 'process'
        Set-FlatNodes $diagram.Art @('准备', '执行', '完成')
        $pictureRange = $diagram.Inline.Range.Duplicate
        $pictureRange.Collapse(0)
        $inlinePicture = $script:Document.InlineShapes.AddPicture((Get-TinyPng), $false, $true, $pictureRange)
        $picture = $inlinePicture.ConvertToShape()
        $picture.LockAnchor = -1
        $floating = $diagram.Inline.ConvertToShape()
        $floating.LockAnchor = -1
        $floating.Width = 300
        $floating.Height = 180
        $floating.WrapFormat.Type = 0
        $floating.RelativeHorizontalPosition = 0
        $floating.RelativeVerticalPosition = 2
        $floating.Left = 0
        $floating.Top = 0
        $anchor = $floating.Anchor.Duplicate
        $picture.RelativeHorizontalPosition = 0
        $picture.RelativeVerticalPosition = 2
        $picture.Left = 320
        $picture.Top = 0
        $picture.Width = 96
        $picture.Height = 48
        $picture.WrapFormat.Type = 0
        $anchor.ParagraphFormat.SpaceAfter = 190
        if ($picture.Anchor.Paragraphs.Item(1).Range.Start -ne $floating.Anchor.Paragraphs.Item(1).Range.Start) { throw 'The floating picture and SmartArt do not share the same paragraph anchor.' }
        $script:Details['sameParagraphPictureAnchor'] = [int]$picture.Anchor.Start
        Record-Diagram $floating.SmartArt '基本流程直线箭头' $true
    } },
    @{ Name = 'smartart/smartart-in-table.docx'; Steps = '2×2表格左上格插基本流程SmartArt'; Action = {
        $diagram = Add-Diagram 'process' (Get-CellFeatureRange)
        $diagram.Inline.Width = 190
        $diagram.Inline.Height = 120
        Set-FlatNodes $diagram.Art @('准备', '执行', '完成')
        Record-Diagram $diagram.Art '表格左上格，基本流程直线箭头'
    } },
    @{ Name = 'smartart/smartart-edited-text.docx'; Steps = '创建基本流程后将第二节点由执行改为执行（已修改），最后仅保存一次'; Action = {
        $diagram = Add-Diagram 'process'
        Set-FlatNodes $diagram.Art @('准备', '执行', '完成')
        $diagram.Art.AllNodes.Item(2).TextFrame2.TextRange.Text = '执行（已修改）'
        $script:Details['edit'] = [ordered]@{ node = 2; before = '执行'; after = '执行（已修改）' }
        Record-Diagram $diagram.Art '基本流程直线箭头'
    } },
    @{ Name = 'math/math-fraction.docx'; Steps = 'OMaths.Add，UnicodeMath输入，BuildUp生成叠式分式和上标'; Action = {
        $math = Add-Equation (Get-FeatureRange) '(a+b)/2+x^2'
        Require-Function $math 7 'fraction'
        Require-Function $math 19 'superscript'
        Record-Math '(a+b)/2 + x²' '独立一段，单个公式'
    } },
    @{ Name = 'math/math-integral.docx'; Steps = '带下限0、上限1的定积分'; Action = {
        $math = Add-Equation (Get-FeatureRange) '∫_0^1 x^2 ⅆx'
        Require-Function $math 13 'integral/n-ary'
        Record-Math '从0到1对x²积分，微分变量x' '独立一段，单个公式'
    } },
    @{ Name = 'math/math-matrix.docx'; Steps = 'UnicodeMath矩阵操作符，2×2矩阵，方括号'; Action = {
        $math = Add-Equation (Get-FeatureRange) '[■(a&b@c&d)]'
        Require-Function $math 5 'outer square-bracket delimiter'
        $script:Details['matrixInput'] = '[■(a&b@c&d)]'
        Record-Math '方括号内2×2矩阵，第一行a、b，第二行c、d' '独立一段，单个公式'
    } },
    @{ Name = 'math/math-inline.docx'; Steps = '质能方程一句话中间插入E=mc²，OMath.Type=行内'; Action = {
        $prefix = '质能方程 '
        $formula = 'E=mc^2'
        $range = Get-FeatureRange
        $start = [int]$range.Start
        $range.Text = $prefix + $formula + ' 很短'
        $range.SetRange($start + $prefix.Length, $start + $prefix.Length + $formula.Length)
        $mathRange = $script:Document.OMaths.Add($range)
        $math = $mathRange.OMaths.Item(1)
        $math.Type = 1
        $math.BuildUp()
        Require-Function $math 19 'superscript'
        Record-Math '质能方程 E=mc² 很短' '正文行内，公式前后有普通文字'
    } },
    @{ Name = 'math/math-display-two.docx'; Steps = '同一空段内依次创建两个独立OMath，间隔一个制表符'; Action = {
        $left = 'x^2+y^2=z^2'
        $right = 'E=mc^2'
        $range = Get-FeatureRange
        $start = [int]$range.Start
        $range.Text = $left + "`t" + $right
        # Build the right equation first so the left equation's original offsets stay valid.
        $rightRange = $script:Document.Range($start + $left.Length + 1, $start + $left.Length + 1 + $right.Length)
        $rightMathRange = $script:Document.OMaths.Add($rightRange)
        $rightMath = $rightMathRange.OMaths.Item(1)
        $rightMath.Type = 0
        $rightMath.BuildUp()
        $leftRange = $script:Document.Range($start, $start + $left.Length)
        $leftMathRange = $script:Document.OMaths.Add($leftRange)
        $leftMath = $leftMathRange.OMaths.Item(1)
        $leftMath.Type = 0
        $leftMath.BuildUp()
        if ($script:Document.Paragraphs.Item(2).Range.OMaths.Count -ne 2) { throw 'Word did not retain two OMath objects in one display paragraph.' }
        [xml]$nativeXml = $script:Document.Content.WordOpenXML
        $nativeNamespaces = [Xml.XmlNamespaceManager]::new($nativeXml.NameTable)
        $nativeNamespaces.AddNamespace('m', 'http://schemas.openxmlformats.org/officeDocument/2006/math')
        if ($nativeXml.SelectNodes('//m:oMathPara[count(m:oMath)=2]', $nativeNamespaces).Count -ne 1) {
            throw 'INCOMPLETE: Word produced separate oMathPara containers. The earlier saved first attempt remains a documented counterexample; create the required two-equation paragraph through the Word UI.'
        }
        Record-Math 'x²+y²=z²；E=mc²' '同一个公式段中两个公式并排，需复制解包确认同属一个m:oMathPara'
    } },
    @{ Name = 'math/math-builtin.docx'; Steps = '从本机Word内置构建基块插入二次公式、泰勒展开、傅里叶级数'; Action = {
        $entries = @(
            (Find-BuiltinEquation @('二次公式', '二次方程式', 'Quadratic Formula')),
            (Find-BuiltinEquation @('泰勒展开', '泰勒展开式', 'Taylor Expansion')),
            (Find-BuiltinEquation @('傅里叶级数', '傅立叶级数', 'Fourier Series'))
        )
        $script:Document.Content.Text = "before 前文`r`r`r`rafter 后文`r"
        for ($i = 2; $i -ge 0; $i--) {
            $range = $script:Document.Paragraphs.Item($i + 2).Range.Duplicate
            $range.Collapse(1)
            [void]$entries[$i].Insert($range, $true)
        }
        if ($script:Document.OMaths.Count -lt 3) { throw 'The three built-in entries did not produce three equations.' }
        $script:Details['buildingBlockNames'] = @($entries | ForEach-Object { [string]$_.Name })
        Record-Math 'Word内置二次公式、泰勒展开、傅里叶级数' '三个内置公式，各自一段'
    } },
    @{ Name = 'math/math-latex.docx'; Steps = '执行Word官方EquationLaTexFormat命令并读回选中状态，再输入指定LaTeX并BuildUp'; Action = {
        $math = Add-Equation (Get-FeatureRange) 'x'
        $script:Document.Activate()
        $math.Range.Select()
        $script:Word.CommandBars.ExecuteMso('EquationLaTexFormat')
        $selected = $script:Word.CommandBars.GetPressedMso('EquationLaTexFormat')
        if (-not $selected) { throw 'Word did not confirm that its LaTeX input mode is selected.' }
        $inputText = '\frac{1}{2}\sum_{i=1}^{n} x_i'
        $script:Document.Content.Text = "before 前文`r`rafter 后文`r"
        $math = Add-Equation (Get-FeatureRange) $inputText
        Require-Function $math 7 'LaTeX fraction'
        Require-Function $math 13 'LaTeX summation'
        $math.Range.Select()
        if (-not $script:Word.CommandBars.GetPressedMso('EquationLaTexFormat')) { throw 'LaTeX input mode was not retained for the final equation.' }
        $script:Details['latexInput'] = $inputText
        $script:Details['inputModeCommand'] = 'EquationLaTexFormat'
        $script:Details['inputModeReadBack'] = [bool]$selected
        Record-Math '1/2乘从i=1到n的x_i之和' '独立一段，真正LaTeX输入模式转换'
    } },
    @{ Name = 'math/math-linear.docx'; Steps = '创建半倍求和公式后OMath.Linearize，以线性格式保存'; Action = {
        $math = Add-Equation (Get-FeatureRange) '1/2 ∑_(i=1)^n x_i'
        Require-Function $math 13 'summation'
        $math.Linearize()
        $structuralTypes = @(Get-FunctionTypes $math | Where-Object { $_ -notin @(20, 21, 22) })
        if ($structuralTypes.Count -gt 0) { throw 'Word still exposes structured math after Linearize.' }
        Record-Math '1/2乘从i=1到n的x_i之和，显示为线性格式' '独立一段，单个线性公式'
    } },
    @{ Name = 'math/math-styled.docx'; Steps = '分式加上标；主体14磅，分子a为红色20磅；整段居中'; Action = {
        $math = Add-Equation (Get-FeatureRange) '(a+b)/2+x^2'
        $math.Range.Font.Size = 14
        Require-Function $math 7 'fraction'
        $findRange = $math.Functions.Item(1).Frac.Num.Range.Duplicate
        $firstCharacterLength = if ([char]::IsHighSurrogate($findRange.Text[0])) { 2 } else { 1 }
        $findRange.End = $findRange.Start + $firstCharacterLength
        $findRange.Font.Color = 255
        $findRange.Font.Size = 20
        $math.Range.ParagraphFormat.Alignment = 1
        $math.Justification = 2
        $script:Details['specificationDeviation'] = 'Word applies the size change across this equation; partial font sizing still requires Word UI work.'
        Record-Math '(a+b)/2 + x²' '独立一段，单个公式，居中' "分子a红色，其余黑色；整式字号实际读回$($math.Range.Font.Size)磅；局部字号要求未完成；居中"
    } },
    @{ Name = 'math/math-in-table.docx'; Steps = '2×2表格左上格插入叠式分式和上标公式'; Action = {
        $math = Add-Equation (Get-CellFeatureRange) '(a+b)/2+x^2'
        Require-Function $math 7 'fraction'
        Record-Math '(a+b)/2 + x²' '2×2表格左上格中的独立公式'
    } }
)

try {
    foreach ($case in $cases) {
        $stem = [IO.Path]::GetFileNameWithoutExtension($case.Name)
        if ($Only.Count -gt 0 -and $case.Name -notin $Only -and $stem -notin $Only) { continue }
        $script:Details = [ordered]@{}
        $script:Document = $null
        $path = Get-UnusedPath $case.Name
        $record = [ordered]@{
            runId = $runId
            requestedFile = $case.Name
            file = [IO.Path]::GetRelativePath($OutputRoot, $path).Replace('\', '/')
            status = 'incomplete'
            wordVersion = $null
            wordBuild = $null
            wordProcessId = $null
            method = '_scripts/create-smartart-math.ps1'
            steps = $case.Steps
            details = $script:Details
            error = $null
            savedOnce = $false
            pdfPreview = $null
            pdfExportStatus = if ($ExportPdf) { 'pending' } else { 'not-requested' }
            visualInspection = 'pending'
            copiedPackageSelfCheck = 'pending'
        }
        try {
            Initialize-WordInstance
            $record['wordVersion'] = [string]$script:Word.Version
            $record['wordBuild'] = [string]$script:Word.Build
            $record['wordProcessId'] = $script:WordProcessId
            $script:Document = $script:Word.Documents.Add()
            $script:Document.Content.Text = "before 前文`r`rafter 后文`r"
            $script:Document.PageSetup.TopMargin = 54
            $script:Document.PageSetup.BottomMargin = 54
            $script:Document.PageSetup.LeftMargin = 54
            $script:Document.PageSetup.RightMargin = 54
            & $case.Action
            Assert-MarkerParagraphs
            $script:Document.Repaginate()
            if ($script:Document.CompatibilityMode -lt 15) { $script:Document.SetCompatibilityMode(15) }
            $record['compatibilityMode'] = [int]$script:Document.CompatibilityMode
            # The only document save in this script; SaveAs2(..., 12) is Word Transitional DOCX.
            $script:Document.SaveAs2($path, 12)
            $record['savedOnce'] = $true
            $record['status'] = 'created-pending-visual-and-package-check'
            $record['bytes'] = (Get-Item -LiteralPath $path).Length
            if ($ExportPdf) {
                try {
                    $relativePreview = '_previews/' + [IO.Path]::ChangeExtension($record['file'], '.pdf')
                    $previewPath = Get-UnusedPath $relativePreview
                    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($previewPath))
                    # Export the still-open Word document, then close without saving again.
                    $script:Document.ExportAsFixedFormat($previewPath, 17, $false)
                    $record['pdfPreview'] = [IO.Path]::GetRelativePath($OutputRoot, $previewPath).Replace('\', '/')
                    $record['pdfExportStatus'] = 'exported-pending-visual-inspection'
                    $record['pdfBytes'] = (Get-Item -LiteralPath $previewPath).Length
                }
                catch {
                    $record['pdfExportStatus'] = 'failed'
                    $record['pdfExportError'] = $_.Exception.ToString()
                    Write-Warning "$($case.Name) PDF preview: $($_.Exception.Message)"
                }
            }
        }
        catch {
            $record['error'] = $_.Exception.ToString()
            $record['errorPosition'] = $_.InvocationInfo.PositionMessage
            Write-Warning "$($case.Name): $($_.Exception.Message)"
        }
        finally {
            if ($null -ne $script:Document) {
                try { $script:Document.Close(0) } catch { $record['closeError'] = $_.Exception.Message }
                try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($script:Document) } catch { }
                $script:Document = $null
            }
            if ($null -ne $script:Word) {
                if ($script:WordOwned) {
                    try { $script:Word.Quit(0) } catch { $record['quitError'] = $_.Exception.Message }
                }
                try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($script:Word) } catch { }
                $script:Word = $null
                $script:WordOwned = $false
            }
            [GC]::Collect()
            [GC]::WaitForPendingFinalizers()
            $script:Rows.Add([pscustomobject]$record)
            Write-Results
        }
    }
}
finally {
    if ($null -ne $script:Word) {
        if ($script:WordOwned) {
            try { $script:Word.Quit(0) } catch { Write-Warning "Word.Quit: $($_.Exception.Message)" }
        }
        try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($script:Word) } catch { }
        $script:Word = $null
    }
    [GC]::Collect()
    [GC]::WaitForPendingFinalizers()
}

Write-Output "Results: $logPath"
