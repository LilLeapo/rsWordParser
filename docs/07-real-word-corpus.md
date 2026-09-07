# 07 · 桌面 Word 语料：制作说明（交给 Windows 侧代理）

> **读者**：在装有桌面版 Microsoft 365 Word 的 Windows 机器上工作的代理（codex）。这份文档是自含的——不需要读本仓库的其他
> 文件就能照做。**产出**：一个目录（打成 zip）交回 macOS 侧，由 rsword 的维护者接进仓库（§6）。有疑问先按本文档的字面做，
> 把疑问写进 `README.md` 的「未完成 / 存疑」一节，不要自行改规格。

## 0. 三十秒背景

- **rsword** 是一个 Rust 写的 `.docx` 引擎：解析 → 文档模型 → 编辑 → 保存。它替代一个 TypeScript 实现（genoffice 的
  `docx-engine`）。核心原则：**凡是引擎不建模的字节，保存时原样保留**；保存后的文件必须能被 Word 无提示打开。
- 现有的 799 份测试文档全部是测试代码拼出来的 XML，钉得住旧实现的行为，钉不住 **Word 实际写出的形态**（`mc:AlternateContent`
  成对写出的新旧兼容内容、根元素上几十个命名空间、`w14:paraId` / `w:rsid*`、图表 part 的 `c:externalData` 与内嵌 xlsx、
  SmartArt 的 `diagrams/drawing1.xml`、公式的 `m:ctrlPr`、Word 原生墨迹的 `w14:contentPart` + InkML……）。
- 引擎的 M6 里程碑（图表 / SmartArt / 画布 / OLE / 公式 / 墨迹的读侧，图表 / 图片 / 墨迹的写侧）已经完成，现在缺两样东西，
  只有真 Word 能给：
  - **任务 A**：Word 自己写出的文档（约 70 份小文件）+ 你在 Word 里**看到什么**（这是唯一的人眼 oracle）；
  - **任务 B**：用 Word 打开**我们写出的** 9 份文件（`_roundtrip/`），记录有没有「修复」提示、画得对不对。

## 1. 环境与方法

| 项 | 要求 |
| --- | --- |
| Word | **桌面版** Microsoft 365，Windows。记下精确版本（文件 → 账户 → 关于 Word，形如 `Microsoft® Word for Microsoft 365 MSO (版本 2409 Build 16.0.18025.20104) 64 位`）写进 `README.md`。网页版做不了图表 / SmartArt / 画布 / 墨迹，或者写出的形态不同，**不要用** |
| 自动化 | **允许并推荐**用 Word 自己的对象模型批量制作（PowerShell `New-Object -ComObject Word.Application` 或 Python `pywin32`）：只要文件是 **Word 写出来**的就算。常用入口：`Documents.Add`、`Selection.TypeText`、`InlineShapes.AddChart2(Style, Type)` + `Chart.ChartData.Activate()` 改工作簿、`InlineShapes.AddSmartArt(Application.SmartArtLayouts(i))`、`OMaths.Add(Range)` + `OMath.BuildUp()`、`InlineShapes.AddOLEObject(ClassType)`、`Shapes.AddCanvas`、`InlineShapes.AddPicture(FileName, LinkToFile, SaveWithDocument)`、`Document.SaveAs2(Path, 12)`（Strict 用 `24`）。**禁止**手拼 XML、直接解包改包再压回——那正是我们要避开的形态 |
| 只能手做的 | 墨迹（`ink/*`）：绘图选项卡 → 笔，用鼠标拖也能画；粘贴类（`chart-pasted-*`、`image-emf`）；「墨迹转形状 / 墨迹公式」 |
| 每份文档 | **一份一个特征**，特征前一段写 `before`、后一段写 `after`（中英各一次即可，例如 `before 前文`），方便定位块与检查丢字 |
| 体积 | 越小越好：图片统一用一张几 KB 的小 PNG（自己画一张 64 × 32 的纯色块即可）；图表数据 3 类别 × 2 系列；SmartArt ≤ 5 节点 |
| 保存 | 默认 `.docx`（Word 文档，Transitional）。**不要**兼容模式；**不要**「检查文档」删元数据（会改写很多 part）；**关掉自动保存 / OneDrive**（协同会多出 `people.xml` 一类 part）；保存后**不要再打开保存**（要看效果就打开、关闭、不存） |
| 命名 | `<域>/<特征>.docx`，全小写连字符，**用第 3 节表里的名字**。文件名重复就在末尾加 `-2` |
| 记录 | 每份一行写进 `OBSERVED.md`（§2.1）。做不出来的也写一行，说明原因 |
| 隐私 | 仓库私有，作者名可以留；不放真实业务内容 |
| 自检 | 每份做完，**复制一份**解包（`Expand-Archive`、7-Zip；别动原件），按各表的「自检」行核对该有的 part / 标记都在。对不上就重做，并在 `OBSERVED.md` 注明 |

