# 最终差异审查快照

最终审查日期：2026-09-07（Asia/Shanghai），已纳入公式、墨迹与四种 ChartEx 的末次补测。本审查基于主任务实际 Office 操作记录及离线 PDF/ZIP 检查；离线检查没有改写 DOCX。本表与最终 OBSERVED.md 一起说明成功版本、替代形态和保留试件。

依据：`C:\Users\Administrator\rsWordParser\docs\07-real-word-corpus.md`；`observed-chart.md`、`observed-smartart-math-ole.md`、`observed-canvas-image.md`、`observed-p1-layout.md`、`observed-p1-data.md`；各自 selfcheck/results JSON；`ROUNDTRIP.md` 和 `STRUCTURE.md`。规格把数量写为约 70 份，但逐个展开表内文件名实际为任务 A 92 项：图表 32、SmartArt 9、画布 5、公式 10、OLE 6、墨迹 4、图片 14、P1 12。不要用原始 DOCX 文件数推断通过项数，修正版和失败试件会重复计数。

## 状态含义

- **通过**：当前 LTSC 环境的目视与相关目标结构均有证据；不代表 Microsoft 365 已核验。
- **替代 / 差异**：Word 原生产物有效，但输出形态或具体布局与规格不同，需交给维护者明确接受。
- **试件**：文件真实存在且应保留，但已知未达到该项要求，不能列作通过。
- **未完成 / 待证据**：缺合格文件或缺指定观察证据。

## 不能遗漏的结论

| 项目 | 状态 | README 应明确写出的事实 |
| --- | --- | --- |
| Word 环境，规格第 23 行 | 全局差异 | 实际为 Office LTSC Professional Plus 2021 x64，16.0.14334.20848，Windows 11 build 22631；不是要求的 Microsoft 365。 |
| `math/math-display-two-2.docx`，规格第 142 行 | 通过；首版保留为试件 | 修正版通过实际 Word 键盘 Alt+=、Shift+Enter、Alt+= 制作。保存包为一个 m:oMathPara 直接包两个 m:oMath，首式末尾有 w:br；PDF 显示 1+1=2、2+2=4 两行居中，11 磅，before/after 顺序正确。首版含两个 m:oMathPara，保留失败结构，不能列为成功版本。证据为 `math-ui-offline-review.json`。 |
| `math/math-styled-2.docx`，规格第 146 行 | 通过；首版保留为试件 | 修正版通过原生 EquationNormalText 命令仅把 a 转为公式内普通文本数学 run，再设红色斜体 20 磅，其余保持黑色 14 磅。保存包 a 含 m:nor/w:i/color=FF0000/sz=40，其余五个文字 run 均 sz=28；一个分式、一个上标、一个公式和居中均保留，PDF 可见仅 a 更大。首版整式 20 磅的失败样本仍保留。证据为 `math-ui-offline-review.json` 和 `math-styled-prepare-20260907-025101-311.json`。 |
| 5 种 canvas，规格第 124、127-131 行 | 替代结构 | 真实 Word 全部写 `wpc:wpc`，内部为 `wps:wsp` / `pic:pic`；没有 `lc:lockedCanvas`、没有对应的 `a:sp/a:cxnSp`。不能声称已取得原规格所需的 lockedCanvas 形态。 |
| `canvas/canvas-resized-2.docx`，规格第 129 行 | 视觉通过 / 结构替代 | 大小已正确从 360×200 缩为 180×100 磅；Word直接缩放各子形状 `a:xfrm`，未生成 `chOff/chExt` 与外层 extent 不一致的目标结构。 |
| `shapes/textbox-shapes-3.docx`，规格第 201 行及第 26 行 | 通过；首两版保留为布局试件 | 第三版四个浮动对象均锚定第二个特征段落，before 在上，文本框/圆角矩形、组合和完整艺术字在中，after 在下。首版 before 落在文本框边界内，第二版组合仍在 after 下方；不能作为成功版本。证据见 `observed-p1-layout.md` 和 `textbox-shapes-3-offline-review.json`。 |
| `ole/ole-icon-3.docx`，规格第 64 行 | 外观、结构与激活通过 | 新版是清晰 Excel 图标、DrawAspect=Icon、内嵌 xlsx；新版自己的只读副本 DoVerb(0) 成功，直接从激活对象暴露的内嵌工作簿读到 A1=12、B1=34；未打开源 XLSX，未写入或保存，原件与副本 SHA-256 均不变。证据为 `ole-icon-3-activation-20260907-022629-778.json`。 |
| 原生墨迹 4 项，规格第 168-171 行 | pen/highlighter/math 三项通过；to-shape 为未转换试件 | `ink-pen.docx` 是黑色 0.5 mm 两笔，两个 w14:contentPart、两份 InkML 各一条 trace；`ink-highlighter.docx` 是黄色 6 mm 单笔，InkML 明确保存 #FFFC00、矩形笔尖、width=0.3 cm、height=0.6 cm、maskPen，两项都有 VML/PNG Fallback。`ink-math.docx` 经真实 UI 手写识别 x²+1，转换为一个行内 m:oMath，含 m:sSup 和 +1。`ink-to-shape.docx` 已有实际 InkToShape UI 尝试及 PDF，但八笔只组成八边形，保存后仍是八个 Type=23 / w14:contentPart、八份 InkML，没有普通形状；该项目未达到目标，保留为试件，本次不再补做。四项都有候选文件不等于四项通过。详见 `observed-ink.md` 和 `ink-offline-review.json`。 |

