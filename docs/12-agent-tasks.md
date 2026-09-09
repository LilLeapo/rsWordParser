# 12 · Agent 任务集、预算基线与交付形态

对应 `spec/20` 9.0 / TEST-10；测量基点 `e132df4`，2026-09-09。
M8′ 实施已完成，门 2 / 门 4 仍待 spec/18 7.4 裁定。9.0 只定义任务与测量，
不实现 Agent API，不替 9.1 起草或批准 `spec/22`。9.0 文档与测量已复核通过；**不是已通过的 Agent 验收结果**。预算和接口由 spec/22 的 9.1 关口稿承接，仍待规范评审。

## 1. 分母、输入与判定纪律

固定分母 **22 条 = R1–R11 + W1–W11**。所有输入均来自 `corpus/real/`，下表路径相对此目录。
输入只读；输出写临时目录，不改 docx、expected、save 或 model 快照。用户消息中的 22 条为起点；
当前工作树没有所述 `scratchpad/m9-taskset-draft.md`，不声称已读该草稿。

每项执行前检查下表的目标存在、数量与身份；条件不符直接失败，不静默跳过、换样本或当作 no-op 成功。
定位使用文字 + 流 + 文档顺序；文中 node 数字只记录当前测量，不能当跨会话稳定地址。
表格及图片按正文出现顺序计数，明确是否包含嵌套对象；章定义为一个一级标题起、下一个一级标题前的半开区间。

判定由独立 harness 读取输入与重新打开的输出，生成结构化事实作比较，不用另一个 LLM 打分。
Agent 可自由组织自然语言，但同时给出任务要求的事实记录（数组、数量、作者等）；先核事实再核正文不得矛盾。
**不比整份回答或输出 ZIP 的字节**。以下字节检查只针对不变式 2 所要求的未涉及内容。
重开后用 part URI、语义位置与 OOXML 自身身份字段对齐，不拿重新分配的 arena id 直接判语义不同。
只读任务不得调用 apply/save，结束时会话状态不变；全程记录工具名称、参数、响应大小与游标。

所有 W 项共有的硬条件（缺任何一项不可判通过）：

- 经 Agent 工具编译成可打印的 EditOp JSON，不允许 Agent 读 nodeXml/partBytes 或构造 XML；新会话 `xmlEscapeCount` 从 0 开始，成功提交后仍为 0。
- 保存后重新 `open → document()`，满足该项语义断言；不能只检查 apply 返回成功。
- 由输入中确定的允许改动集合约束 MutationResult，并独立比对其余块的原 XML 切片字节；未涉及 ZIP 条目的 CRC 与压缩字节相同。
  同一个 part 内新增关系或修改一个块，不授权重写该 part 的其他块；样式、关系、ContentTypes 等必要附带改动按任务点名。
- 桌面 Word 打开输出无修复提示，按 `tests/real_edits.rs` 与 `docs/09` 的既有流程记录 Word 版本、输出哈希、打开结果。
  这是独立宿主验收证据，不能拿 rsword 重解析成功代替；没有桌面结果记“待验”，不计通过。
- 不变式 2 中的允许集合不得由被测实现的“实际改了哪里”反向生成，否则错误改动会给自己授权。

读改任务的语义判定可自动化；桌面 Word 条件需上述宿主流程，9.0 未运行任何 Agent 或桌面验收。

## 2. 读类任务

