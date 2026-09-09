# Windows Word 往返核对

日期：2026-09-07，Asia/Shanghai。环境：Word / Office LTSC Professional Plus 2021，64 位，Click-to-Run build 16.0.14334.20848；Windows 11 专业版 10.0.22631。并非任务说明要求的 Microsoft 365，结果只能代表这个实际环境。

Word 对象模型负责打开、属性读回和另存；界面直接检查渲染、图表编辑数据、打开错误与恢复提示。所有原始 DOCX 保留，另存文件使用规定的 `-resaved-by-word.docx` 后缀。恢复后才可打开的文件不计为无提示通过。

| 文件 | 打开时有无修复 / 兼容提示（有则截图） | 看到什么（图表类型 / 标题 / 数值；图片位置与环绕；墨迹位置） | 与 README 期望是否一致 |
| --- | --- | --- | --- |
| `01-text-source.docx` | 已打开的基线正常显示，标题栏为兼容性模式；见 `screenshots/01-text-source-20260907.png` | 第一段、第二段，各占一行；1 页 | 文字符合；已另存 |
| `01-chart-insert-bar.docx` | 正常打开，无修复对话框；兼容性模式 | 簇状柱形图，标题“季度销售”；类别 Q1、Q2、Q3；蓝色“华东”120 / 88.5 / 96，橙色“华南”70 / 空 / 110；Q2 橙色柱缺位，无图例，位于第一段和第二段之间。右键“编辑数据”及 Word ChartData.Activate 打开内嵌 Excel，A1:C4 的名称、类别和值与图一致，关闭后数值保持。见 `screenshots/01-chart-insert-bar-open.png`、`01-chart-insert-bar-data.png`，系列数值的 Word 辅助功能记录见同名 `-accessibility.txt` | 符合；已另存 |
| `06-chart-insert-line-pie.docx` | 普通打开失败，UI 提示“发现无法读取的内容，是否恢复”；见 `screenshots/06-chart-insert-line-pie-recovery-prompt.png`；选择恢复后生成未命名文档，兼容性模式 | 恢复后上下两张图：上方饼图“占比”，蓝 / 橙 / 灰三块约 30% / 50% / 20%；下方折线“趋势”，一月 / 二月 / 三月为 3 / 5 / 2，带标记和光滑连接线；第一段、第二段均保留。各图通过 ChartData.Activate 分别打开内嵌 Excel，系列2 / 系列1 的值均为 3 / 5 / 2。见 `screenshots/06-chart-insert-line-pie-recovered.png` | **无修复要求不符合**；恢复后两图类型、标题、数值符合，顺序为饼图在前、折线在后；已将恢复结果另存 |
| `02-chart-source.docx` | Word 对象模型与 UI 普通打开均失败：“Word 在试图打开文件时遇到错误”，提示检查权限 / 内存 / 文本恢复转换器；见 `screenshots/02-chart-source-open-error.png` | 没有打开文档，不能观察图表或执行编辑数据 | 源文件基线失败；无法另存 |
| `02-chart-setdata.docx` | 与源文件相同的普通打开错误；见 `screenshots/02-chart-setdata-open-error.png` | 没有打开文档，不能核对新标题 / 新值。只读包检查确认此样本和源文件均没有内嵌工作簿或 externalData，无法验证“旧工作簿数字刷回缓存”的情形 | **未能验收**；失败与源文件共同存在，无法另存；工作簿刷回实验缺少输入前提 |
| `03-image-insert-inline-and-square.docx` | 正常打开，无修复对话框；兼容性模式 | 两块浅绿色半透明矩形均可见，尺寸各 192 x 96 px；左图随文，右图靠正文区域右侧。Word 对象模型读到一张 inline 144 x 72 pt、另一张浮动 144 x 72 pt 且 WrapFormat.Type=0（四周型）。第一段在上方、第二段在下方；文字较短，不能单凭页面证明长文本环绕形态。见 `screenshots/03-image-insert-inline-and-square-open.png` | 插入、尺寸及环绕属性符合；已另存 |
| `04-image-source.docx` | Word 对象模型与 UI 普通打开均失败，通用打开错误；见 `screenshots/04-image-source-open-error.png` | 没有打开文档，无法观察源图片位置或环绕 | 源文件基线失败；无法另存 |
| `04-image-replace.docx` | 与图片源文件相同的普通打开错误；见 `screenshots/04-image-replace-open-error.png` | 没有打开文档，无法观察替换效果。只读包检查发现源文件与修改版的媒体字节相同，均为 1 x 1 半透明绿色 PNG，无法用这组文件验证肉眼内容变化 | **未能验收**；无法另存，输入也没有实际媒体内容变化 |
| `05-ink-insert.docx` | 普通打开失败，UI 提示“发现无法读取的内容，是否恢复”；见 `screenshots/05-ink-insert-recovery-prompt.png`；选择恢复后可显示，兼容性模式 | 恢复后两块浅绿色浮动矩形，第一段、第二段仍连续排列，没有被矩形挤开。Word 读回 `aidocs-ink 1/2`，各 150 x 60 pt；偏移分别 (30,-7.5) 和 (225,15) pt，即 (40,-10) 和 (300,20) px；包检查 wrapNone、behindDoc=0。它们是图片，并非原生手写笔画。见 `screenshots/05-ink-insert-recovered.png` | **无修复要求不符合**；恢复后位置、大小和文字前方覆盖属性符合；已将恢复结果另存 |

## 只读结构检查

`_scripts/inspect-roundtrip.ps1` 与 `_scripts/roundtrip-structure.json` 记录每份原件的 SHA256、关系、图表缓存 / 工作簿、绘图属性和媒体解码结果。9 份交付原件均与克隆仓库对应原件一致。

- `05-ink-insert` 有重复的 `Default Extension="png"`；`06-chart-insert-line-pie` 有重复的 `Default Extension="xlsx"`。Word 恢复另存后这些重复项消失。此变化与恢复提示相关，但未通过修改原件实验断言它们是唯一原因。
- `02-chart-*` 中 barChart 引用轴 111 / 222，却缺少 plotArea 内 catAx / valAx 定义。
- `04-image-*` 中 pic:pic 缺少必要的 pic:nvPicPr 和 pic:spPr。

这些是失败样本的结构证据，不是对引擎的修复。本次没有编译或运行 Rust 项目，也没有改写源文件 ZIP/XML。
