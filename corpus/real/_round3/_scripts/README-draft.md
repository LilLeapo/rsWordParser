# Windows Word 第三轮实测交付

记录日期：2026-09-07T13:45:48.3823108+08:00。本说明生成于 2026-09-07T07:22:44.459785+00:00。结论以引用的实际记录为准；未完成和存疑项目单列如下。

## 环境与方法

- Office LTSC Professional Plus 2021，ProPlus2021Volume，x64。
- WINWORD.EXE 文件版本：16.0.14334.20848；Word 对象模型 Version=16.0，Build=16.0.14334。
- Windows：Microsoft Windows 11 专业版，版本 10.0.22631，Build 22631，64 位。
- 本轮使用 Windows 中的真实 Word。文档操作、保存、转换及对象模型读数与实际 UI 截图分开记录。未运行、构建或修改 rsWordParser；没有 Microsoft 365 对照。
- DOCX 仅由 Word 创建或另存；包内 XML、哈希和 ZIP 检查均为只读。原始输入和前两轮底稿的保存状态由独立哈希审计核实。
- Word 导出 PDF 是独立的版面证据。修订 PDF 通常呈现最终正文，修订气泡和标记以 Word UI 截图及原生 XML 为证。PDF 导出后不再保存 DOCX。

## 输入范围

- 输入 ZIP SHA256：`D52DABAAF11627127AA6CBB04398FE84E5F45001FC94851D2E69108EE7AA6C09`。
- 已核对 1564 个解包文件；DOCX 1560 份，其中 edited/ 为 1544 份、fixture 为 16 份。
- manifest 实际引用 180 份底稿：110 份第一轮原件、53 份第二轮 Word 另存件、17 份第二轮新建样本。任务书写的 127 未计入 53 份另存件。
- manifest 共 1556 行，1544 行为 generated。11 个 ChartEx chartdata 操作不支持而跳过；fields-toc-stale--deleteblock.docx 因引擎 FLD_STRAY_END 保存失败，未生成。这 12 行不属于 Word 打开失败。
- UI 计划共 60 个唯一文件：36 个常规样本、13 个必须复验样本、11 个 M7 额外样本。17 份 M7 底稿没有 chartdata 派生文件，无法提供这一类第 12 个额外样本。
- 八份原始 fixture 与第二轮逐字节相同。compat15 版本只新增 settings.xml 及必要的关系和内容类型登记，document.xml/styles.xml 字节保持不变。

## 任务 A

已记录 24/24 份 Word 原生保存件，8 个 case；结构要求全部通过：True。详见 REVISIONS.md、_scripts/task-a-final-summary.json 和 _readouts/revfix-inspection.json。

实际 Word 结果：

- Literal same-location cut/paste in table-and-move produced real moveFrom/moveTo markup and paired source/destination ranges.
- RejectAllRevisions in table-and-move restores original rows 2/3 and removes the inserted row, but retains the merged first row.
- Deleting the section break retains the later section landscape page setup.
- Tracked PDFs show final content; revision markup observations come from GUI screenshots and package XML.

与字面界面操作的差别：

- Task A document operations used native Windows Word COM APIs, including AcceptAllRevisions/RejectAllRevisions, rather than clicking each requested ribbon command.
- Section-break deletion used Word Range.Delete on the break character, not a keyboard deletion in Draft view.
- Picture movement used native Word shape positions and a locked aspect ratio with half width, not mouse dragging with Shift. Saved OOXML coordinates are reported, including Word rounding.
- run-edits/accepted.docx was reopened read-only for its GUI screenshot after a window-activation interruption; it was never saved again.
- move-resize/before screenshot contains a Start panel outside the inspected document content. Independent PDF evidence is unobstructed.

## 任务 B

当前 COM 记录 1544/1544，open=ok 1544，open error 0；独立 UI 记录 69。详见 EDITED3.md、edited3-results.json 和原始 JSONL。
批量打开禁用提示，因此不能从 COM 成功推断没有恢复提示。13 份重点复验与全部抽检的恢复结论来自实际 Word UI。chartdata 保留激活前的图表值、内嵌工作簿值及关闭工作簿后的读数，回滚现象单独报告。
原始检查分类：pass 1526，mismatch 7，incomplete 11。
其中 7 个 ink 计数 mismatch 经独立 XML/代码复核，来自检测器“底稿数量 + 1”的假设不适用于覆盖列表替换语义，不能据此判引擎编辑失败。原始读数及 mismatch 均保留；复核后的解释为 pass 1533，incomplete 11。
11 个 incomplete 均为 ChartEx 底稿的 newchart 派生件，保留原有 ChartEx 对象模型读数限制；这不是 Word 打开失败。
实际 COM 激活前后读数显示，chartdata 中 40/40 份在激活内嵌数据工作簿后回到各自底稿的第一系列值。激活前的请求值与图表标题读取成功，不代表工作簿内容已同步。
另对 7 个抽检 chartdata 样本独立解析图表缓存及内嵌工作簿，确认输入中的两者不一致，详见 _scripts/edited3-chart-audit.json。这项 7 份结构复核与上述 40 份 COM 激活读数是不同证据范围，也与 ink 的计数假阳性复核分开。
重点复验：9/9 个此前恢复提示样本在实际 UI 打开时没有恢复提示；4/4 个指定图表样本的激活前标题/数据与请求一致。工作簿激活后的回滚结论单列如上。