## 2. 交付物

```
real-word-corpus-<yyyymmdd>/
  README.md          Word 精确版本、Windows 版本、日期；哪些用 UI 手做、哪些用脚本；未完成 / 存疑清单
  OBSERVED.md        任务 A：每份文档一行（§2.1）
  ROUNDTRIP.md       任务 B：每份样本一行（§4）
  _scripts/          用过的 COM / PowerShell / Python 脚本，能复现就行
  screenshots/       修复提示、异常渲染的截图（文件名 = 文档名 + 序号）
  chart/  smartart/  canvas/  math/  ole/  ink/  image/           ← 任务 A 的 P0
  text/  table/  hf/  sections/  fields/  revisions/  shapes/  sdt/  links/  strict/  misc/   ← 任务 A 的 P1
  _roundtrip/        任务 B：把我们给的 9 份原样放回来，再加上你用 Word「另存为」出的 <名>-resaved-by-word.docx
```

打成 `real-word-corpus-<yyyymmdd>.zip` 交回。zip 里不要带 `~$*.docx` 锁文件。

### 2.1 `OBSERVED.md` 的写法

表头固定：

```
| 文件 | Word 版本（build）/ 平台 | 制作方式（UI / 脚本名） | 步骤要点 | 看到什么 |
```

「看到什么」按域写全，这是我们校准显示模型的依据：

- **图表**：类型、标题文字、类别文字（日期类别写 Word 显示的格式）、每个系列的名字与值、图例位置（有 / 无 / 顶 / 右）、
  颜色方案（单色 / 彩色 / 灰度）、是否浮动；饼图 / 圆环有没有某块特殊颜色；圆环内径大小。
- **SmartArt**：布局名；节点文字**按阅读顺序**列出并标层级（`总经理 > 部门A > 组1`）；连线是直线还是弧线 / 箭头；配色与样式名。
- **画布**：画布里每个形状（类型 / 文字 / 填充色）、画布尺寸（选中后布局里的宽高）、是否浮动。
- **公式**：读法（`(a+b)/2 + x²`）、是行内还是独立一段、有没有两个公式并排、颜色 / 字号 / 对齐。
- **OLE**：预览是表格内容还是图标；双击能否打开；ProgID（对象 → 属性 里能看到类型）。
- **墨迹**：几笔、颜色、大概位置；是否被转成形状 / 公式。
- **图片**：环绕方式、是否裁剪 / 旋转 / 翻转、是否链接、是不是 SVG / EMF、可选文字。
- **其他 P1 域**：把 Word 显示的结构复述一遍（几节、页眉文字、目录条目、修订 / 批注内容、控件类型…）。

## 3. 任务 A · 清单

「检什么」是我们拿到文件后跑的检查，你不用管；「自检」是你解包后要看到的东西。

### P0 · 引擎当前直接要用的域

