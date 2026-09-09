# Windows Word 第二轮往返核对

日期：2026-09-07，Asia/Shanghai。环境：Office LTSC Professional Plus 2021 x64（ProPlus2021Volume），Word 文件版本 / Click-to-Run 版本 16.0.14334.20848；对象模型 Version=16.0、Build=16.0.14334。Windows build 22631.3155，23H2。当前环境记录见 `environment.json`，本表不是 Microsoft 365 观察结果。

本轮 9 份文件包括 3 份底稿与 6 份修改样本。先观察对应底稿，再观察修改版；Word 可见且开启提示，记录打开及页面渲染。对象模型读取 CompatibilityMode、对象计数、图表数据，使用 Chart.ChartData.Activate() 激活内嵌工作簿，关闭工作簿后重读系列；不是将对象模型调用记成右键菜单操作。九份副本均由 Word SaveAs2 保存到 `_resaved/<文件名>-resaved-by-word.docx`，原件未覆盖。

| 文件 | 打开时有无修复 / 兼容提示（有则截图） | 看到什么（图表类型 / 标题 / 数值；图片位置与环绕；墨迹位置） | 与 README 期望是否一致 |
| --- | --- | --- | --- |
| `01-text-source.docx` | 正常打开；可见窗口未观察到修复或兼容提示；CompatibilityMode=15。此次已观察界面但未保存该文件截图 | before 前文、自定义加粗标题“我的标题示例”、“自定义标题后的正文”、after 后文可见；对象模型读到 6 段、无图形 | 符合当前真实 Word 底稿；已另存。输入 README 开头“两段文字”的旧描述已过期 |
| `01-chart-insert-bar.docx` | 正常打开；未观察到修复或兼容提示；CompatibilityMode=15。见 `screenshots/01-chart-before-0.jpg` | 簇状柱形图“季度销售”；Q1/Q2/Q3，“华东”120/88.5/96，“华南”70/空/110，Q2 橙色柱缺位；1 个 InlineShape。ChartData.Activate 成功，内嵌工作簿名称、类别及单元格数值一致；关闭后系列保持 | 符合；已另存；图表激活和单元格读数见 `_readouts/01-chart-insert-bar-chartdata-1.json` |
| `02-chart-source.docx` | 正常打开；未观察到修复或兼容提示；CompatibilityMode=15。见 `screenshots/02-chart-source-0.jpg` | 簇状柱形图“销售统计”；Category 1/2/3，Series 1 为 10/20/30、Series 2 为 15/25/35，右侧图例。内嵌工作簿激活成功，关闭后数据不变 | 符合当前底稿；已另存。当前文件确有内嵌工作簿，输入 README 的旧说明不适用 |
| `02-chart-setdata.docx` | 正常打开；未观察到修复或兼容提示；CompatibilityMode=15。前后截图为 `screenshots/02-chart-setdata-before-0.jpg` 与 `screenshots/02-chart-setdata-after-0.jpg` | 初开标题“已改标题 Edited”，第一系列“改名系列”9/8/7。ChartData.Activate 成功，工作簿仍是 Series 1、10/20/30；执行工作簿 Close(false) 后，可见蓝柱和对象模型读数均回到 10/20/30，名称回到 Series 1，标题仍为“已改标题 Edited”；第二系列一直为 15/25/35 | 初开与缓存编辑期望一致；**确认工作簿旧值会回刷第一系列及名称**。已在激活后另存；保存包也保留回刷后的值。完整前后读数见 `_readouts/02-chart-setdata-chartdata-1.json` |
| `03-image-insert-inline-and-square.docx` | 正常打开；未观察到修复或兼容提示；CompatibilityMode=15。见 `screenshots/03-image-insert-0.jpg` | 两块绿色矩形可见：一块随文居中，一块浮动靠右。对象模型读到 1 个 InlineShape、1 个 Shape；浮动图 144×72 pt、WrapFormat.Type=0（四周型） | 插入与位置、环绕属性符合；已另存。短正文不足以单凭页面验证长文本的完整环绕形态 |
| `04-image-source.docx` | 正常打开；未观察到修复或兼容提示；CompatibilityMode=15。见 `screenshots/04-image-source-0.jpg` | before/after 文字右侧可见青绿 / 黄色双色图片；1 个浮动 Shape，144×72 pt，Left=180、Top=0 pt，WrapFormat.Type=0 | 底稿正常；已另存 |
| `04-image-replace.docx` | 正常打开；未观察到修复或兼容提示；CompatibilityMode=15。见 `screenshots/04-image-replace-0.jpg` | 原双色图片变成均匀绿色；位置、大小、环绕与底稿目视一致。对象模型同为 1 个 Shape，144×72 pt，Left=180、Top=0 pt，WrapFormat.Type=0 | 替换内容及保持布局符合；已另存 |
| `05-ink-insert.docx` | 正常打开；未观察到修复或兼容提示；CompatibilityMode=15。见 `screenshots/05-ink-insert-0.jpg` | 两块绿色浮动矩形覆盖首段区域，未把正文挤开。对象模型读到 aidocs-ink 1/2 两个 Shape，各 150×60 pt，偏移分别 (30,-7.5) 与 (225,15) pt，WrapFormat.Type=3；这是引擎用图片表示的墨迹层 | 符合本项约定；本轮无需恢复；已另存 |
| `06-chart-insert-line-pie.docx` | 正常打开；未观察到修复或兼容提示；CompatibilityMode=15。上部截图见 `screenshots/06-chart-line-pie-top-0.jpg` | 上方饼图“占比”、下方带标记折线“趋势”，两图均已在 Word 界面观察；一月/二月/三月均为 3/5/2，饼图扇区对应 30%/50%/20%。2 个 InlineShape，类型分别 5、65；两个内嵌工作簿均激活成功，读数一致，关闭后不变 | 类型、标题、数值符合；顺序实际为饼图在前、折线在后；本轮无需恢复；已另存 |

## 证据与只读检查

UI 观察汇总为 `_scripts/roundtrip-ui.json`；每份初开对象模型读数为 `_readouts/<文件名>.json`，图表工作簿激活前后读数带 `-chartdata-<序号>` 后缀。01-text-source 的界面观察只在操作记录中，未保存截图；其余截图按表中路径引用。没有修复提示，因此本轮没有修复对话框截图。

`_scripts/resaved-roundtrip-audit.json` 确认九份预期另存文件全部存在，9/9 另存件 SHA-256 与原件不同，9/9 原件 SHA-256 与打开前的输入审计一致。使用 .NET ZipArchive 只读解压全部条目，检查 CRC32、长度并解析 XML / rels，含内嵌 XLSX：共 190 个条目、181 个 XML / rels，CRC 错误 0、XML 解析错误 0、重复 ZIP 名称 0。02 修改版另存件的 chart1.xml 实际保存“已改标题 Edited”、Series 1、10/20/30。此检查未再用 Word 打开或保存另存件。

目前仅六份 B2-a PDF 导出到 `_previews/roundtrip/`：02-chart-setdata、03-image-insert-inline-and-square、04-image-source、04-image-replace、05-ink-insert、06-chart-insert-line-pie；未声称九份均有 PDF。02 的 PDF 和另存件均属于工作簿激活之后的状态。

本表以当前 v2 输入、当前 Word 读数和本轮 UI 观察为依据。输入 `_roundtrip/README.md` 开头仍有合成底稿及无内嵌工作簿的旧文字，与末尾“已全部改为真实 Word 文档”的更正冲突；本轮没有把上一轮的打开失败或恢复结果移用到当前文件。