| ID | 输入与自然语言指令 | 预检与可自动判定的结果 |
| --- | --- | --- |
| R1 | `misc/large-report.docx`：“这份文档讲什么？给出前三个一级标题涉及的主题。” | 输入有 26 个一级标题。回答含 `Windows Word`、`验证方法与往返结果`、`table-styled`，事实数组依次对应前三个标题；不得声称已解析图表数据或媒体。回答 UTF-16 长度 ≤ 2000，字节代理 token ≤ 1500；这只验最小主题覆盖，不宣称自动证明摘要的一切语义正确。 |
| R2 | 同 R1：“列出所有一级标题，保持文档顺序。” | 结构化 `(level,title)` 序列与独立模型标题扫描完全相等，长度精确 26，无重漏；也与未来 outline 的 level=1 序列相等。只对照 outline 自己不算独立 oracle。 |
| R3 | `fields/fields-toc.docx`：“目录里有几条条目？” | 唯一 TOC 字段，nested PAGEREF 为 3；非空结果条目精确 3，文字依次为一级章节、二级章节、三级章节。不能把最后一个空结束段算成第四条，也不以全局字段数当条目数。 |
| R4 | `revisions/revisions-comments.docx`：“谁在何时写了哪些批注？包括回复和已解决项。” | comments 精确 3 项，id 为 3/4/5，作者均 `燚坡 李`，日期均 `2026-09-07T01:38:00Z`；正文为 `第一条批注`、`第一条批注的回复`、`第二条未解决批注`。与 `(id,author,date,text,done,parent)` 投影逐字段相等，缺席字段保持缺席，不杜撰线程关系。 |
| R5 | `revisions2/rev-insert-delete.docx`：“把尚未接受的修订按作者分组计数。” | revisions 精确 3 项；作者甲 2（插入 1、删除 1），作者乙 1（插入 1）。按修订记录计数，不能按字符或重复内联 rev 元数据计数。 |
| R6 | `hf/hf-variants.docx`：“有哪些页眉页脚？首页、奇偶页是否不同？” | hfParts 精确 6 个；settings.evenAndOddHeaders=true，唯一 section 的 props.titlePg=true；逐项比较 header/footer × first/even/default 的引用及文本。计数引用与不同 part 分开，不把继承等同于缺失。 |
| R7 | `misc/large-report.docx`：“第一张正文表几行几列？第一行是什么？” | 顶层第一表 main[43]，3 行、3 个 gridCol；逐单元格输出第一行完整可见文字，与模型行内文本拼接相等。称“第一行”，不擅自认定它带语义表头标记；逻辑列数按 span 算，不直接用 cells.len。 |
| R8 | `chart/chart-column.docx`：“这份文档里的图表是什么类型？” | 预检唯一引用图表；按需 display=true，kind=`bar`、horizontal 缺席（按 false 解读），即竖向柱形。比较 `chartParts[part].display.kind` 的完整集合；回答含类型的中文描述及原始 kind。不能从文件名猜，也不能把默认 display=false 缺少 display 解释成无图表。 |
| R9 | `image/image-two-in-run.docx`：“正文有几张图片，各在哪一节？有几个媒体资源？” | 图片出现位置精确 2（drawing 节点当前为 29/60），均在第 1 节；唯一媒体资源 `word/media/image1.png`，media 数精确 1。沿绘图引用→关系→mediaId 联接；media 清单不是图片出现清单，也不自带节归属。 |
| R10 | `chart/chartex-sunburst.docx`：“这份文档有什么问题？用人话说明。” | warnings 非空，code 集合精确 `{CHART_NO_SERIES}`；事实保留 code、origin、part、range，描述“图表没有带缓存值的系列，不能据此报告完整数据”，不能推断整个文件打不开或没有图表。见 §6 的 AGENT-01 缺口。 |
| R11 | `misc/large-report.docx`：“只看第 3 章 table-styled，告诉我其中有几张表。” | 第三个一级标题到第四个之前为 main `[41,48)`，恰好 1 张表。允许先 outline；内容请求必须带该范围内 blockRange，禁止完整 document/text 预读再客户端过滤。合计响应代理 token ≤ 6000；只对本范围计数，读取日志中无范围外正文载荷。 |

R1/R2 的最大文档由实际 ZIP 字节数排序确定（266 份真实件，排除 `edited/`）；不能仅凭文件名认定最大。
R11 的大纲元数据不等于正文读取；范围边界应由标题锚点计算，表中下标为输入预检值，不给 Agent 偷渡完整模型。

## 3. 改类任务

