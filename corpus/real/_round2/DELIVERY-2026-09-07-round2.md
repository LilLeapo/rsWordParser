# Windows Word 第二轮实测

执行日期：2026-09-07（Asia/Shanghai）。本交付在 Windows 桌面 Word 中执行，没有编译、运行或修改 rsWordParser。只在本机整理文件，没有向外部发送。

## 环境与输入

- Windows 11 专业版 23H2，64 位，build 22631.3155。`environment.json` 的注册表 ProductName 仍写 Windows 10 Pro；Win32_OperatingSystem.Caption 实际为 Microsoft Windows 11 专业版。
- Office LTSC Professional Plus 2021 x64，ProductReleaseIds=ProPlus2021Volume；WINWORD.EXE / Click-to-Run 精确版本 16.0.14334.20848。COM Version=16.0、Build=16.0.14334。
- 输入为 `real-word-round2-inputs-20260907.zip`，SHA-256：`76BC84CE148648E4CC29D50C0C368FD4200D036CAFEE80793580B07C51DE7CA3`。
- B2-b 基线来自上一轮真实 Word 语料 `real-word-corpus-20260906`。本轮 manifest 有 944 份 generated、11 条 skipped，944 份均有记录，使用的 110 份基线均存在。

## 方法与记录

`ROUNDTRIP2.md` 是 B2-a 的 9 行记录；`EDITED.md` 与 `edited-results.json` 是 B2-b 的 944 行最终记录；`TOGGLE.md` 是 8 份 fixture、25 个测点的逐句结论。原始读数与脚本保存在 `_readouts/`、`_scripts/`，屏幕证据在 `screenshots/`，Word 导出的 PDF 在 `_previews/`。

B2-a 逐份可见打开，先观察底稿，再观察编辑版；9 份都由 Word SaveAs2 另存。B2-b 第一层按任务书使用 Word COM、只读、隐藏窗口、DisplayAlerts=0 打开全部 944 份；136 份图表相关文件补做可见打开读取，因为隐藏窗口初读部分 Chart 对象为空。初读记录仍保存在 `edited-results-hidden-pass.json` 和对应 `hidden_pass_record`，没有将空值伪装成成功读数。未进行 UI 抽检的行不宣称无恢复提示或渲染通过。

B2-b 第二层计划每种编辑 3 份，共 36 份；再覆盖第一层全部 9 份失败，其中一份与计划重合，最终有 44 份独立 UI 样本，44 套截图、PDF、另存件与读数均齐全。9 份正常打开失败的样本都在原生文件对话框打开时显示恢复提示，并成功恢复。UI 操作用 Windows Word 的原生窗口与文件对话框；对象计数、图表工作簿激活、另存和 PDF 导出使用 Word 对象模型。报告分别标明 UI 观察、COM 读数、PDF 页面核验和 XML 结构检查。

任务 D 使用 Word COM 创建文档并首次保存，包自检使用共享只读文件流，不修改 ZIP/XML。首次保存后导出 PDF、查看当前 Word 页面并关闭，不再打开并保存原件。作者名通过 Word UserName 属性切换，而不是逐次点击“文件 → 选项”；为避免登录身份覆盖作者名，制作双作者修订时临时开启 Options.UseLocalUserInfo，结束后已恢复原值 false。字段、题注、索引、书目源使用对应 Word 原生 API。具体步骤、异常与自检结果在各目录 `OBSERVED.md` 和 `_scripts/task-d-results.json`，直接页面观察另在 `_scripts/task-d-ui.json`；设置恢复和十份任务文档不再保存的关闭记录见 `_readouts/word-session-finish.json`。

任务 D 的 PDF 以最终内容模式导出，不显示修订标记和批注气泡，不能据此证明修订作者、类型或批注层级。这些结论使用 DOCX 结构和实际 Word UI；UI 修订展开截图另行保存。所有首次保存后的界面查看、导出操作都未保存回原件。

任务 D 正式目录有 17 份原生 Word 文档，17 份直接 UI 观察、17 份 PDF 共 23 页独立目视复核均齐全，首次保存哈希 17/17 保持一致。制作自检和独立结构复核均为 16/17 通过；批注的两层嵌套要求未满足。汇总见 `_scripts/task-d-final-summary.json`，独立 PDF/结构证据见 `_readouts/task-d-pdf-review.json`。

表格样本通过两种仅边框颜色不同的原生表格样式，将黑色改为红色并生成真实 tblPrChange。目录样本仅替换正文标题的前缀，保留三个有效书签，目录仍显示旧标题；文本框在设置相对坐标基准后定位，避免覆盖正文。这三份重新制作并验证后的首次保存件按字节复制到正式位置，未再经 Word 保存；旧件、失败尝试及当时记录保存在 `_trials/`，采用记录见 `_readouts/task-d-adoptions.json`。其中早期批次的文件共享哈希与 ZIP 程序集错误属于辅助脚本问题，不计作 Word 打开失败。

## 主要发现