**图表**（插入 → 图表 → 选类型 → 在弹出的 Excel 里把数据改成 3 类别 × 2 系列 → 关闭 Excel）
自检：`word/charts/chart1.xml`、`word/charts/_rels/chart1.xml.rels`、`word/embeddings/*.xlsx`（内嵌工作簿），`chart1.xml` 里有
`<c:externalData r:id=…>`；`word/document.xml` 里有 `<c:chart r:id=…>`。chartex 类：`word/charts/chartEx1.xml` +
`<cx:chart>`，`document.xml` 里的 `mc:AlternateContent` 带一张回退图片（`word/media/*.png`）。

| 文件 | 怎么做 | 检什么 |
| --- | --- | --- |
| `chart/chart-column.docx` | 簇状柱形图，标题改成"销售统计" | `chart1.xml`、`c:externalData`、`c14:style` 包装；`chartDisplay` kind / title / 类别 / 系列 |
| `chart/chart-bar.docx` | 簇状条形图 | `c:barDir val="bar"` → `horizontal` |
| `chart/chart-stacked.docx` | 堆积柱形图；再另存一份 `chart-percent-stacked.docx`（百分比堆积） | `grouping` |
| `chart/chart-line.docx` | 带数据标记的折线图；另一份 `chart-line-plain.docx` 不带标记 | `markers` |
| `chart/chart-pie.docx` | 饼图 | `kind: pie` |
| `chart/chart-doughnut.docx` | 圆环图；再把"圆环图内径大小"从 75% 改到 30%（右键系列 → 设置数据系列格式） | `holePct` |
| `chart/chart-area.docx` | 面积图 | |
| `chart/chart-scatter.docx` | 散点图（仅标记）；另一份 `chart-scatter-lines.docx`（带平滑线和标记） | `xValues` / `line` |
| `chart/chart-bubble.docx` | 气泡图 | `sizes` |
| `chart/chart-combo.docx` | 组合图：系列 1 簇状柱形 + 系列 2 折线（次坐标轴） | 第一个 `*Chart` 决定 kind |
| `chart/chart-3d.docx` | 三维簇状柱形图 | `bar3DChart` |
| `chart/chart-dates.docx` | 类别列填日期（2024/1/1、2024/2/1、2024/3/1）的折线图 | 日期序列号 → `m/d/yyyy` |
| `chart/chart-style.docx` | 任一图表，"图表样式"里选一个非默认样式 + "更改颜色"选**单色**方案；另一份 `chart-style-gray.docx` 选灰度 | `c:style` / `c14:style` → `palette` |
| `chart/chart-point-color.docx` | 饼图，单独把一块扇区填成红色 | `c:dPt` → `pointColors` |
| `chart/chart-legend.docx` | 图例放顶部；另一份 `chart-no-legend.docx` 删掉图例；另一份 `chart-no-title.docx` 删掉标题 | `legendPos`、自动标题 / 无标题 |
| `chart/chart-floating.docx` | 任一图表，布局选项 → 四周型环绕，拖到正文右侧 | `wp:anchor` 图表 |
| `chart/chart-in-table.docx` | 2×2 表格，一格里插图表 | 表格 × 图表交叉（旧实现会丢，我们要保住） |
| `chart/chartex-sunburst.docx` | 插入 → 图表 → 旭日图 | `cx:chartSpace` + `mc:Fallback` 图片；`kind: pie` |
| `chart/chartex-treemap.docx`、`chartex-waterfall.docx`、`chartex-histogram.docx`、`chartex-boxwhisker.docx`、`chartex-funnel.docx` | 同上各一 | chartex 各 `layoutId` |
| `chart/chart-pasted-embedded.docx` | 在 Excel 里做一张图，复制，Word 里粘贴（默认"使用目标主题和嵌入工作簿"） | 与 Word 原生插入的差别 |
| `chart/chart-pasted-linked.docx` | 同上，粘贴选项选"链接数据" | `c:externalData` 指外部文件 |
| `chart/chart-pasted-picture.docx` | 同上，粘贴为图片 | 应是普通图片块 |

