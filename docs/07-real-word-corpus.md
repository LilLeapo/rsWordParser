# 07 · 桌面 Word 语料清单（需要项目负责人在真实 Word 里制作）

这份文档回答"要在桌面 Word 里做哪些文档、怎么做、做完放哪、我们拿它验什么"。它是 `spec/17-m6-plan.md`
「依赖与被阻塞」里"需要真实 Word"那几行的展开，也顺带补上 M1–M5 一直没有的真实样本。

## 1. 为什么合成语料不够

今天 `corpus/synthetic/` 的 573 份文档全部是 TS 测试用 `buildDocx` 拼出来的 XML；M6 新补的一批（`m6-chart__*` 等）
也是手写 XML。它们能钉住 TS 的行为，但钉不住 **Word 实际写出的形态**——那才是这个引擎最终要面对的输入：

- Word 会用 `mc:AlternateContent` 把新元素与旧兼容形态成对写出（图表的 `c14:style`、形状的 VML Fallback、
  chartex 的图片 Fallback），`w:document` 根上带几十个命名空间与 `mc:Ignorable`，每段有 `w14:paraId` / `w:rsid*`；
- 图表 part 带 `c:externalData autoUpdate`、内嵌 xlsx、`a:extLst` 里的 `c16r2` 扩展；SmartArt 除了四个 part 还有
  `diagrams/drawing1.xml`（Word 预算好的布局，语料里今天 **0 份**）；公式的 `m:oMath` 带 `m:ctrlPr` / `m:rPr` 属性包；
  Word 自己的墨迹是 `w14:contentPart` + InkML part，跟我们的 `aidocs-ink` 浮动图片是两回事；
- 这些东西里凡是我们不建模的，都必须**原字节保留**且 Word 能重新打开——这一条只有拿真文档、在真 Word 里开一遍才算验过。

真实文档拿来做四件事：① 无编辑保存字节相同（`TEST-04`，最重要）；② 改一个字后其他 zip 条目 CRC 不变且 **Word 打开无修复提示**
（只有人能做）；③ 解析差分（TS 对它们的输出作参考，差异按路径登记，TS 不是权威）；④ 编辑 / 保存操作的人工核对（`spec/17` 6.6 / 6.8）。

## 2. 制作规矩

| 项 | 要求 |
| --- | --- |
| Word | **桌面版** Microsoft 365（Windows 优先，macOS 也要一小批，见 P2）。网页版做不了图表 / SmartArt / 画布 / 墨迹，或者写出的形态不同。记下版本号（文件 → 帐户 → 关于 Word，例如 `16.0.18xxx`）与平台 |
| 内容 | **一份文档一个特征**，外加一两行正文（比如特征前一段写 `before`、后一段写 `after`），方便定位块与检查正文是否丢字。文字用中英混排一次即可 |
| 体积 | 越小越好。图片用一张小 PNG（几 KB）；图表数据 3 类别 × 2 系列；SmartArt 5 个节点以内 |
| 保存 | 默认 `.docx`（Word 文档，Transitional）。**不要**选兼容模式，**不要**"检查文档"删元数据（那会改写很多 part），**不要**开自动保存到 OneDrive（协同会多出 `people.xml` 一类 part——除非专门做那一份）。保存后不要再打开保存 |
| 命名 | `<域>-<特征>.docx`，全小写连字符，见第 3 节表里的名字。放到 `corpus/real/<域>/`（目录我建） |
| 记录 | 每份文档在 `corpus/real/OBSERVED.md` 加一行：文件名、Word 版本 / 平台、操作步骤要点、**你在 Word 里看到什么**（图表类型与标题、SmartArt 节点文字顺序、公式读法、页眉文字…）。这是 resolve 校准与 compat 投影唯一的"人眼 oracle" |
| 隐私 | 仓库私有，作者名可以留；不要放真实业务内容 |

## 3. 清单

"检什么"一栏是我们拿到文档后要跑的检查；你只需要按"怎么做"操作并把看到的写进 `OBSERVED.md`。

### P0 · M6 直接要用（图表 / SmartArt / 画布 / 公式 / OLE / 墨迹 / 图片）

**图表**（插入 → 图表 → 选类型 → 在弹出的 Excel 里把数据改成 3 类别 × 2 系列 → 关闭 Excel）