| ID | 输入与自然语言指令 | 预检、语义断言与允许改动集合（另须满足 §1） |
| --- | --- | --- |
| W1 | `hf/hf-variants.docx`：“把正文里的所有『页』替换成『版』，页眉页脚一个字也别改。” | 正文目标非空，hf 也含“页”，确保排除条件真的受测；正文匹配数归零，替换后的各段文本等于输入文本的指定字面替换。hf part 原字节相同，其他正文块原切片不变。只授权命中正文段落，不授权字段、页眉页脚或声明 part。 |
| W2 | `text/text-basic.docx`：“在每个二级标题后插入一段『摘要：本节介绍正文与列表。』。” | 二级标题精确 1；输出在该标题紧后多 1 个普通段落，文字精确等于指定摘要；所有原块相对顺序、文字及原字节保持，不能附到文末或误匹配一级标题。 |
| W3 | `revisions2/rev-insert-delete.docx`：“只接受作者甲的修订，作者乙的保持待决。” | 输入作者甲 2、作者乙 1；输出作者甲为 0、作者乙仍为 1 且元数据/载荷保持；作者甲新增句保留、被删句消失。输出 accept-all 指纹等于输入 accept-all；输出 reject-remaining 指纹等于独立 oracle 对输入仅接受甲再拒绝乙的结果，复用 ModelFingerprint 双视图，不能只断言所有修订归零。只授权甲的两个所属段落。 |
| W4 | `table/table-styled.docx`：“删除第一张表的第 3 列，保留其他内容。” | 输入第一表为 3×3，无待决表修订；输出 grid 为 2，每行逻辑宽度 2，内容等于输入删掉第三逻辑列。其他列顺序不变，表外块与 part 原字节不变。原提案第二张表第三列在最大文档不可用（第二表只有 2 列），不能让坏位置拒绝冒充编辑成功。 |
| W5 | `text/text-basic.docx`：“给『Mixed English』加批注，作者 Agent，正文『请核对英文表述』。” | 唯一命中范围；新增批注精确 1，author/text 与范围映射相等，锚定字符不变。允许目标段落、comments part 及必需关系/ContentTypes；其余块字节不变，不扩大到整个段落的批注范围。 |
| W6 | `text/text-basic.docx`：“给『这是正文 Mixed English and 中文。』应用 AgentQuote 引用样式；如无此样式，创建段落样式，左缩进 720 twips。保留段落的其他直接格式。” | 输入唯一段落 pPr.rpr.size=22；不存在 AgentQuote 时以结构化 UpsertStyle 创建，再用 SetParaProps 的 style patch。输出 styleId=AgentQuote、样式 indent.start=720（原生键；对应左缩进），段落除 style 外的已建模属性逐字段不变，原有 pPr 未涉及子元素原字节不变。禁止 ReplaceParaProps/XML 逃生口；允许目标 pPr、styles 条目及必要声明关系。真实语料未发现 Quote/引用声明，不能引用不存在的 styleId 后自称成功。 |
| W7 | `fields2/fields-toc-stale.docx`：“更新目录标题文字，保留标题原文；不要猜页码。” | TOC 非空且旧缓存与当前 heading 不同；输出条目文字序列等于当前 level 1–3 标题（Changed heading one / Changed heading two / Original heading three），TOC/PAGEREF 结构仍可解析。只授权 TOC 结果、生成器必要的标题书签；其他标题文字和原块未涉及部分不变。不以 Word 布局产生的页码为内核断言。 |
| W8 | `text/text-basic.docx`：“为这一节新建默认页眉，文字『Agent 审阅稿』。” | 输入目标节无默认页眉引用；输出有 header part、有效关系和 ContentType，默认引用指向它，重解析文字相等。只授权 sectPr、关系、ContentTypes 与新 part；原正文各块不变。检验 SAVE-05 真建 part，不用已有 hf 样本掩盖创建路径。 |
| W9 | `image/image-two-in-run.docx`：“只把正文第二张图换成提供的图片，第一张保持不变。” | 替换载荷取 `image/image-svg.docx` 的 `word/media/image1.png`（PNG 回退媒体），新载荷 SHA-256 为 `b04c1699effcdb9be2dc67b8d7c1347bd1558d9ff2f2328240205a5ae27a24b7`，原载荷为 `91a27c9ae84524f4b5e9114eddfa017b21d7b6ad77aeecdc9fdc2e3ef184d6cb`，预检不同；先 addMedia 得新句柄再 ReplaceImageMedia 定位第二个 drawing。输出两处绘图仍在原顺序与位置，第二处解析到新字节，第一处仍到原字节。只授权第二绘图的引用、新媒体、其关系/ContentTypes；原媒体 part CRC/压缩字节不变，不能原位覆盖共享 image1.png。 |
| W10 | `misc/large-report.docx`：“把第 3 章 table-styled 整章移到文档第一个一级标题之前。” | 输入 `[41,48)` 七个顶层块，含表格；以 MoveBlock 序列移动整体，不重建其内容。输出为原序列移除这七块后在首个一级标题前插入，块内语义与原切片字节保持；其余块相对顺序与字节不变，sectionProps 留原位。不得仅移动标题。 |
| W11 | `text/text-basic.docx`：“打开修订追踪，把第一段改成『Agent 改写后的首段』，作者 Agent。” | 第一段为 `before 前文`，输入无待决修订；EditContext track_changes=true。输出接受视图该段为新文字，拒绝视图的 ModelFingerprint 等于输入，新增修订作者均为 Agent 且非空。只授权第一段；其余原块不变。不能直接 ReplaceInlines 不追踪后伪报成功。 |