**SmartArt**（插入 → SmartArt）
自检：`word/diagrams/data1.xml`、`layout1.xml`、`quickStyle1.xml`、`colors1.xml`、**`drawing1.xml`** 五个 part；`document.xml` 里
`<dgm:relIds r:dm=… r:lo=… r:qs=… r:cs=…>`。

| 文件 | 怎么做 | 检什么 |
| --- | --- | --- |
| `smartart/smartart-list.docx` | 列表 → 基本列表，输入 5 项文字 | 五个 part；`previewText` 顺序；`diagramDisplay.shapes` |
| `smartart/smartart-hierarchy.docx` | 层次结构 → 组织结构图：总经理 → 两个部门 → 一个组，再加一个"助理" | `dgm:cxn` 树、`srcOrd` |
| `smartart/smartart-process.docx` | 流程 → 基本流程 3 步 | 连线形状（`prst=line` / 箭头） |
| `smartart/smartart-cycle.docx` | 循环 → 基本循环 | 弧形连线 |
| `smartart/smartart-picture.docx` | 图片 → 图片列表，两个节点各插一张小图 | 绘图 part 的 `a:blipFill` 经它**自己的** rels |
| `smartart/smartart-styled.docx` | 任一，"更改颜色"选彩色范围 + SmartArt 样式选一个三维样式 | `schemeClr` / 主题色、`a:effectLst` 原样保留 |
| `smartart/smartart-floating.docx` | 任一，布局选项 → 四周型，旁边再插一张浮动图片 | 锚定图示 + 同段兄弟绘图 |
| `smartart/smartart-in-table.docx` | 表格一格里插 SmartArt | 表格 × SmartArt 交叉 |
| `smartart/smartart-edited-text.docx` | 做完后在文本窗格里改一个节点文字再保存 | 数据 part 与绘图 part 是否同步 |

**绘图画布**（插入 → 形状 → 最底下"新建画布"）
自检：`document.xml` 里 `<lc:lockedCanvas>`（在 `mc:AlternateContent` 的 Choice 里），画布内 `<a:sp>` / `<a:cxnSp>` / `<a:pic>`。

| 文件 | 怎么做 | 检什么 |
| --- | --- | --- |
| `canvas/canvas-shapes.docx` | 画布里放：矩形（带文字"画布"）、椭圆（填充主题色）、一条直线、一个箭头 | `lc:lockedCanvas`、`a:sp` / `a:cxnSp`、`a:txSp` |
| `canvas/canvas-picture.docx` | 画布里插一张图片 + 一个形状 | `a:pic` |
| `canvas/canvas-resized.docx` | 做完形状后把画布整体缩小到一半 | `chOff / chExt` 与 `wp:extent` 不一致 → 缩放 |
| `canvas/canvas-floating.docx` | 画布 → 布局选项 → 四周型 / 衬于文字下方 | `wp:anchor` 画布 |
| `canvas/canvas-textbox.docx` | 画布里放一个文本框（多段文字） | `wps:wsp` 在画布里 |

**公式**（插入 → 公式）
自检：`document.xml` 里 `<m:oMathPara>` / `<m:oMath>`，结构元素如 `<m:f>`、`<m:sSup>`、`<m:nary>`、`<m:m>`、`<m:d>`。