| 文件 | 怎么做 | 检什么 |
| --- | --- | --- |
| `chart/chart-column.docx` | 簇状柱形图，标题改成"销售统计" | `word/charts/chart1.xml`、`charts/_rels`、`embeddings/*.xlsx`、`c:externalData`、`c14:style` 包装；`chartDisplay` kind / title / 类别 / 系列 |
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
| `chart/chart-style.docx` | 任一图表，"图表样式"里选一个非默认样式 + "更改颜色" 选**单色**方案；另一份 `chart-style-gray.docx` 选灰度 | `c:style` / `c14:style` → `palette` |
| `chart/chart-point-color.docx` | 饼图，单独把一块扇区填成红色 | `c:dPt` → `pointColors` |
| `chart/chart-legend.docx` | 图例放顶部；另一份 `chart-no-legend.docx` 删掉图例；另一份 `chart-no-title.docx` 删掉标题 | `legendPos`、自动标题 / 无标题 |
| `chart/chart-floating.docx` | 任一图表，布局选项 → 四周型环绕，拖到正文右侧 | `wp:anchor` 图表 |
| `chart/chart-in-table.docx` | 2×2 表格，一格里插图表 | M3 × M6 交叉（TS 会丢，我们要保住） |
| `chart/chartex-sunburst.docx` | 插入 → 图表 → 旭日图 | `cx:chartSpace` + `mc:Fallback` 图片；`kind: pie` |
| `chart/chartex-treemap.docx`、`chartex-waterfall.docx`、`chartex-histogram.docx`、`chartex-boxwhisker.docx`、`chartex-funnel.docx` | 同上各一 | chartex 各 `layoutId` |
| `chart/chart-pasted-embedded.docx` | 在 Excel 里做一张图，复制，Word 里粘贴（默认"使用目标主题和嵌入工作簿"） | 与 Word 原生插入的差别 |
| `chart/chart-pasted-linked.docx` | 同上，粘贴选项选"链接数据" | `c:externalData` 指外部文件 |
| `chart/chart-pasted-picture.docx` | 同上，粘贴为图片 | 应是普通图片块 |

**SmartArt**（插入 → SmartArt）

| 文件 | 怎么做 | 检什么 |
| --- | --- | --- |
| `smartart/smartart-list.docx` | 列表 → 基本列表，输入 5 项文字 | 四个 part + `diagrams/drawing1.xml`；`previewText` 顺序；`diagramDisplay.shapes` |
| `smartart/smartart-hierarchy.docx` | 层次结构 → 组织结构图：总经理 → 两个部门 → 一个组，再加一个"助理" | `dgm:cxn` 树、`srcOrd` |
| `smartart/smartart-process.docx` | 流程 → 基本流程 3 步 | 连线形状（`prst=line` / 箭头） |
| `smartart/smartart-cycle.docx` | 循环 → 基本循环 | 弧形连线 |
| `smartart/smartart-picture.docx` | 图片 → 图片列表，两个节点各插一张小图 | 绘图 part 的 `a:blipFill` 经它**自己的** rels |
| `smartart/smartart-styled.docx` | 任一，"更改颜色"选彩色范围 + SmartArt 样式选一个三维样式 | `schemeClr` / 主题色、`a:effectLst` 原样保留 |
| `smartart/smartart-floating.docx` | 任一，布局选项 → 四周型，旁边再插一张浮动图片 | 锚定图示 + 同段兄弟绘图 |
| `smartart/smartart-in-table.docx` | 表格一格里插 SmartArt | M3 × M6 交叉 |
| `smartart/smartart-edited-text.docx` | 做完后在文本窗格里改一个节点文字再保存 | 数据 part 与绘图 part 是否同步（Word 会同步；我们只读绘图 part） |

**绘图画布**（插入 → 形状 → 最底下"新建画布"）

| 文件 | 怎么做 | 检什么 |
| --- | --- | --- |
| `canvas/canvas-shapes.docx` | 画布里放：矩形（带文字"画布"）、椭圆（填充主题色）、一条直线、一个箭头 | `lc:lockedCanvas`、`a:sp` / `a:cxnSp`、`a:txSp` |
| `canvas/canvas-picture.docx` | 画布里插一张图片 + 一个形状 | `a:pic` |
| `canvas/canvas-resized.docx` | 做完形状后把画布整体缩小到一半 | `chOff / chExt` 与 `wp:extent` 不一致 → 缩放 |
| `canvas/canvas-floating.docx` | 画布 → 布局选项 → 四周型 / 衬于文字下方 | `wp:anchor` 画布 |
| `canvas/canvas-textbox.docx` | 画布里放一个文本框（多段文字） | `wps:wsp` 在画布里 |