对草案的调整不是缩减分母：W1 使用真实存在、且正文/hf 均有的“页”；W3 使用真实双作者；
W4 改为真实三列表；W6 明确缺失引用样式的结构化创建；W9 强化共享媒体的隔离验证。
W3/W11 不含本轮待裁定的追踪列几何交互；这不代替 spec/18 的裁定，也不放宽 M8′ 的门。

## 4. 9.0 历史预算实测与 9.1 建议

复现：在仓库运行 `tools/agent-baseline.sh`（Rust + Ruby，使用现有 Cargo.lock，临时构建位于 target，退出清理）。
脚本调用真实 `SessionTable::document(display=false)`，没有把 model 快照当成本次运行结果。

| 项目 | 实测 |
| --- | --- |
| 最大真实输入 | `misc/large-report.docx`，326406 B |
| 输入 SHA-256 | `70ced902f90a60505a8445144519c52d17ee7050e7763054e9ced98e68e45b93` |
| document JSON | 128519 UTF-8 B；124146 Unicode scalar；124146 UTF-16 code units |
| document SHA-256 | `86b4f13d518d3b4a8b6d857d7a35727b60e7f5670d13f879659d0ecc7bf48607` |
| document 范围 | 213 顶层块，truncated=false；一级标题 26 个 |
| token 估算 | `ceil(UTF8_bytes / 4)` = 32130，**不是 tokenizer 实测，也不是硬上界** |
| outline / text | 尚未实现，大小未测；标题清单只是输入事实，不是模拟接口结果 |

建议由 9.1 写入 AGENT-06 后生效，当前不冻结协议参数：

| 接口 | 建议缺省 limit（UTF-16 code units） | 补充预算/验收 |
| --- | --- | --- |
| outline | 4000 | 最大真实文档完整 26 标题应能在响应代理 token ≤ 4000 内返回；9.3 实测，不预报已达成 |
| text / context | 8000 | 约为当前整份 JSON 字符量的 6.4%；不足时分页，不允许客户端先收全文再截断 |
| find | 4000，至多 20 个命中 | 命中正文上下文计入 limit，续读必须无重复遗漏；数量限制不能替代字符限制 |
| document 按需取 | 8000，display=false | 响应外壳、诊断等也须计入总传输预算；先用 blockRange 缩小范围，块数不能保证字节上限 |

字符上限与 token 是两种度量，不能从 ASCII JSON 的比例推断中文 text() 的真实 token 数。
建议每次读取完整响应另限 UTF-8 24000 B（代理 token 6000），R11 所有读取累计同限；
outline 的最大件响应建议限 16000 B（代理 token 4000）。这些是预算建议，不是现存 API 能保证的行为。
Agent 工具描述及请求日志也耗 token，应在真实 Agent 验收时另计端到端总预算并写明 tokenizer 名称/版本。

**9.1 必须解决的边界**：spec/20 同时要求“任意 N 字符内”与“只在块/段边界截断”。
若单段或不可拆对象超过 limit，两条无法直接兼得；规范须定具名预算错误/最小可读单位或新的分片规则。
本稿不擅自选择，不以返回空内容且游标不推进来过门。续读拼接与一次性读取相等须在同一投影配置、同一会话版本下检查。