| 文件 | 怎么做 | 检什么 |
| --- | --- | --- |
| `math/math-fraction.docx` | 分式 → 叠式分式，分子 `a+b`，分母 `2`；后面加上标 `x²` | `m:f` / `m:sSup`、`m:ctrlPr` 属性包、`formulaDisplay.mathml / latex` |
| `math/math-integral.docx` | 积分 → 带上下限的定积分 | `m:nary` |
| `math/math-matrix.docx` | 矩阵 → 2×2，外面加方括号 | `m:m` 在 `m:d` 里 |
| `math/math-inline.docx` | 正文一句话中间插入公式（"质能方程 E=mc² 很短"） | 行内 `m:oMath` → `runs[].math`，前后文字拆 run |
| `math/math-display-two.docx` | 一个公式段里放两个公式（公式右侧下拉 → 插入新公式） | `m:oMathPara` 包两个 `m:oMath` |
| `math/math-builtin.docx` | 内置公式：二次公式、泰勒展开、傅里叶级数各一段 | 元素覆盖面（根式 / 求和 / 上下标 / 希腊字母） |
| `math/math-latex.docx` | 公式工具 → 转换 → 选 LaTeX 输入，键入 `\frac{1}{2}\sum_{i=1}^{n} x_i` 回车 | 与手点出来的结构是否一样 |
| `math/math-linear.docx` | 同一个公式转成"线性"格式保存 | `m:oMath` 只有 `m:r` |
| `math/math-styled.docx` | 公式里一部分改颜色、改字号；整段居中 | `w:rPr` 在 `m:r` 里、`m:oMathParaPr/m:jc` |
| `math/math-in-table.docx` | 表格一格里放公式 | 表格 × 公式交叉（旧实现会丢，我们要保住） |

**OLE 嵌入对象**（插入 → 对象）
自检：`document.xml` 里 `<w:object>` 包 `<v:shape>` + `<o:OLEObject Type="Embed|Link" ProgID=…>`；`word/embeddings/*.xlsx|.bin`；
预览 `word/media/image*.emf`。

| 文件 | 怎么做 | 检什么 |
| --- | --- | --- |
| `ole/ole-excel-embedded.docx` | 对象 → 新建 → Microsoft Excel Worksheet，写两格数字，点回正文 | `ProgID="Excel.Sheet.12"`、`embeddings/*.xlsx`、预览 EMF |
| `ole/ole-excel-linked.docx` | 对象 → 由文件创建 → 选一个 xlsx → 勾"链接到文件" | `Type="Link"`、`r:id` 指外部文件 |
| `ole/ole-icon.docx` | 同上但勾"显示为图标" | 预览是图标 EMF，`DrawAspect="Icon"` |
| `ole/ole-ppt.docx` | 对象 → 新建 → PowerPoint 幻灯片 | 另一种 ProgID |
| `ole/ole-with-text.docx` | 一段文字末尾插入嵌入对象（不要单独一段） | 与文字同段 → run 级图片 |
| `ole/ole-in-table.docx` | 表格一格里插嵌入对象 | 表格 × OLE 交叉 |

**墨迹**（"绘图"选项卡；鼠标拖动也能画）
自检：`document.xml` 里 `<w14:contentPart r:id=…>`（在 `mc:AlternateContent` 的 Choice 里，Fallback 是一张图片）、`word/ink/ink1.xml`
（InkML）。注意：这是 **Word 原生墨迹**，与引擎自己写的 `aidocs-ink` 浮动图片是两回事——我们要的是「不误判、原字节保留」。

| 文件 | 怎么做 | 检什么 |
| --- | --- | --- |
| `ink/ink-pen.docx` | 绘图 → 笔，在一段文字上画两笔 | `w14:contentPart` + InkML + Fallback 图片；不误判成图片块 |
| `ink/ink-highlighter.docx` | 荧光笔划一段 | 同上 |
| `ink/ink-to-shape.docx` | 画一个圆，让 Word "墨迹转形状" | 变成普通形状（对照） |
| `ink/ink-math.docx` | 绘图 → 墨迹公式，手写 `x²+1` 转换 | 变成 OMML（对照 `math-*`） |

**图片**（插入 → 图片 → 此设备，用同一张小 PNG）
自检：`word/media/image1.png`；`document.xml` 里 `<wp:inline>` 或 `<wp:anchor>`（后者带 `<wp:wrapSquare>` / `wrapTight` +
`wp:wrapPolygon` / `wrapTopAndBottom` / `wrapNone`），`<a:blip r:embed=…>`（链接图是 `r:link`）。