## 图表

23 个标准图表已有逐份 PDF 观察、数据缓存/工作簿和图表关系检查，按所记录的实际类型、图例、颜色、30% 圆环内径等可列为通过。饼图和单点着色饼图使用单系列，属于图表类型决定的合理例外；日期轴实际显示 `2024/1/1` 等，不应改写为未看到的格式。

| 图表项目 | 本快照状态 | 说明 |
| --- | --- | --- |
| `chart/chartex-sunburst.docx` | 已生成 UI 试件，不能列为通过 | 02:03 保存的新文件有 `chartEx1.xml`、`layoutId=sunburst`、`cx:externalData`、内嵌 xlsx 和 AlternateContent/PNG 回退，说明目标 Chartex 包形态确已得到。但类别缓存是 3 层×16 项 `#REF!`，数值 `cx:numDim` 为空，仍引用 A2:C17 / D2:D17。应保留为数据失败试件或由后续修正版替代。 |
| `chart/chartex-sunburst-2.docx`、`chart/chartex-treemap.docx` | 通过 | 实际 Word UI 制作并目视确认；`chartEx1.xml` 的 layoutId 分别为 sunburst/treemap，含 cx:externalData、内嵌 xlsx、AlternateContent 和 PNG 回退。两份数据均为 Group A/A/B、Item 1/2/3、10/20/30，无 #REF!；旭日图保留未使用空类别槽位，不影响三个有效数据点。 |
| `chart/chartex-waterfall-2.docx` | 三类别单系列显示与结构通过；保留单系列说明 | 新版 PDF 横轴及柱内标签均为 Category 1/2/3，三段蓝色瀑布柱累计 0→10→30→60，无多余位置。layoutId=waterfall，公式已缩成 A2:C4 / D2:D4，缓存 ptCount=3、数值 [10,20,30]，工作簿 C:D 一致，内部 xlsx、externalData、555×324 PNG fallback 齐全。旧无后缀文件有 16 位置，保留为试件。未声称两系列。 |
| `chart/chartex-histogram-2.docx` | 部分完成；第二系列只在工作簿 | 新版 layoutId=clusteredColumn 且有 binning intervalClosed=r，公式 A2:C4 / D2:D4，缓存 ptCount=3。PDF 仍只有一个蓝色区间 [10,34] 柱，频数 3；图表只引用 [10,20,30]，第二系列 [15,25,35] 虽在 E2:E4，但没有进入图表系列或缓存，不能列作两系列通过。标题、before/after 正常，内部 xlsx 与 PNG fallback 齐全。 |
| `chart/chartex-boxwhisker.docx`、`chart/chartex-boxwhisker-2.docx` | 原版单箱数据差异；新版为空白绘图区试件 | 原版可见一个 10 至 30 的蓝色箱体，中位数与均值为 20，但只有 [10,20,30] 一系列。新版已缩为三项缓存且有 Category 1/2/3 轴，却没有可见箱体；[15,25,35] 只存在工作簿 E2:E4，未进入图表系列或缓存。两份均为真实 boxWhisker/内嵌 xlsx/PNG fallback；新版不能替代原版，合格两系列箱线图仍未完成。 |
| `chart/chartex-funnel-2.docx` | 三类别单系列显示与缓存通过；公式残留范围 | 新版 PDF 只有 Category 1/2/3 三行，三条居中蓝条长度比例 1:2:3，条内类别正确，无多余行。layoutId=funnel，缓存 ptCount=3、数值 [10,20,30]，工作簿 C:D 对应，内部 xlsx 与 PNG fallback 齐全。但公式文本仍保留 A2:C17 / D2:D17，不能声称公式范围同步缩短。旧无后缀文件 16 行保留为试件；未声称两系列。 |
| `chart/chart-pasted-embedded.docx`、`chart/chart-pasted-linked.docx`、`chart/chart-pasted-picture.docx` | 已完成 UI 制作，结构通过 | 三份均来自真实 Excel 图表复制与 Word 指定粘贴。embedded 为 c:chart + 内部 xlsx；linked 为 c:chart + 外部 Excel 文件关系且无内嵌工作簿；picture 为普通 pic:pic + PNG，无图表或 OLE。图表显示完整，before/after 顺序成立。链接来源 `_assets/chart-paste-source.xlsx` 已保留。 |

