# _roundtrip · 本引擎写出的文件，等桌面 Word 核对

由 `cargo test -p rsword --test roundtrip_samples -- --ignored` 生成（源文档来自 `corpus/synthetic`，是 TS 测试拼出来的 XML，
所以每份样本旁边放着未改动的 `*-source.docx`：先开源文件再开样本，把「源文件本身就有提示」与「我们改坏了」分开）。
请在 Windows 桌面 Word 里逐个打开，结果记到 `corpus/real/ROUNDTRIP.md`（有提示就截图放 `screenshots/`）。

| 文件 | 我们做了什么 | 在 Word 里应看到 / 请核对 |
| --- | --- | --- |
| `01-text-source.docx` | 源文件（两段文字：第一段 / 第二段） | 基线：有无提示 |
| `01-chart-insert-bar.docx` | 在第一段后插入簇状柱形图「季度销售」：类别 Q1–Q3，系列「华东」120 / 88.5 / 96、「华南」70 / 空 / 110（图表 part + 内嵌 xlsx + 关系由本引擎生成） | 图表可见、标题与两系列正确、Q2 华南是空档；右键图表 → **编辑数据** 能打开内嵌工作簿且数字一致；无修复提示 |
| `06-chart-insert-line-pie.docx` | 同一段后插两张：折线「趋势」、饼图「占比」，类别一月 / 二月 / 三月，值 3 / 5 / 2 | 两张都可见，类型正确 |
| `02-chart-source.docx` | 源文件（TS 生成的图表文档；**没有内嵌工作簿**，所以「编辑数据」报错是正常的） | 基线 |
| `02-chart-setdata.docx` | 只改图表 part 的**缓存文本**：标题「已改标题 Edited」、第一系列改名「改名系列」、值 9 / 8 / 7；内嵌工作簿**没改** | 打开时图表按缓存画（新标题、新值）；然后 **编辑数据** 打开工作簿——请记录 Word 是否用工作簿里的旧数字把图表刷回去（这是已知的设计取舍，要知道 Word 的实际行为） |
| `03-image-insert-inline-and-square.docx` | 第一段后插两张 192 × 96 px 的图（同一张 1 × 1 PNG 拉伸成纯色块）：一张随文居中，一张四周型环绕靠右 | 两块纯色小图都可见；第二张浮动在右侧、文字环绕 |
| `04-image-source.docx` | 源文件（一张环绕图片） | 基线 |
| `04-image-replace.docx` | 把那张图的媒体换成 1 × 1 PNG（裁剪窗删掉、填充窗清空） | 图片位置与环绕不变，内容变成纯色块；无提示 |
| `05-ink-insert.docx` | 在第一段上加两条「墨迹」：本引擎的墨迹是 `wp:anchor` 浮动图片（`aidocs-ink`），偏移 (40, −10) px 与 (300, 20) px，各 200 × 80 px | 两块纯色浮动图片压在第一段文字**前方**、不挤开文字；无提示 |

核对完请另存一份（`<名>-resaved-by-word.docx`）放回这个目录：我们拿它看 Word 重写后哪些字节变了。

## 第一轮 Word 核对（2026-09-07，Office LTSC 2021）的结论与修正

结果全文在 `../ROUNDTRIP.md` / `../STRUCTURE.md`。要点：

| 样本 | 结果 | 我们的处理 |
| --- | --- | --- |
| `01-*`、`03-*` | 正常打开、内容与期望一致（图表「编辑数据」能打开内嵌工作簿且数字一致） | 通过 |
| `05-ink-insert`、`06-chart-insert-line-pie` | 打开弹「发现无法读取的内容」，恢复后内容正确 | **真 bug**：`[Content_Types].xml` 里写了两条同扩展名的 `Default`（一次会话补两个媒体 / 工作簿）。已修（`ensure_default_type` 看活 DOM + 同步缓存），本目录的两份已**重新生成（v2）**，请下一轮再开一次 |
| `02-*`、`04-*` | 源文件本身就打不开（TS 合成的图表没有坐标轴、`pic:pic` 缺 `nvPicPr` / `spPr`） | 不是引擎问题。样本底稿已改为真实 Word 文档（`chart-column.docx` / `image-wrap-square.docx` / `text-custom-styles.docx`），全部样本都已重新生成 |

`*-resaved-by-word.docx` 是 Word 对**第一版**样本的另存件，与现在目录里的 v2 原件不再逐字节对应；留着它们是为了看 Word 重写了哪些字节
（例如 Word 去掉了重复的 `Default`、给 run 补 `w:noProof`）。
