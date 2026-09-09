# 08 · 桌面 Word 第二轮任务（交给 Windows 侧代理）

> **已完成**（2026-09-07 交付并接入，见 `docs/04` §15 与 `corpus/real/_round2/`）。收尾轮见 **`docs/09-real-word-round3.md`**。

> **读者**：上一轮（`docs/07`）的 Windows 侧代理。这份文档自含：不需要读仓库其他文件。**产出**：`real-word-round2-<yyyymmdd>.zip`
> 交回 macOS 侧。有疑问按字面做，把疑问写进 `README.md` 的「未完成 / 存疑」，不要自行改规格。
> 环境仍可用 Office LTSC 2021；若手边有 **Microsoft 365** 也请写明并做任务 E。

## 0. 上一轮的结果与这一轮为什么这么排

上一轮交付已全部接入：124 份文档在本引擎里**全部**往返字节相同、编辑后其他条目 CRC 不变，与旧实现的差分为 0。你们在任务 B 里
发现的两处恢复提示是本引擎的真 bug（`[Content_Types].xml` 里重复的 `Default Extension`），已修；`02-*` / `04-*` 打不开是旧实现
合成的底稿本身残缺，样本已改用你们做的真 Word 文档做底稿重新生成。画布是 `wpc:wpc`、装饰性图片是 `adec:decorative` 这两处是我们
规格写错，已改。

这一轮最有价值的事只有 Windows 上的 Word 能做：**用 Word 打开本引擎写出来的文件**。所以任务 B2 是最重的一项。其余是等桌面 Word
很久的两件事（toggle 复核、M7 需要的修订 / 字段语料）。

## 1. 交付物

```
real-word-round2-<yyyymmdd>/
  README.md            Word 精确版本、Windows 版本、日期、方法；未完成 / 存疑
  ROUNDTRIP2.md        任务 B2-a：9 份样本，每份一行（同上一轮 ROUNDTRIP.md 的表头）
  EDITED.md            任务 B2-b：944 份编辑后文档，每份一行（§2.2 的表头）；另附 edited-results.json（机器可读）
  TOGGLE.md            任务 C：8 份 fixture 的逐句读数（§3 的表头）
  _resaved/            Word「另存为」出来的副本：9 份样本全部 + B2-b 里抽检与失败的
  blank/ revisions2/ fields2/ sections2/ image2/ shapes2/ ink/   任务 D 的新文档
  m365/                任务 E（若有 Microsoft 365）
  screenshots/  _scripts/  _previews/（只放 PDF）
```

## 2. 任务 B2 · 用 Word 打开本引擎写出的文件（最重要）

输入 zip 里的 `_roundtrip/`：

- **B2-a**：`_roundtrip/*.docx` 9 份（`README.md` 逐份写了「我们做了什么 / 应看到什么」）。与上一轮做法相同：先开 `*-source.docx`
  再开样本，记有无修复 / 兼容提示、看到什么、图表的「编辑数据」能否打开且数字是否一致，每份另存到 `_resaved/`。
  这一版全部以真 Word 文档为底稿，**不应再有兼容性模式**，也不应再有任何恢复提示——有就是我们的 bug，请截图。
  `02-chart-setdata.docx` 这次有内嵌工作簿了：请务必记录「编辑数据」之后 Word 是否用工作簿里的旧值把图表刷回去。
- **B2-b**：`_roundtrip/edited/` 944 份 + `MANIFEST.md` / `manifest.json`。每份 = 上一轮你们做的一份真 Word 文档 + 本引擎的一种编辑
  + 本引擎保存。12 种编辑：`insert`（首段开头插字）、`newimage`（插浮动图片）、`newchart`（插柱形图）、`ink`（加一条墨迹层）、
  `deleteblock`（删第二个块，连带回收它的媒体 / part）、`chartdata`（只改图表缓存）、`replaceimage`（换图）、`insertrow` /
  `mergecells`（表格）、`header`（替换默认页眉）、`comment`（加批注）、`split`（拆段）。清单的「期望」列写了每份应看到什么。

### 2.1 怎么做 B2-b（944 份不可能全手看，分两层）

**第一层，全部 944 份，用 Word 对象模型批量开**（一份一秒）：`Application.DisplayAlerts = wdAlertsNone`，
`Documents.Open(path, ConfirmConversions:=False, ReadOnly:=True, AddToRecentFiles:=False, Visible:=False)`。每份记录：