## 任务 C

当前模式 15 测点 25/25；与第二轮一致 25，不同 0。转换交叉验证 12/12，三方一致 12/12。详见 TOGGLE15.md 和 _scripts/task-c-summary.json。
第二轮 toggle-para-and-char.docx 未记录精确的 CompatibilityMode 数值，仅标题栏证实兼容性模式。历史记录不补造数值。

## 任务 D

`ink2/ink-to-shape-2.docx`：已保留原生结果，目标未达成。

在 Word 绘图选项卡启用“墨迹转形状”，执行一次连续拖动，得到并保存一条原生直线墨迹，未出现转换后的圆形。

当前绘图接口只支持起点到终点拖动，不能提供连续曲线路径，因此未完成一笔闭合圆；该直线试件不能判断 Word 是否能转换正确画出的圆。

独立结构检查：保留 1 条原生墨迹，存在 w14:contentPart，没有 wps:wsp 圆形，目标结构未通过。

`comments2/comment-nesting.docx`：已保留原生结果，目标未达成。

先用 Word COM 创建正文及三条根批注；随后在界面点击第一条根批注的“答复”，再点击第一条回复自己的“答复”，最后在界面解决整个线程。两次回复未使用 COM Replies.Add。

实际执行了回复上“答复”按钮，但保存结果把两条回复都挂在同一根批注下，未保留第二层父子关系；第一线程已成功标为已解决。

独立结构检查：5 条批注、3 条根批注，最大回复深度 1，目标深度应为 2。
截图中另一桌面应用遮挡了页面下部；第一线程及点击的答复、解决控件仍可见，初始图可见三条根批注。独立 PDF 未被遮挡。

逐步操作、首次保存哈希及截图索引见 [_scripts/task-d-results.json](_scripts/task-d-results.json)；独立结构和 PDF 复核见 [_readouts/task-d-inspection.json](_readouts/task-d-inspection.json)。两份 DOCX 保存后均未再次保存。

## 完整性与环境恢复

本地目录的独立完整性审计已通过，报告位于交付目录外：`C:/word/round3-work-20260907/final-independent-audit.json`。此结论验证文件、哈希和证据覆盖，不会把已声明的未达成事项变成已达成。
ZIP 打包后 CRC 和逐文件哈希核对使用单独的外部报告：`C:/word/round3-work-20260907/final-package-audit.json`。本地目录审计与 ZIP 校验是两项不同的记录；本 README 不声明 ZIP 已生成或通过校验，打包后不再改写它。
环境恢复：8 项原始设置逐项回读一致，environment.json 保持不变；任务 Word 实例剩余文档 0，Word 退出记录为 True。详见 _readouts/settings-restored.json。

## 未完成 / 存疑

- 任务 A：原生文件和结构要求已完成，但部分操作使用 Word COM；分节符删除、图片移动缩放等未按字面鼠标或键盘流程执行，差别详见任务 A。
- 任务 B：11 份 ChartEx 底稿的原有图表 COM 读数仍不完整。
- 任务 B：40 份 chartdata 存在激活内嵌工作簿后的数据回滚；前后读数已保留。
- ink2/ink-to-shape-2.docx：当前绘图接口只支持起点到终点拖动，不能提供连续曲线路径，因此未完成一笔闭合圆；该直线试件不能判断 Word 是否能转换正确画出的圆。
- comments2/comment-nesting.docx：实际执行了回复上“答复”按钮，但保存结果把两条回复都挂在同一根批注下，未保留第二层父子关系；第一线程已成功标为已解决。

## 证据边界

- REVISIONS.md：逐 case、逐文件的实际正文、操作和结构结果。
- EDITED3.md / edited3-results.json：1544 份输入的 COM 读数、实际抽检观察和与第二轮对照。
- TOGGLE15.md：25 个模式 15 测点及 12 个 Word 转换交叉验证。
- screenshots/：实际 Word UI 截图与独立 PDF 渲染证据，文件名区分来源。_previews/ 只存 PDF。
- _scripts/ 与 _readouts/：作者操作日志、原始读数、复核脚本、只读审计及未达成项。