- B2-a 9 份均正常打开，CompatibilityMode=15，未观察到恢复提示。`02-chart-setdata` 初开第一系列为 9/8/7；激活内嵌工作簿并关闭后，Word 回刷为 Series 1、10/20/30，编辑后的标题仍保留。
- B2-b 第一层为 935 份 ok、9 份 error。失败集中在三份原生 Ink 底稿的 newimage/newchart/ink 编辑。结构检查发现新增 wp:docPr id=1 与原生 Ink id 冲突；这是相关性证据，未通过隔离变量实验确定单一根因。
- 9 份恢复件的原生 Ink 保留审计全部通过：highlighter 每份 1→1、pen 每份 2→2、ink-to-shape 每份 8→8；原 Ink XML 字节、关系目标内容、媒体内容、尺寸及水平/垂直位置均保持。Word 重编号/重命名没有被误记为内容丢失。精确逐 part 差异在 `_scripts/final-independent-audit.json` 的 recoveredInk 字段。
- 26 份 chartdata 的内嵌工作簿均可激活；23 份从缓存 11/21/31 回到基线 10/20/30。bubble、scatter、scatter-lines 初读已为 10/20/30。无标题图表仍无标题。详见 `CHART-DERIVED.md`。
- canvas-floating--newimage 的新绿色图被原白色画布遮挡约 71.1%，上部约 28.9% 可见；这有 Word 页面和 PDF 绘制顺序证据。
- 三份正常打开的 newimage 抽检均在 Word 中读到新增图为 108×54 pt，清单却要求 144×72 pt；canvas 的 PDF 也确认实际为 108×54 pt。恢复的 newimage 样本记录同样保留具体尺寸，不把图已出现等同于全部期望符合。
- 三份 header 抽检的 PDF 已逐页核对。hf-variants 保留首页/偶数页页眉，第三页为 rsword 页眉；sections-three 保留前两节页眉，第三节替换，第二页横向；strict-basic-2 显示替换页眉。新页眉左对齐、无原有横线，未见页眉与正文重叠。
- Toggle 共 50 次实际 COM 读数。两层 strike/caps/smallCaps 在本桌面 Word 中关闭，dstrike 仍开启；25 测点与 fixture 的逐句网页版记录相比为 16 一致、7 不同、2 无已知网页版对照。详见 `TOGGLE.md`，不能概括为所有属性遵循同一规则。
- 新制修订样本确认了两位作者、原生 moveFrom/moveTo 与成对范围标记、字符/段落/编号格式修订、sectPrChange，以及接受第一处/拒绝第二处后仅剩第三处插入修订。
- 分节样本有五节，四处分节符在 Word 草稿视图依次为下一页、连续、偶数页、奇数页。PDF 第2页同时包含第2/3节，第3页为空白补页，第4/5页为第4/5节；第3节的 titlePg 和 first headerReference 均存在，但它的自定义首页页眉未在此次分页中显示。

## 未完成 / 存疑

- `rev-comment-threads` 确有5条批注、两条回复和解决标记；但 Word 将 reply.Replies.Add 生成的回复也指向根批注，UI同级显示，没有真正 root→reply→reply 的两层嵌套。本项按字面要求未全部满足，未修改XML伪造层级。
- B2-b 有 900 份未做页面抽检；只对 44 份提供 UI/PDF 结论。对象模型 mismatch/incomplete 不等同于已经逐一目视复核。
- 11 份 ChartEx 底稿的 newchart 样本能读取新增图表且对象数量符合，但原有 ChartEx 的系列值无法完整读取，保留 incomplete。
- table-styled 与 large-report 的 mergecells 基线第一行已经合并；编辑前后 document.xml 相同。此项为幂等操作/期望歧义，不能据此单独判引擎合并失败。
- Toggle 重复读数以 COM 为主，并结合实际页面、功能区和关键字体对话框；没有每句手动打开两次字体对话框。fixture 多数 CompatibilityMode=12；一份只有兼容模式截图、缺数值，不补造数字，也没有转换后复测。
- 未检测到 Microsoft 365；任务 E 按条件跳过。任务 D 的可选一笔墨迹转形状未执行，按可选项跳过，未提供合成替代件。

## 文件使用

`_resaved/` 内为 Word 实际另存件，不能当作本引擎原始输出；图表工作簿激活后的副本可能已保存旧值回刷的结果。恢复副本只代表恢复后状态，不会将原始 normal-open error 改成 ok。输入 DOCX 原件没有被覆盖。

最终 ZIP 排除 `_control/`、`~$` 锁文件、临时 `01-source-test.docx` 和运行进度文件。此目录保留可复查的读数与脚本；旧的中途 JSONL/hidden-pass 文件是过程记录，最终逐行结论以 `edited-results.json` 为准。

交付前的完整性审计在 `_scripts/final-independent-audit.json`：逐一核对输入 961 份 DOCX 与使用的 110 份基线未变，检查 9/944/25 行报告、44 份 UI 样本、53 份另存件，以及正式和试件 DOCX 的 ZIP CRC/XML 完整性。完整性通过不代表所有样本都符合编辑期望；上述异常、抽检范围和未满足项目仍然适用。ZIP 生成后另外执行包内文件与本地交付目录的逐文件比对，包审计记录留在 ZIP 外，避免改写已经打包的报告。