| 列 | 取什么 |
| --- | --- |
| open | `ok` / `error`（异常文本原文） |
| compat | `Document.CompatibilityMode`（期望 15） |
| paragraphs | `Paragraphs.Count` |
| marker | `insert` 类：第一个有字段落的 `Range.Text` 是否以 `rsword✎ ` 开头；`header` 类：`Sections.Last.Headers(wdHeaderFooterPrimary).Range.Text` 是否含 `rsword 页眉`；`comment` 类：`Comments.Count` 是否 ≥ 1 且作者 `rsword`；`split` 类：段落数是否比底稿多 1；`deleteblock` 类：段落 / 表格 / 图形数是否比底稿少 |
| shapes | `InlineShapes.Count` 与 `Shapes.Count`（`newimage` / `ink` 应各 +1 个 Shape，`newchart` +1 个 InlineShape，`replaceimage` 不变） |
| chart | `newchart` / `chartdata` 类：`InlineShapes(i).Chart.ChartTitle.Text`、第一系列 `Values`；`chartdata` 类再执行一次 `Chart.ChartData.Activate()` 关闭后重读 `Values`，记录是否被工作簿旧值刷回 |

对象模型打开报错的，**再到 UI 里手开一次**，分清是「恢复提示（可恢复）」还是「无法打开」，截图放 `screenshots/`。

**第二层，抽检看渲染**：每种编辑各抽 3 份（尽量覆盖不同域：图表 / SmartArt / 画布 / 公式 / OLE / 墨迹 / 图片 / 表格 / 页眉）+
第一层里所有失败的，在 UI 里打开、按清单「期望」列核对、导出 PDF 到 `_previews/edited/`、另存到 `_resaved/`。

### 2.2 `EDITED.md` 表头

```
| 文件 | open | compat | 恢复提示 | marker / shapes / chart 的读数 | 抽检：看到什么（未抽检写 -） | 与期望是否一致 |
```

## 3. 任务 C · 桌面 Word 复核 toggle 属性（十分钟，但等了很久）

背景：ECMA-376 说 `b / i / strike / caps / smallCaps / dstrike / vanish` 这些属性在样式层级里按"为真的次数的奇偶"决定；上一轮之前我们只在
**Word 网页版**测过：`b` / `i` 两层都声明时**抵消**（不加粗），而 `strike / caps / smallCaps / dstrike` 两层都声明时**仍然生效**。
网页版与桌面版是两套渲染实现，请在桌面版复核。输入 zip 的 `fixtures/` 里有 8 份最小文档，每份几句话，每句话就是一个测点。

做法：打开文档，光标放进那句话里，看功能区对应按钮是否按下（加粗 / 倾斜 / 删除线 / 双删除线在"字体"对话框里）以及字体名框
（加粗时显示"宋体 (粗体)"），`caps` / `smallCaps` 直接看字形是否大写 / 小型大写，`vanish` 看那句话是否被隐藏（桌面版应当藏起来；
若开着"显示隐藏文字"请关掉再看）。每句话读两次（光标移开再移回）。

| 文件 | 看哪句 | 要读的属性 |
| --- | --- | --- |
| `toggle-other-toggles.docx`（**最重要**） | `i twice` / `i once`、`strike twice` / `strike once`、`caps twice` / `caps once`、`smallcaps twice` / `smallcaps once`、`dstrike twice` / `dstrike once`、`vanish twice` / `vanish once` | 各自的属性开没开 |
| `toggle-para-and-char.docx` | `para b + char b`、`para b only` | 加粗 |
| `toggle-docdefaults-and-para.docx` | `docDefaults b + para b`、`docDefaults b only` | 加粗 |
| `toggle-docdefaults-and-para-off.docx` | 每句 | 加粗 |
| `toggle-based-on-two-levels.docx` | `basedOn b + derived b`、`base b only` | 加粗 |
| `toggle-table-first-row.docx` | 表格首行与正文行 | 加粗 |
| `toggle-direct-off.docx` | `direct b=0 over style b`、`style b, no direct` | 加粗 |
| `sections-inherit-default.docx` | **第二页**的页眉显示什么 | 页眉文字 |

`TOGGLE.md` 表头：`| 文件 | 句子 | 属性 | 桌面 Word 里开/关（或页眉文字） | 与网页版结论是否一致 |`（网页版结论：`b` / `i` 两层抵消；
`strike` 一族两层仍开；`vanish` 未知；第二页页眉显示"第一节页眉"）。

## 4. 任务 D · 下一里程碑（修订 / 字段 / 分节）要用的真实文档

规矩同上一轮（`docs/07` §1：一份一个特征、`before 前文` / `after 后文` 夹着、小、不要检查文档、不要再开再存、`OBSERVED` 式记录写进
`README.md` 或各目录的 `OBSERVED.md`）。修订类请**开着修订**做，做完不要接受 / 拒绝（除非那一项要求）；两位作者 = 中途改一次
「文件 → 选项 → 用户名」。