## 5. A / B / C 复核与证据边界

**确认 A：文件级 CLI + MCP**，负责人已定，rsword 现有全包 open/apply/save 路径可直接复用。
8.7 的隔离下游生命周期和真实 wasm 会话检查是 A 的底层证据；它们在 9.0 时不代表尚未实现的 CLI/MCP 端到端验收；9.6/9.7 的进程证据见 §9。

| 形态 | 当前证据与阻碍 | 本次结论 |
| --- | --- | --- |
| A 文件工具 | 现有原生协议输入/输出完整 DOCX，字节保真门与会话门已存在，不要求宿主重新加载后维持实时编辑状态 | 按已批准方向推进；显式文件读改存，不承诺保留宿主撤销栈 |
| B COM / VSTO | spec/20 记载全包保存后需要重载，撤销栈/光标保留有障碍；本工作树没有 Windows/Word 版本、重载步骤与前后撤销记录，9.0 未复现 | 不把设计初判冒充实测。补证步骤：Word 建多步撤销→文件级修改→按拟集成方式重载→记录 Undo 可用序列及光标。宿主版本和具体重载路径必须点名 |
| C Office.js | spec/20 记载 insertOoxml 片段写入要求；当前 save 返回全包，不提供面向宿主的片段生成契约。没有该加载项的执行记录 | 新输出路径属于另立范围；补证应记录宿主/API 版本、具体插入范围、正文/hf/section 测例与失败结果，不能把所有 Office.js 能力概括成“不支持整包” |

B/C 的宿主实测记录待提供，未运行、未捏造失败数据；这不重开已裁定的 A 选择。

## 6. 9.0 留给 9.1 的历史输入

- R10 的 code → 人类描述映射归 AGENT-01：当前 warnings **已有 message**（本样本为“图表 part 里没有带缓存值的系列”），
  并非只有机器码；缺的是稳定的用户说明/能力影响与未知 code 回退契约。保留原 code 与定位，未知项回退原 message 并声明未知，不能静默丢警告。
- R8 要显式按需 display；R9 要区分媒体资源和绘图出现位置，补引用与所属流/节的定位能力。
- W6/W9 需要结构化样式创建、媒体上传与精准出现位置编辑；AGENT-07 的编译规则不能只覆盖文字替换那几个示例。
- R11 要能审计读取范围和累计预算；limit 的单位、不可拆长块行为、游标与会话版本失效规则必须在 AGENT-06 裁定。
- 任务结果、Word 打开证据、真实 Agent 的三条改类会话记录均留待后续阶段，本表不提前填通过率。

## 7. 9.0 验证记录

任务表精确 22 行，目标前置条件按真实 model 快照核查；预算脚本真实调用原生协议，重复运行结果相同，
脚本 bash 语法检查通过。这里只核实任务可落在样本上，不计 Agent 任务通过数。

本轮 workspace 默认 debug/release 各 **792 通过 / 0 失败 / 13 ignored**，compat 各 **911 / 0 / 13**；
fmt 干净，两套 clippy/audit 零告警。八道 `diff-parse --features compat-ts` 检查仍为 **242 + 547 已知 / 0 未知**。
没有改引擎、语料或冻结规范；B/C 宿主实测缺证据，9.0 文档复核已通过，宿主补证仍未完成。

## 8. 9.5 写侧支持状态（分母仍为 11）

以下是编译器和 Rust 自动化证据，不是桌面 Word 或真实 Agent 端到端通过率。**11 项均缺本轮桌面 Word 无修复提示证据**，未缩分母。

