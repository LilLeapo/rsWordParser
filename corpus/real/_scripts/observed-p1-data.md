# P1 数据与报告观察记录

本表记录 text、table、strict、misc 的 Word 导出 PDF 实际观察，包含初版失败样本与修订版。PDF 由 Word 创建实例导出，本次检查未启动 Office、未重新保存原件。报告26页均已逐页查看；其余各文件均为1页。

结构证据来自 `p1-package-check-20260906-174102-549842.json`（12个初版）和 `p1-package-check-20260906-174323-324967.json`（text-basic-2、strict-basic-2、content-controls-2）。每次检查先复制原件，再只读解析副本 ZIP，并确认原件 SHA256 不变。SDT 视觉观察由另一分表记录，本检查确认修订版为真正的 `w:dropDownList`。

| 文件 | Word 版本（build）/ 平台 | 制作方式（UI / 脚本名） | 步骤要点 | 看到什么 |
| --- | --- | --- | --- | --- |
| `text/text-basic.docx` | 16.0.14334.20848 / Windows 11 x64（Office LTSC 2021，非 Microsoft 365） | create-p1.ps1（Word COM）；Word PDF实际观察 | 插入标题、正文、项目符号、编号和字符格式；首次保存；副本包检查 | **初版未达三级编号要求。** 1页；大号粗体“一级标题”、较小“二级标题”，正文“这是正文 Mixed English and 中文。”，一个实心圆项目符号；三段编号均显示为并列的“1.”，包中各段 ilvl 都为0，且 numId 为3、4、5。粗体、斜体、下划线、删除线、小号上标均可见，“彩色文字”为较大红色字；before 前文与 after 后文完整。其余文字特征正常，修订样本见 text-basic-2。 |
| `text/text-basic-2.docx` | 16.0.14334.20848 / Windows 11 x64（Office LTSC 2021，非 Microsoft 365） | create-p1.ps1（Word COM）；Word PDF实际观察 | 先完整定义三级模板，再整体应用列表，并逐段设置 ListLevelNumber=1/2/3；作为新原件保存 | 1页；两级标题、混排正文、实心圆项目符号与字符格式均完整。三段编号逐级缩进，实际显示“1. 编号一级”“1.1. 编号二级”“1.1.1. 编号三级”；保存后的编号层级与格式通过包检查。红色18磅“彩色文字”突出，before/after 标记完整，无明显重叠或截断。 |
| `text/text-custom-styles.docx` | 16.0.14334.20848 / Windows 11 x64（Office LTSC 2021，非 Microsoft 365） | create-p1.ps1（Word COM）；Word PDF实际观察 | 新建“我的标题”样式，基于标题1，将字体改为 Microsoft YaHei，应用后首次保存 | 1页；“我的标题示例”显示为黑色粗体标题，下方为“自定义标题后的正文”，上下各有 before/after 标记。包中存在名为“我的标题”的样式，其 basedOn 指向标题1，保存了自定义字体并确实应用于标题段落；布局正常。 |
| `table/table-styled.docx` | 16.0.14334.20848 / Windows 11 x64（Office LTSC 2021，非 Microsoft 365） | create-p1.ps1（Word COM）；Word PDF实际观察 | 建3×3表格，使用“网格表4 - 着色1”；启用标题行与镶边行；合并首行前两格；右下格插2×2嵌套表 | 1页；首行深蓝底、白色文字，合并区域内 R1 C1 与 R1 C2 分两行，右侧为 R1 C3；第二行浅蓝色镶边，第三行白底。右下单元格可见 N11/N12 与 N21/N22 的2×2内容，下方仍有 R3 C3；嵌套表内边框不明显。外框完整，before/after 均在表外。3行3列网格、gridSpan合并、嵌套2×2结构和样式条件均通过包检查。 |
| `strict/strict-basic.docx` | 16.0.14334.20848 / Windows 11 x64（Office LTSC 2021，非 Microsoft 365） | create-p1.ps1（Word COM），SaveAs2格式24；Word PDF实际观察 | 以基础文字特征创建 Strict Open XML 文档，首次保存 | **初版未达三级编号要求。** 1页；显示与 text-basic 初版一致，三段编号均为并列的“1.”，未形成层级。两级标题、项目符号、混排正文、粗斜体、下划线、删除线、上标与红色大字均可见。Strict 主命名空间确为 `http://purl.oclc.org/ooxml/wordprocessingml/main`；编号差异保留在原件，修订样本见 strict-basic-2。 |
| `strict/strict-basic-2.docx` | 16.0.14334.20848 / Windows 11 x64（Office LTSC 2021，非 Microsoft 365） | create-p1.ps1（Word COM），SaveAs2格式24；Word PDF实际观察 | 使用修正后的三级列表创建独立 Strict 样本 | 1页；视觉与 text-basic-2 一致，编号依次为“1.”、“1.1.”、“1.1.1.”并逐级缩进，其他字符格式和前后标记完整。Strict 命名空间、三级编号及基本文字特征均通过副本包检查，无明显布局异常。 |
| `misc/large-report.docx` | 16.0.14334.20848 / Windows 11 x64（Office LTSC 2021，非 Microsoft 365） | create-p1.ps1（Word COM）；Word PDF全26页实际观察 | 在Word中撰写本次语料验证方法和任务B实测结论；插入一份样式表与23份实际Word图表及其观察文字；生成目录、页眉、页码后首次保存 | **26页均已查看。** 第1页为“Windows Word 语料验证报告”、日期和完整目录，目录列到第26页；第2页为“验证方法与往返结果”，说明9份任务B输入的3正常/2恢复/4普通打开失败及环境局限，并说明饼图单系列例外；第3页显示蓝色样式表与嵌套表；第4至26页每页一个图表及对应观察记录，23种图表均实际显示。页眉统一为“Windows Word 语料验证报告 / 2026-09-07”，页码连续1至26；图表、正文、页眉页脚无明显重叠、截断或空白页。面积图橙系列覆盖蓝系列、圆环双环30%孔、日期标签2024/1/1等、浮动图右对齐、表格内图、无标题/无图例、红扇区与灰度/单色等差异均保留。该报告是本次真实检查内容的Word汇编，不是下载的公开报告；包中26页元数据、目录、表格、图表部件和页眉引用通过检查。 |

PDF视觉证据：`_previews/rendered/contact-text-1.jpg`、`contact-table-1.jpg`、`contact-strict-1.jpg`、`contact-misc-1.jpg` 至 `contact-misc-5.jpg`。报告原始逐页渲染为 `misc-large-report-p1.png` 至 `misc-large-report-p26.png`。