新 UI 样本的 ZIP 证据见 `ui-samples-offline-check.json`。最终 OBSERVED.md 已合并新 UI 试件及成功版本，替换同名“未生成”记录。

上述 ChartEx 各版 PDF 观察见 `observed-chartex.md`。原版副本检查为 `chartex-package-check-20260906-182419-174280.json`，生成日志为 `chartex-native-template-results-20260906-182310-780.json`。02:52 的 `retry-modern-chart-data.ps1` 已实际调用 `prepare-modern-chart-dataset.ps1 -DeleteEmptyTailRows`，四个新 -2 原件及模板哈希未变；重设源范围均 E_FAIL，但工作簿修改与删空行已提交。新版副本检查 `chartex-package-check-20260906-185344-954989.json` 为 waterfall/funnel 三类别单系列约定通过，histogram/boxwhisker 因第二系列未进缓存而失败；boxwhisker 的 PDF 空白绘图区另构成视觉失败。日志为 `modern-chart-data-retry-20260906-185235-256.json`。不得将 setter 失败直接等同于全部数据更新失败，也不得将 E 列有数值等同于已绘制第二系列。

## SmartArt、公式与 OLE

- SmartArt 9 项目标结构和观察基本完整。`smartart-picture` 实际使用本机“图片题注列表”布局，属于具体布局替代；两个图片填充及 drawing part 自身关系齐全，应保留实际名称。三维 styled 样本确有 `a:effectLst` 与 `a:sp3d`。
- 公式 10 项均有成功版本：fraction、integral、matrix、inline、display-two（使用 -2 修正版）、builtin、latex、linear、styled（使用 -2 修正版）、in-table。fraction 的 `m:f/m:sSup/m:ctrlPr` 存在；积分省略 `m:chr` 使用 OMML 默认积分号，不是结构丢失。LaTeX 是真实输入模式而非伪装；linear 是真实线性公式。
- display-two 和 styled 的两份首版仍保留为失败试件；成功状态来自各自修正版的真实 Word 操作、保存包结构及 PDF 三方证据，不能套用旧 selfcheck 的 passed。
- OLE embedded、linked、ppt、with-text、in-table 有预览、ProgID 和实际激活/数据证据。linked 预览实际为 WMF；ppt 内嵌包实际为 `.sldx`，应登记这些真实 Word 形态，不改造成规格示例扩展名。
- `ole-icon.docx` 的阴影通用纸张图标为保留异常试件。`ole-icon-3.docx` 是外观修正版，且已通过其自身只读检查副本的实际对象激活及单元格读取验证，6 项 OLE 均有对应证据。

## 图片与 P1

- 图片 14 种已有合格文件；旋转项应使用 `image-rotated-2.docx`。`image-emf.docx` 通过实际 Excel 复制、Word 选择性粘贴“图片（增强型图元文件）”制作，普通 pic:pic 内嵌有效 EMF；其 Word PDF 图表内容和 before/after 顺序完整，裁剪节点为空，无旋转/翻转。证据见 `observed-canvas-image.md` 和 `ui-samples-offline-check.json`。
- 装饰性图片真实输出的是 `adec:decorative`，命名空间为 `http://schemas.microsoft.com/office/drawing/2017/decorative`，`val=1`；Word移除了该图原先的可选文字。规格写 `a16:decorative` 与真实前缀/命名空间并不相同，应保留原件并如实登记。
- linked-only 图片无内嵌 media，insert-and-link 同时有 `r:embed/r:link`；引用源 `_assets/tiny.png` 已保留。外部链接样本在别的机器能否解析其路径尚未测试，不能把本机可见推断为可移植。
- P1 12 项均有结构与目视证据；shapes 应选 `textbox-shapes-3.docx`，text-basic、strict-basic、content-controls 必须选 `-2` 修正版作为成功样本。原件的定位/平级编号/组合框问题已分别留证。
- `p1-final-package-results.json` 的“12 个 accepted / 12 passed”只是其结构检查结果，其中 shapes 还指向首版。完整通过的判断必须结合新版 `observed-p1-layout.md` 及 `textbox-shapes-3-offline-review.json`，不能只引用旧结构汇总。
- 页眉水印只在第三页出现；规格只要求加一个文字水印，未要求每个页眉变体都加，故不判未完成，但保留真实显示范围。第三节双栏的结构存在，短正文只占左栏，不能声称从 PDF 目视观察到双栏排版。
- 修订批注已补 Word 全部标记截图和带标记 PDF，插入/删除/格式修订、两批注、一回复、已解决状态有证据，可通过；不再把“默认 PDF 不显示修订”当未完成。
- 26 页 `misc/large-report.docx` 为本次真实核查记录的 Word 汇编，所有页已目视检查，包含 23 图表、表格、目录、页眉和连续页码。规格允许但不强制使用公开报告；此来源差异应说明，不能称为下载的公开报告。