| 任务 | 已支持与自动化证据 | 尚缺 |
| --- | --- | --- |
| W1 | hf-variants：正文目标确实非空，替换后归零；带同词的 hf 原字节不变 | 真实 Agent 调用链及桌面 Word |
| W2 | text-basic：精确 1 个二级标题，摘要紧随其后，全部原块字节保留 | 真实 Agent 调用链及桌面 Word |
| W3 | rev-insert-delete：精确作者字符串为“作者甲/作者乙”，甲归零、乙保留；两视图与独立原生作者筛选 oracle 相等 | 复杂修订宿主/跨作者配对不猜，超出可表达授权时具名拒绝；桌面 Word |
| W4 | table-styled：第三逻辑列删除，3 行与 2 列 grid；辅助 part 提前拒绝 | 更完整逐列内容/其他 part 保真任务 oracle及桌面 Word |
| W5 | text-basic：新增批注正文；重解析后的范围截取恰为 Mixed English | 真实 Agent 调用链及桌面 Word |
| W6 | text-basic：创建 paragraph 样式、正文仅 style patch；保留 rPr size=22；同声明不重复 upsert、冲突报错 | 全部未建模 pPr 子元素的独立局部字节 oracle及桌面 Word；原生属性键是 indent.start，不是 TS 的 left |
| W7 | 操作名/schema/拒绝测试保留在同表清单 | **尚不支持执行**：原 PAGEREF 缓存页码与目标书签来源核对、完整更新摘要；AGENT_UNSUPPORTED_RANGE，不猜页码 |
| W8 | text-basic：创建默认页眉；新增部件进入报告；后一步失败时原包/版本不变，重试与独立新会话结果相同 | 全部旧正文块字节 oracle及桌面 Word |
| W9 | image-two-in-run：只改第二个出现位置，第一 drawing 原片段及原媒体 CRC/压缩字节保留；外置审计还原与篡改拒绝 | 更多媒体容器验证器；目前 MIME 检查是容器签名，不是完整图像解码；桌面 Word |
| W10 | large-report main[41,48) 七块移动到首块前，七块原片段、顺序均保留；含分节/不连续/自内目的地拒绝 | 高层 chapter 选择语法仍需调用方用 outline 转成完整 ObjectRef 列表；桌面 Word |
| W11 | text-basic：追踪改写首段，accept 含新文、reject 指纹等于修改前，作者 Agent 且修订非空 | 真实 Agent 调用链及桌面 Word |

9.5 修正了共享 ModelFingerprint 的文字/指令取值：原 helper 把元素当文本节点，T/F 可全空而不报错。
新增 FIRST/OTHER 不同文档必须不同指纹的常驻反例；旧的“两个空值相等”不再能充当文字保真证明。
批次内符号引用尚缺；不可通过猜测新建 nodeId 绕过。具体接口、报告容量与审计外置见 [17-agent-edit.md](17-agent-edit.md)。

## 9. 9.8 读侧支持状态与完整分母

当前分母仍为 **R1–R11 + W1–W11，共 22 项**。§2/§3 是任务验收要求，§8 与本节是实现证据，
两者不能互换。§4/§6/§7 保留 9.0 的历史测量与关口输入；当前预算见 §10。
9.6/9.7 已补 CLI 与 MCP 的真实进程驱动，**没有**因此补齐 §8 中真实 Agent、桌面 Word 或独立任务 oracle 的缺口。
W7 仍不支持执行，11 项 W 均仍缺本轮桌面 Word 无修复提示证据。

| 任务 | 当前支持与证据 | 尚缺的任务级验收 |
| --- | --- | --- |
| R1 | model/text/outline 可有界读取；最大件标题序列与预算有自动化断言 | 真实 Agent 摘要的三个主题覆盖和实际上下文消耗；不以标题枚举证明摘要正确 |
| R2 | outline 标题级别/文字/顺序对独立模型 oracle，全 real 跑；最大件 26 个标题 | 真实 Agent 返回的 level=1 序列与任务预期核对 |
| R3 | model 的字段模型与只读 field detail 可用于核对 TOC | 具名样本的 Agent 条目计数任务 oracle，不能直接拿占位符个数当条目数 |
| R4 | model comments[] 保留作者、日期、正文；text 有独立批注流 | 真实输出三字段与具名样本逐项核对（包括缺席状态） |
| R5 | model revisions[] 可读取，编译器有按作者筛选的写侧证据 | 读任务按作者聚合计数的完整 Agent 回答 oracle |
| R6 | model 可按 fields 取 hfParts/settings/sections；各流有身份 | 同时核对奇偶页、首页标志与实际引用关系的 Agent 回答 |
| R7 | context/detail 的表格信息与真实模型逐字段比较 | 按任务表的精确表序号、表头语义核对真实回答 |
| R8 | 按需 display 的 chart detail 对真实模型核对类型 | 指定图表的 Agent 可读类型回答，不能把所有图表占位符当同一种图 |
| R9 | 媒体资源与 drawing 出现位置分开；detail 有真实样本对照 | 完整出现位置到所属节的任务级回答，资源个数不能代替图片出现次数 |
| R10 | 版本化 DiagnosticView 表、原 code/message/定位、能力影响及未知码回退均有断言 | 真实 Agent 回答的 code 集合与可读说明核对；不宣称诊断能证明所有文档问题 |
| R11 | blockRange 复用原生选择；续读有独立全文 oracle，单元格/授权范围有越界反例 | 真实会话读取日志只含 main[41,48) 与累计预算证明；禁止先全量读取再过滤 |