| 文件 | 怎么做 | 检什么 |
| --- | --- | --- |
| `image/image-inline.docx` | 嵌入型 | 基线 |
| `image/image-wrap-square.docx`、`image-wrap-tight.docx`、`image-behind.docx`、`image-front.docx`、`image-top-bottom.docx` | 布局选项五种各一 | `wp:anchor` 各 wrap；`wp:wrapPolygon` |
| `image/image-cropped.docx` | 图片格式 → 裁剪，切掉一部分 | `a:srcRect`（换图时要删它） |
| `image/image-svg.docx` | 插入一张 `.svg` | `asvg:svgBlip` 扩展 + PNG 回退 |
| `image/image-linked.docx` | 插入 → 图片 → 下拉选"链接到文件" | `a:blip r:link` |
| `image/image-insert-and-link.docx` | 同上选"插入和链接" | `r:embed` + `r:link` 同时存在 |
| `image/image-emf.docx` | 插入一张 `.emf` 或 `.wmf`（从 PowerPoint 复制形状，粘贴为图片-增强型图元文件） | metafile 媒体（不转换，登记） |
| `image/image-rotated.docx` | 旋转 45°，水平翻转 | `a:xfrm rot / flipH`、`wp:effectExtent` |
| `image/image-alt-decorative.docx` | 可选文字里勾"标记为装饰性" | `wp:docPr` 的 `a16:decorative` 扩展 |
| `image/image-two-in-run.docx` | 一段里连续插两张图，中间不打空格 | 同一 `w:r` / 相邻 run 两个 `w:drawing` |

### P1 · 其他域至今没有真实样本（回归用，每项一份）

| 文件 | 怎么做 | 检什么 |
| --- | --- | --- |
| `text/text-basic.docx` | 标题 1 / 标题 2 / 正文各一段，一段项目符号，一段三级多级编号（1. → 1.1 → 1.1.1），粗斜体下划线删除线上标各一处，一处改颜色和字号，一段中英混排 | 文本域、`w14:paraId` / rsid 保留 |
| `text/text-custom-styles.docx` | 新建样式"我的标题" basedOn 标题 1 再改字体，应用它 | 样式链 |
| `table/table-styled.docx` | 3×3 表格套"网格表 4 - 着色 1"，勾标题行 / 镶边行；合并两格；一格里再套一个 2×2 表 | 表格样式条件格式、嵌套 |
| `hf/hf-variants.docx` | 页眉页脚：勾"首页不同"和"奇偶页不同"，三种页眉各写不同文字，页脚插页码，再加一个文字水印 | 六变体、水印 VML |
| `sections/sections-three.docx` | 三节：第 2 节横向，第 3 节双栏，各节页边距不同，第 2 节页眉取消"链接到前一节" | `w:sectPr` 三份、继承 |
| `fields/fields-toc.docx` | 三级标题 + 插入目录，一处交叉引用（引用标题），一个脚注一个尾注，一个日期域 | 字段 / 注释 |
| `revisions/revisions-comments.docx` | 开启修订：插一句、删一句、改一处格式；两条批注其中一条有回复且标为"已解决" | 修订三形态、`commentsExtended` |
| `shapes/textbox-shapes.docx` | 一个文本框（两段文字）、一个带文字的圆角矩形、两个形状组合、一个艺术字 | `wps` + VML Fallback |
| `sdt/content-controls.docx` | 开发工具：富文本控件、下拉列表、日期选取器、复选框各一，其中一个勾"无法删除 / 无法编辑" | `SdtInfo` |
| `links/hyperlinks-bookmarks.docx` | 外链、书签 + 指向书签的内链、邮件链接 | `w:hyperlink` 两种目标 |
| `strict/strict-basic.docx` | 把 `text-basic` 另存为 **Strict Open XML 文档** | flavor 判定与 Strict 保持 |
| `misc/large-report.docx` | 一份 20 页以上的真实报告（可用公开文档），含图表 / 表格 / 目录 / 页眉 | 性能与鲁棒性；只做往返与编辑保真 |

### P2 · 跨平台（Windows 侧不做）