| 文件 | 怎么做 | 自检（解包后应看到） |
| --- | --- | --- |
| `blank/blank-new.docx` | 新建空白文档，什么都不输，直接保存 | `document.xml` 只有一个空 `w:p` + `w:sectPr`；`styles.xml` / `settings.xml` / `theme1.xml` / `fontTable.xml` / `webSettings.xml` 都在 |
| `blank/blank-styles-used.docx` | 空白文档里各输入一行并套用：标题 1、标题 2、正文、项目符号、编号；然后**全部删掉**再保存 | 同上，但 `styles.xml` 里出现了 标题 1 / 标题 2 / 列表段落 等样式定义 |
| `revisions2/rev-insert-delete.docx` | 开修订：作者甲插一句、删一句；改用户名为作者乙再插一句 | `w:ins` × 2、`w:del`（内含 `w:delText`）、两个不同 `w:author` |
| `revisions2/rev-move.docx` | 开修订：剪切一段，粘到 `after` 之前 | `w:moveFrom` / `w:moveTo` + `moveFromRangeStart` 等 |
| `revisions2/rev-format.docx` | 开修订：一处加粗改色、一段改居中并加缩进、一段套上编号 | `w:rPrChange`、`w:pPrChange`、`w:numPr` 的修订 |
| `revisions2/rev-table.docx` | 开修订：2×2 表里插一行、删一行、合并两格、改表格边框颜色 | `w:trPr/w:ins`、`w:trPr/w:del`、`w:tcPrChange`、`w:tblPrChange` |
| `revisions2/rev-section.docx` | 开修订：改页边距与纸张方向 | `w:sectPrChange` |
| `revisions2/rev-accept-reject.docx` | 做三处插入修订，然后**接受第一处、拒绝第二处**，第三处留着 | 只剩一个 `w:ins`；正文能看出接受与拒绝的结果 |
| `revisions2/rev-comment-threads.docx` | 三条批注：第一条有两层回复且标为已解决，第二条批注整段，第三条批注一个词 | `comments.xml` 5 条、`commentsExtended.xml` 有 `paraIdParent` 与 `done` |
| `fields2/fields-seq-captions.docx` | 三张小图各插题注（引用 → 插入题注，标签"图"），再插一处交叉引用指向第 2 张的题注 | `SEQ 图` × 3、`REF` |
| `fields2/fields-index.docx` | 三处标记索引项，文末插入索引 | `XE` × 3、`INDEX` 字段 |
| `fields2/fields-toc-stale.docx` | 三级标题 + 目录；然后**改掉两个标题的文字**，不更新目录，保存 | `TOC` 字段的结果与正文标题不一致 |
| `fields2/fields-citations.docx` | 引用 → 管理源，加两条源（书、期刊）；正文插两处引文；文末插书目 | `CITATION` × 2、`BIBLIOGRAPHY`、`customXml/item1.xml` 有 `b:Sources` |
| `fields2/fields-page-in-footer.docx` | 页脚："第 PAGE 页 / 共 NUMPAGES 页"；正文三页 | 页脚 part 里 `PAGE`、`NUMPAGES` |
| `sections2/sections-breaks-zoo.docx` | 五节：分节符依次用"下一页"、"连续"、"偶数页"、"奇数页"；第 3 节勾首页不同并写首页页眉 | 五个 `w:sectPr`，`w:type` 各不同，第 3 节 `titlePg` + `headerReference type="first"` |
| `image2/image-z-order.docx` | 三张浮动图片叠放；把第 1 张"下移一层"、第 3 张"置于顶层" | 三个 `wp:anchor` 的 `relativeHeight` 次序与视觉一致 |
| `shapes2/textbox-linked.docx` | 两个文本框，"创建链接"，第一框写满溢到第二框 | 两个 `wps:txbx` 带 `id` / `seq`，或 `wps:linkedTxbx` |
| `ink/ink-to-shape-2.docx`（补做，可选） | 绘图 → 开"墨迹转形状"，**一笔**画闭合圆 | 变成 `wps:wsp` 椭圆；仍不成功就保留试件并说明 |

## 5. 任务 E · Microsoft 365 对照（有则做）

用 Microsoft 365 重做上一轮的 `chart/chart-column`、`smartart/smartart-list`、`math/math-fraction`、`image/image-wrap-square` 四份，
文件名加 `-m365`，放 `m365/`；`README.md` 写明 M365 的精确版本。没有 M365 就跳过并写明。

## 6. 顺序与完成标准

顺序：**B2-a → B2-b 第一层 → C → B2-b 第二层 → D → E**。完成标准：`EDITED.md` 944 行齐全（第一层每份必有）、`ROUNDTRIP2.md` 9 行、
`TOGGLE.md` 每句一行、任务 D 每份通过自检并有记录、`README.md` 有精确版本与未完成清单、zip 里没有 `~$` 锁文件。