## 推荐版本与保留试件

| 目标项 | 推荐作为成功样本的文件 | 必须保留并标为试件的文件 |
| --- | --- | --- |
| 画布形状 | `canvas/canvas-shapes-4.docx`（wpc 替代） | `canvas-shapes.docx`、`canvas-shapes-2.docx`、`canvas-shapes-3.docx`：画布在 before 上方 |
| 画布半尺寸 | `canvas/canvas-resized-2.docx`（wpc 替代） | `canvas/canvas-resized.docx`：实际缩为四分之一 |
| 旋转翻转图片 | `image/image-rotated-2.docx` | `image/image-rotated.docx`：最终 315°，不是最终 45° |
| 基础文字 | `text/text-basic-2.docx` | `text/text-basic.docx`：三级编号未形成层级 |
| Strict 基础文字 | `strict/strict-basic-2.docx` | `strict/strict-basic.docx`：同一编号问题 |
| 内容控件 | `sdt/content-controls-2.docx` | `sdt/content-controls.docx`：comboBox，不是 dropDownList |
| OLE 图标 | `ole/ole-icon-3.docx` | `ole/ole-icon.docx`：阴影中的通用图标 |
| 组合形状 | `shapes/textbox-shapes-3.docx` | `shapes/textbox-shapes.docx`、`shapes/textbox-shapes-2.docx`：before/特征/after 定位问题 |
| 单段双公式 | `math/math-display-two-2.docx` | `math/math-display-two.docx`：两个 m:oMathPara，每个一个公式 |
| 局部公式字号 | `math/math-styled-2.docx` | `math/math-styled.docx`：所有数学 run 均为 20 磅 |
| 墨迹转形状 | 暂无成功转换版本 | `ink/ink-to-shape.docx`：八边形轮廓仍由八份原生墨迹构成 |
| Chartex 旭日图 | `chart/chartex-sunburst-2.docx` | `chart/chartex-sunburst.docx`：缓存 #REF!、数值为空 |
| Chartex 瀑布图 | `chart/chartex-waterfall-2.docx`（三类别单系列） | `chart/chartex-waterfall.docx`：16 个位置与数字类别标签 |
| Chartex 漏斗图 | `chart/chartex-funnel-2.docx`（三类别单系列；公式仍到 17 行） | `chart/chartex-funnel.docx`：16 行与数字类别标签 |
| Chartex 直方图 | `chart/chartex-histogram-2.docx` 仅为较好部分样本，未完成两系列 | 两版均只绘制第一系列，新版第二系列仅在工作簿 |
| Chartex 箱线图 | 暂无合格两系列版本；原 `chart/chartex-boxwhisker.docx` 保留可见单箱 | `chart/chartex-boxwhisker-2.docx`：有坐标轴但无可见箱体 |

## 任务 B 与交付整理

任务 B 不是“9 份通过”：3 份正常、2 份需恢复、4 份普通打开失败；只得到 5 份 Word 另存文件。02 两份缺轴定义/工作簿；04 两份缺 picture 必需子节点且媒体相同；05/06 重复 content type default 与恢复提示一致。详见 `ROUNDTRIP.md` / `STRUCTURE.md`，不将结构原因假设写成经隔离实验确定的唯一原因。

最终 README 已说明实际状态，固定五列 OBSERVED.md 覆盖 110 份任务 A DOCX，每份失败试件也有独立行。加上任务 B 的 14 份文件，共 124 份 DOCX；92 个任务 A 项目均有候选，但不能据此判为全部通过。corpus-audit.json 检查 CRC、XML 解析、内部关系均无错误，输入原件 9/9 哈希匹配。ZIP 保留源资产与脚本，排除检查副本、缓存与所有 ~$ 锁文件。macOS/WPS P2 与 Rust/TS 接入不属于本次 Windows 工作。