**公式**（插入 → 公式）

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
| `math/math-in-table.docx` | 表格一格里放公式 | M3 × M6 交叉（TS 会丢，我们要保住） |

**OLE 嵌入对象**（插入 → 对象）

| 文件 | 怎么做 | 检什么 |
| --- | --- | --- |
| `ole/ole-excel-embedded.docx` | 对象 → 新建 → Microsoft Excel Worksheet，写两格数字，点回正文 | `w:object` + `v:shape` + `o:OLEObject Type="Embed" ProgID="Excel.Sheet.12"`、`embeddings/*.xlsx`、预览 EMF |
| `ole/ole-excel-linked.docx` | 对象 → 由文件创建 → 选一个 xlsx → 勾"链接到文件" | `Type="Link"`、`r:id` 指外部文件 |
| `ole/ole-icon.docx` | 同上但勾"显示为图标" | 预览是图标 EMF，`DrawAspect="Icon"` |
| `ole/ole-ppt.docx` | 对象 → 新建 → PowerPoint 幻灯片 | 另一种 ProgID |
| `ole/ole-with-text.docx` | 一段文字末尾插入嵌入对象（不要单独一段） | 与文字同段 → run 级图片（6.4） |
| `ole/ole-in-table.docx` | 表格一格里插嵌入对象 | M3 × M6 交叉 |

**墨迹**（"绘图"选项卡；触屏 / 触控板 / 鼠标都行）

| 文件 | 怎么做 | 检什么 |
| --- | --- | --- |
| `ink/ink-pen.docx` | 绘图 → 笔，在一段文字上画两笔 | Word 原生墨迹：`w14:contentPart` + `word/ink/ink1.xml`（InkML）+ `mc:Fallback` 图片；我们要原字节保留、不误判成图片块 |
| `ink/ink-highlighter.docx` | 荧光笔划一段 | 同上 |
| `ink/ink-to-shape.docx` | 画一个圆，让 Word "墨迹转形状" | 变成普通形状（对照） |
| `ink/ink-math.docx` | 绘图 → 墨迹公式，手写 `x²+1` 转换 | 变成 OMML（对照 `math-*`） |

**图片**（插入 → 图片 → 此设备，用同一张小 PNG）

| 文件 | 怎么做 | 检什么 |
| --- | --- | --- |
| `image/image-inline.docx` | 嵌入型 | 基线 |
| `image/image-wrap-square.docx`、`image-wrap-tight.docx`、`image-behind.docx`、`image-front.docx`、`image-top-bottom.docx` | 布局选项五种各一 | `wp:anchor` 各 wrap；`wp:wrapPolygon` |
| `image/image-cropped.docx` | 图片格式 → 裁剪，切掉一部分 | `a:srcRect`（6.7 的 `replaceImage` 要删它） |
| `image/image-svg.docx` | 插入一张 `.svg` | `asvg:svgBlip` 扩展 + PNG 回退 |
| `image/image-linked.docx` | 插入 → 图片 → 下拉选"链接到文件" | `a:blip r:link` |
| `image/image-insert-and-link.docx` | 同上选"插入和链接" | `r:embed` + `r:link` 同时存在 |
| `image/image-emf.docx` | 插入一张 `.emf` 或 `.wmf`（Windows 剪贴画 / 从 PowerPoint 复制形状粘贴为图片-增强型图元文件） | metafile 媒体（不转换，登记） |
| `image/image-rotated.docx` | 旋转 45°，水平翻转 | `a:xfrm rot / flipH`、`wp:effectExtent` |
| `image/image-alt-decorative.docx` | 可选文字里勾"标记为装饰性" | `wp:docPr` 的 `a16:decorative` 扩展 |
| `image/image-two-in-run.docx` | 一段里连续插两张图，中间不打空格 | 同一 `w:r` / 相邻 run 两个 `w:drawing` |

### P1 · 其他里程碑至今没有真实样本的域（回归用，每项一份）