以上仅声明接口与 Rust/进程测试覆盖，不报告 22 项通过率。自动化入口包括
`crates/rsword/tests/agent_text.rs`、`tools/agent-query/tests/query.rs` / `paging.rs` / `edit.rs`，
以及 `crates/rsword-cli/tests` / `crates/rsword-mcp/tests`。实际可执行命令见 README、docs/18、docs/19。

## 10. 9.8 当前预算实测

见 docs/05 的最大三份真实件基准表与复现命令。测的是共享 Agent 读接口的**缺省首屏**，
不是整份文档的总耗时或总体积；完整 JSON 信封（锚点、诊断、游标、usage 等）全部计入。
两种 MCP 形态的实际字节各列一栏，分页仍按两者较大成本。估算 token 统一为 `ceil(实际字节/4)`，
这是预算代理值，不是假称实测某个模型的 tokenizer。
原生无参 document(display=false) 继续是整份模型，不能拿它的体积冒充 Agent 首屏体积。

最大件 `misc/large-report.docx` 的共享接口缺省首屏：

| 接口 | limit / maxBytes | content UTF-16 | 共享 JSON B | MCP text / structured B | truncated |
| --- | ---: | ---: | ---: | ---: | --- |
| text | 8000 / 24000 | 1140 | 19559 | 22914 / 19658 | true |
| outline | 4000 / 16000 | 3870 | 4408 | 5195 / 4507 | true |
| find（字面 r，scope=all） | 4000 / 24000 | 3561 | 4874 | 5677 / 4973 | true |

这里的共享 JSON 使用会话身份，不是 CLI 文件游标信封。跨传输只比较业务区间与续读终态；
两 MCP 形态在同会话同预算下共用分页。outline 记录字符包括结构 JSON，不能当标题净文字。
文本首屏的锚点等元数据占用了大量字节预算；未调高默认值，也不把截断页当完整文档。

9.8 重跑 `tools/agent-baseline.sh`：输入 SHA-256、原生 JSON SHA-256 与 §4 相同，
仍为 **128519 B / 124146 UTF-16 / 32130 代理 token、213 块、truncated=false**。
脚本现明确标注只测原生模型，不再把未在此脚本测量的 Agent 接口误报成“未实现”。
独立 outline 模型 oracle 复测为 **26 标题 / 5052 UTF-16 / 5415 B / 1354 代理 token，默认分页 2 页**；
该整份记录信封使用测试快照标识，与上述真实会话首屏的信封不是同一口径。

## 11. 真实 Agent 门 5 记录（由评审者填写）

**尚未提供真实会话记录，不预填通过率。** 以下是待提供的证据字段，不是已执行的会话：

- 客户端、模型、MCP result shape 与客户端实际消费情况；text/structured 缺省选择结论。
- 会话日期、输入文件 SHA-256、所选三项 W 编号；完整 open → 读取 → preview/edit → summary → save/close 日志。
- 实际预算与游标续读、版本、正向操作审计及附件绑定、BIND_XML_ESCAPE 计数。
- 保存后 document() 语义断言、未涉及块字节证据、桌面 Word 版本与无修复提示证据。
- 每项结果及未通过原因；未验证项仍留在 22 项总分母，不能记作跳过后通过。

stdio JSON-RPC 与二进制可驱动性由进程测试验证；这些传输测试不填写本节的真实 Agent 结果。