macOS Word 与 WPS 各重做 `chart-column` / `smartart-list` / `math-fraction` / `image-wrap-square` 四份，文件名加 `-mac` / `-wps`。由 macOS 侧自己做。

## 4. 任务 B · 用 Word 打开我们写出的文件

`_roundtrip/` 里有 9 份文件，`_roundtrip/README.md` 逐份写了「我们做了什么 / 应看到什么」。请：

1. 先开 `*-source.docx`（改动前的源文件，来自旧实现的测试语料），再开对应的样本；两者都记「打开有无提示」——源文件本身
   若有提示，那不是我们的问题，但要记下来。
2. 每份一行写进 `ROUNDTRIP.md`：`| 文件 | 打开时有无修复 / 兼容提示（有则截图） | 看到什么（图表类型 / 标题 / 数值；图片位置与环绕；
   墨迹位置） | 与 README 期望是否一致 |`。
3. 图表样本额外做：右键 → **编辑数据**，记能否打开、数字是否与图一致；`02-chart-setdata.docx` 请特别记录 Word 是否用工作簿
   里的旧数字把图表刷回去。
4. 每份用 Word **另存为** `<名>-resaved-by-word.docx` 放回 `_roundtrip/`：我们拿它对比 Word 重写了哪些字节。

## 5. 数量、顺序与完成标准

- P0 约 60 份，P1 约 12 份，任务 B 9 份，每份 1–3 分钟。优先顺序：**任务 B → 图表 → SmartArt → 画布 → 公式 → OLE → 墨迹 →
  图片 → P1**。做不完先给任务 B 与图表、SmartArt、公式各 3 份，形态问题多半在前几份就能看出来。
- 完成标准：每份文件通过自检；`OBSERVED.md` / `ROUNDTRIP.md` 每份一行、「看到什么」按 §2.1 写全；`README.md` 有精确版本与
  未完成清单；zip 里没有锁文件。

## 6. 接入方式（macOS 侧，维护者做，你不用管）

**状态（2026-09-07）**：第一轮交付已收到并接入；**第二轮任务书在 `docs/08-real-word-round2.md`**（Word 验收本引擎写出的 944 份编辑后文档、桌面版 toggle 复核、M7 语料）。第一轮交付已收到并接入（Office LTSC 2021，124 份；结果与规格修正见下）。下一轮若再做，优先：
① 用 Microsoft 365 复做 `chart-column` / `smartart-list` / `math-fraction` / `image-wrap-square` 四份对照版本差异；
② 重开 `_roundtrip/` 里重新生成的样本（`05` / `06` 修了重复 `Default`，`02` / `04` 换成真 Word 底稿）；③ 补 `ink-to-shape` 的成功版本。

规格修正（真 Word 与本文档第 3 节原先写法不同，以真 Word 为准）：画布写 `wpc:wpc`（`mc:Choice Requires="wpc"`，子形状
`wps:wsp` / `pic:pic`），**不是** `lc:lockedCanvas`；装饰性图片写 `adec:decorative`（`…/drawing/2017/decorative`），不是 `a16`；
链接 OLE 的预览是 WMF，PowerPoint 嵌入包是 `.sldx`；图片 SmartArt 的布局名是「图片题注列表」。

- 目录 `corpus/real/<域>/<名字>.docx` 为**源文件**，随仓库提交；旁边的 `<名字>.expected.json` 由
  `tools/export-golden/real.export.test.ts` 生成（读 `corpus/real/**/*.docx`，用 TS `parseDocx` 录制，写回同目录）。
- `cargo run -p diff-parse -- --corpus corpus/real` 看 TS 差分（参考，不是门；差异按路径登记，TS 不是权威）；`tests/save.rs`
  的往返与编辑保真扫描加上 `corpus/real`（这两条**是**门：字节相同、CRC 不变）。
- `_roundtrip/` 的样本由 `cargo test -p rsword --test roundtrip_samples -- --ignored` 生成；Word 另存回来的文件用来对比
  Word 重写了哪些字节。