| 文件 | 怎么做 | 检什么 |
| --- | --- | --- |
| `text/text-basic.docx` | 标题 1 / 标题 2 / 正文各一段，一段项目符号，一段三级多级编号（1. → 1.1 → 1.1.1），粗斜体下划线删除线上标各一处，一处改颜色和字号，一段中英混排 | M1 文本域、`w14:paraId` / rsid 保留 |
| `text/text-custom-styles.docx` | 新建样式"我的标题" basedOn 标题 1 再改字体，应用它 | 样式链 |
| `table/table-styled.docx` | 3×3 表格套"网格表 4 - 着色 1"，勾标题行 / 镶边行；合并两格；一格里再套一个 2×2 表 | M3 表格样式条件格式、嵌套 |
| `hf/hf-variants.docx` | 页眉页脚：勾"首页不同"和"奇偶页不同"，三种页眉各写不同文字，页脚插页码，再加一个文字水印 | M5 六变体、水印 VML |
| `sections/sections-three.docx` | 三节：第 2 节横向，第 3 节双栏，各节页边距不同，第 2 节页眉取消"链接到前一节" | `w:sectPr` 三份、`RES-10` 继承 |
| `fields/fields-toc.docx` | 三级标题 + 插入目录，一处交叉引用（引用标题），一个脚注一个尾注，一个日期域 | M2 字段 / 注释 |
| `revisions/revisions-comments.docx` | 开启修订：插一句、删一句、改一处格式；两条批注其中一条有回复且标为"已解决" | 修订三形态、`commentsExtended` |
| `shapes/textbox-shapes.docx` | 一个文本框（两段文字）、一个带文字的圆角矩形、两个形状组合、一个艺术字 | M4 `wps` + VML Fallback |
| `sdt/content-controls.docx` | 开发工具：富文本控件、下拉列表、日期选取器、复选框各一，其中一个勾"无法删除 / 无法编辑" | M3 `SdtInfo` |
| `links/hyperlinks-bookmarks.docx` | 外链、书签 + 指向书签的内链、邮件链接 | `w:hyperlink` 两种目标 |
| `strict/strict-basic.docx` | 把 `text-basic` 另存为 **Strict Open XML 文档** | flavor 判定与 Strict 保持 |
| `misc/large-report.docx` | 一份 20 页以上的真实报告（可用公开文档），含图表 / 表格 / 目录 / 页眉 | 性能与鲁棒性；只做往返与编辑保真，不做差分 |

### P2 · 跨平台（可选）

- **macOS Word** 重做 `chart-column` / `smartart-list` / `math-fraction` / `image-wrap-square` 四份，文件名加 `-mac`。Mac 版的
  `mc:Ignorable` 集合与部分扩展不同。
- **WPS** 重做同样四份，文件名加 `-wps`（WPS 会给文本框形状写 `<v:imagedata o:title=""/>` 一类空壳，TS 有专门分支）。

## 4. 接入方式（由我做，你只管放文件）

- 目录 `corpus/real/<域>/<名字>.docx` 为**源文件**，随仓库提交；旁边的 `<名字>.expected.json` 由
  `tools/export-golden/real.export.test.ts` 生成（读 `corpus/real/**/*.docx`，用 TS `parseDocx` 录制，写回同目录；
  `record.ts` 加一个输出目录参数，`run.sh` 对 `corpus/real` 只删 json 不删 docx）。
- `cargo run -p diff-parse -- --corpus corpus/real` 看 TS 差分（参考，不是门）；`tests/save.rs` 的往返与编辑保真扫描
  加上 `corpus/real`（这两条**是**门：字节相同、CRC 不变）。
- 我们保存出来的文件（无编辑 / 改一个字 / 6.6 的图表数据补丁 / 6.8 的墨迹）会放到 `corpus/real/_roundtrip/`，
  请你在 Word 里打开并在 `OBSERVED.md` 记"打开无提示 / 有修复提示（截图）"——这是 6.6 / 6.8 的人工核对项。

## 5. 数量与顺序

P0 约 60 份，P1 约 12 份，每份 1–3 分钟。优先顺序：图表 → SmartArt → 画布 → 公式 → OLE → 墨迹 → 图片 → P1。
做不完先给图表、SmartArt、公式各 3 份也行，形态问题多半在前几份就能看出来。
