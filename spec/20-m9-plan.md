# SPEC 20 · M9′ 任务分解（Agent 接口层与文件级工具）

> **本文件在 2026-09-08 被整体替换。** 原 SPEC 20 是「M9：原生协议 + 渲染器接管排版启发式 + 删除 `compat_ts` 与 TS 引擎」，
> 其中**属于 rsword 的那半边**（9.1–9.4、9.8：协议规范、模型 JSON、`EditOp` JSON、会话与媒体句柄、`fuzz_bind`）
> 已前移为 M8′（`spec/19`）；**属于 genoffice 的那半边**（9.5 十二条排版启发式搬进渲染器、9.6 编辑器迁移、
> 9.7 删 TS 引擎与 `compat_ts`）随范围改定**整体撤销**——本项目没有自己的渲染器，也不再迁移任何编辑器。
> 原文的分析（TS 助手 → `EditOp` 映射表、排版启发式清单、编辑器基线）在 git 历史里可查，仍是有价值的参考。
>
> 新的 M9′ 是**全新内容**：让 Agent（LLM）在不看 XML 的前提下读懂一份 docx 并正确修改它，交付一个文件级工具。

对应 `docs/03` v3.3 §12 的 M9′ 行。格式同 `spec/12`–`spec/19`。**前置：M8′ 完成**（原生协议、公共 API、自快照网）。
分支 `m9-agent`。本计划在 2026-09-08 写成，M8′ 未开工；下文凡是标「9.0 定」的数字都要在开工时实测填入。

## 目标

项目负责人 2026-09-08 定的形态：**Word / WPS 的外挂应用，对 docx 做阅读与修改，后续接入 Agent 来读改内容；
不考虑渲染，专注 parser。** 外挂形态取 **A：文件级工具**（CLI / MCP server / 桌面小程序），不进宿主进程——
理由与另两种形态（Word COM / VSTO 加载项、Office.js / WPS JSAPI 任务窗格）的比较见「形态取舍」。

拆成两件事：

1. **Agent 接口层**（`spec/22-agent.md`，前缀 `AGENT`）：M8′ 的原生协议是给**程序**用的——`nodeId` + UTF-16 偏移 +
   完整模型 JSON。Agent 用不了：它读的是文字，引用的是「第 3 段」「写着 X 的那句」，而且**上下文是稀缺资源**。
   这一层负责把两者接起来：
   - **读**：文本投影（带地址的可读文本）、大纲、定位、上下文窗口，全部带预算与截断游标。
   - **写**：文本锚定的编辑（「把第 N 块里的 X 换成 Y」）编译成 `EditOp`；保存前给出**文本层面的预览 diff**；
     保存后给出变更摘要让 Agent 自检。
2. **文件级工具**：`rsword` CLI 与 `rsword-mcp`（MCP server）。用户选一个 `.docx`，Agent 通过工具读、改、存；
   字节保真（不变式 1 / 2）保证「Agent 只改了它说要改的地方」，这是本项目相对「用 python-docx 重写一遍文档」的核心差别。

## M9′ 门（同步写进 `spec/11` TEST-10 M9′ 行）

1. **Agent 任务集**：9.0 定义的任务集（读类 + 改类，分母见 9.0）全部通过。改类任务的输出 docx **必须**满足：
   桌面 Word 打开无修复提示（`corpus/real` 的 `real_edits` 流程）；未涉及的块字节原样（不变式 2）；
   `document()` 重解析后语义断言成立。
2. **文本投影往返**：全语料（799 + 266 + 38）`text()` 的**每个**字符位置都能映射回 `(nodeId, InlinePos)`，
   且反向映射回同一位置；投影丢弃的每一类内容（图片、图表、公式、墨迹、批注、字段结果、隐藏文字…）都有占位符与计数，
   `text()` 的元数据里报「本次投影省略了什么、各多少个」——**不允许静默丢东西**。
3. **预算**：最大真实文档的 `outline()` 输出 ≤ 9.0 定的 token 上限；任何一次读取调用都能限制在 N 字符内并给出续读游标；
   续读拼接的结果与一次性读取逐字符相同。
4. **写路径成熟度**：改类任务里逃生口 `BIND_XML_ESCAPE` 计数为 **0**（`spec/19` 决策 10 留下的指标在这里收口）；
   每条文本锚定编辑都能给出它编译成的 `EditOp` 序列（可打印、可审计）。
5. **交付**：CLI 的每个子命令有端到端测试；MCP server 用一次真实 Agent 会话验收（记录进 `docs/12`），
   工具在 macOS 与 Linux CI 上构建通过。
6. **既有门不退**：rsword 全部门（往返、编辑保真、`TEST-07`、`fuzz_bind`、四个 fuzz、hostile、
   `--features compat-ts` 下的九道差分门）继续绿。

## 任务

| # | 任务 | 规范 | DoD |
| --- | --- | --- | --- |
| 9.0 | **场景、验收集与预算基线**（`docs/12-agent-tasks.md`）：① **Agent 任务集**——读类（「这份文档讲什么」「有哪些一级标题」「第 3 章有几张表」「谁在什么时候留了批注」「有哪些未接受的修订」）与改类（「把全文的『甲方』换成『乙方』但不动页眉」「给每个二级标题后面加一段摘要」「接受所有张三的修订」「把第 2 张表的第 3 列删掉」「给这段话加批注」「把这段设成引用样式」），每个任务给：输入文档、自然语言指令、期望结果的可断言形式（不是逐字节，是语义断言 + 不变式 2）。**分母就是这张表**，M9′ 门 1 按它判；② 预算基线——最大真实文档的 `document()` / `outline()` / `text()` 各多大（字符与估算 token），定门 3 的上限；③ 形态取舍复核：A / B / C 三种外挂形态的实测阻碍（B 的重新加载丢撤销栈、C 的 `insertOoxml` 片段要求）写进 `docs/12`，确认 A | TEST-10 | 任务集 ≥ 20 条（读改各半），每条可自动判定；预算上限写进 `spec/22`；`docs/12` 评审通过 |
| 9.1 | **规范 `spec/22-agent.md`**（前缀 `AGENT`，`spec/00` §0.2 加一行）：`AGENT-01` 文本投影的语法与保真声明（哪些结构映射成什么、哪些是占位符、占位符的语法与可逆性）；`AGENT-02` 双向锚点（文本偏移 ↔ `(nodeId, InlinePos)`，UTF-16 单位，与 `EDIT-02` 同体系）；`AGENT-03` 大纲；`AGENT-04` 定位查询（字面 / 正则 / 大小写与全半角规则）；`AGENT-05` 上下文窗口；`AGENT-06` 预算、截断与游标（每个读接口都必须支持）；`AGENT-07` 文本锚定编辑与它到 `EditOp` 的编译规则（含歧义处理：匹配到多处怎么办）；`AGENT-08` 预览 diff；`AGENT-09` 变更摘要；`AGENT-10` 工具接口（CLI 子命令与 MCP 工具的名字、参数、错误）。**关口**：9.2–9.7 的 API 面在它之后才定 | BIND-02, BIND-03, BIND-10, EDIT-02 | 规范评审通过；`spec/00` §0.2 表加 `AGENT` 行；每条带验收小节 |
| 9.2 | **文本投影与双向锚点**（`agent/text.rs`）：把 `Document` 投影成一份可读文本——标题按级别加 `#`，列表按 `numbering` 的**实际编号**（走 `resolve::list_markers`，不是猜）加 `-` / `1.`，表格投成管道表，页眉页脚 / 脚注尾注 / 批注 / 文本框各自成段并标明所属流（`FlowId`），字段结果、图片、图表、公式、墨迹、OLE 投成占位符 `[image #12]` / `[field PAGE]` / `[chart "销售额" #3]`。**同时产出锚点表**：文本偏移 → `(part, nodeId, InlinePos)` 的分段映射，双向可查。丢弃项逐类计数并随投影一起返回 | AGENT-01, AGENT-02, MOD-01–MOD-09, RES-01 | 门 2 全绿；投影是**纯函数**（同一 `Document` 两次投影逐字节相同）；`fixture` 覆盖每一类占位符 |
| 9.3 | **大纲、定位与上下文**（`agent/nav.rs`）：`outline()`——标题树（级别、文字、`nodeId`、该节的块数与字符数），无标题的文档退化为「按块分组的首句摘要」；`find(pattern, opts)`——在文本投影上搜，返回 `{nodeId, textOffset, len, 上下文片段}`，支持字面与正则、大小写 / 全半角 / 空白归一选项；`context(anchor, before, after)`——按字符或按块取窗口。三者共用 9.2 的锚点表 | AGENT-03, AGENT-04, AGENT-05 | 全语料 `find` 结果与在 `text()` 输出上直接搜的结果一致；`outline` 对 `corpus/real` 的 266 份各产出可读结果（无 panic、无空树误判） |
| 9.4 | **预算、截断与游标**（`agent/budget.rs`）：每个读接口加 `limit`（字符）与 `cursor`；返回 `{ content, truncated, next_cursor, omitted }`。截断只在**块边界或段落边界**发生，不切碎一句话；游标是不透明字符串，编码 `(sessionId, flow, blockIndex, offset)`。`document()` 的 `BIND-10` 按需取在这里对齐同一套游标语义 | AGENT-06, BIND-10 | 门 3 全绿；「续读拼接 == 一次性读取」的属性测试进 `TEST-07` 风格的随机门 |
| 9.5 | **文本锚定编辑、预览与变更摘要**（`agent/edit.rs`）：① 文本锚定操作——`ReplaceText { anchor, find, replace, occurrence }`、`InsertParagraphAfter { anchor, text, style }`、`SetBlockStyle`、`DeleteBlock`、`AddComment { anchor, find, text }` 等，**编译**成 `EditOp` 序列；歧义（`find` 命中多处而 `occurrence` 未指定）**必须**报错而不是猜；② `preview(ops)`——在克隆会话上跑一遍，返回**文本层面**的 diff（改前 / 改后的受影响块），不落盘；③ `summary(result)`——`MutationResult` 的可读投影：改了哪些块、各自改前改后、产生了哪些修订 / 批注、触发了哪些诊断。三者都要能打印它编译出的 `EditOp` JSON（门 4 的可审计性） | AGENT-07, AGENT-08, AGENT-09, EDIT-01–EDIT-06 | 门 1 的改类任务全部只用文本锚定操作完成（逃生口计数 0）；`preview` 与真正 `apply` 的结果一致（同一克隆语义） |
| 9.6 | **CLI**（`crates/rsword-cli`，二进制名 `rsword`）：`outline` / `text` / `find` / `context` / `model`（原生 JSON）/ `ops`（应用一份 `EditOp` 或文本锚定操作的 JSON）/ `preview` / `diff`（两份 docx 的文本层 diff）/ `media`（列出与导出）/ `check`（跑不变式与诊断）。全部支持 `--limit` / `--cursor` / `--json`。会话在 CLI 里是**单次调用内**的（每次打开文件），MCP 才需要跨调用会话 | AGENT-10 | 每个子命令一条端到端测试；`--json` 输出过 schema；`rsword check` 对 38 份 hostile 文档不 panic |
| 9.7 | **MCP server**（`crates/rsword-mcp`）：把 9.2–9.5 暴露成 MCP 工具（`open` / `outline` / `text` / `find` / `context` / `edit` / `preview` / `save` / `close`），跨工具调用保持会话；工具描述里写清预算语义与「先 `outline` 再下钻」的用法；错误按 `BIND-07` 的 `code` + `message` 返回。**运行形态见「待决」1**（原生 Rust 的 MCP 实现 vs 经 wasm 绑定的 node 实现） | AGENT-10, BIND-01, BIND-07 | 门 5 全绿：一次真实 Agent 会话完成 9.0 任务集里的三条改类任务并记录进 `docs/12`；会话泄漏检查（`close` 后内存回落） |
| 9.8 | **门、性能与文档**（`benches/agent.rs`、`docs/`）：`text` / `outline` / `find` 在最大三份真实文档上的耗时与输出体积；`docs/05` 数字；`docs/03` v3.3 §12 勾掉 M9′；`docs/12` 收尾（任务集通过率、真实会话记录）；`README.md` 加「给 Agent 用」一节与 MCP 安装说明 | TEST-10 | 门 1–6 全绿；`docs/05` 与 `docs/12` 更新 |

建议顺序：9.0 → 9.1（**关口**）→ 9.2 → 9.3 / 9.4 并行 → 9.5 → 9.6 → 9.7 → 9.8。

## 形态取舍（9.0 复核，此处记初判）

| 形态 | 怎么拿到文档 | 怎么写回 | 判断 |
| --- | --- | --- | --- |
| **A. 文件级工具**（CLI / MCP / 桌面小程序） | 直接读 `.docx` 字节 | 直接写 `.docx` 字节，字节保真 | **选它**。rsword 的能力完全对得上，不需要任何新的输出形态；Agent 经 MCP 驱动；B / C 都能复用同一套 Agent 接口层 |
| B. Word COM / VSTO 加载项（Windows 桌面） | 宿主给完整文件；Rust 编 `cdylib` 直接调 | 改完要让宿主**重新加载**文档 | 用户的撤销栈与光标会丢；且只覆盖 Windows + Word。留作后续 |
| C. Office.js / WPS JSAPI 任务窗格 | `getFileAsync` 取整份 docx 字节，rsword 编 wasm 在窗格里跑 | **整份替换受限**（页眉页脚、节设置支持不完整），实际要走 `insertOoxml` 插片段 | 要求 rsword 产 **OOXML 片段**而不只是整包保存——7.7 / 7.8 的块生成器有底子，但这是一条新的输出路径，单独立项 |

## 分层决策（实现前定死）

1. **Agent 层是 M8′ 协议之上的一层，不是旁路**：文本投影读 `Document`，文本锚定编辑**编译**成 `EditOp` 再走
   `EditSession::apply`。不允许 Agent 层自己碰 DOM 或 Span（`docs/03` §0.7 规范状态的规矩照旧）。
2. **投影可以有损，但不能静默有损**：`text()` 每次都报「省略了什么、各多少个」。Agent 据此知道要不要下钻。
3. **歧义报错，不猜**：`find` 命中多处而调用方没指定第几处 → `AGENT_AMBIGUOUS` 错误并列出候选。
   让 Agent 多问一轮，比改错地方便宜得多。
4. **预算是一等参数**，不是可选项：每个读接口都有 `limit` / `cursor`，缺省值保守（9.0 定）。
5. **写之前先预览**：`preview` 是无副作用的克隆运行。工具描述里引导 Agent「preview → 确认 → save」。
6. **字节保真是卖点，要能证明**：`save` 之后 `check` 能报「本次改动触及了哪些 part、哪些块」，
   未触及的部分字节相同（不变式 2）——这是相对「重写整份文档」的类库的核心差别，要在 `README` 和工具描述里说清楚。
7. **id 不跨会话**（沿用 `spec/19` 决策 6）：MCP 会话内 `nodeId` 稳定；`close` 之后失效。
   Agent 若要跨会话引用，用文本锚点（`find` 的字符串）而不是 id。
8. **不做的**：分页、渲染、`.doc` / RTF / ODT、宿主进程内运行（B / C）。

## 实现约定

同 `spec/19`：**同一形状重复三次以上就上 `macro_rules!`**。M9′ 的同形处：

- `agent_tool!`：CLI 子命令与 MCP 工具同一张表——「名字 + 参数结构 + 调用 + 输出投影」，展开 clap 子命令、
  MCP 工具描述与一条端到端测试，避免两边漂。
- `text_projection!`：块类型 → 投影规则（前缀、占位符语法、是否递归子流）一张表，展开投影函数、
  「每类都有 fixture」的测试与 `AGENT-01` 的清单。
- 沿用 `named_enum!`、`set_some!` / `set_if!`、`fixture_tests!`。
- **不上宏**：`find` 的匹配规则；游标编解码；MCP 的传输层。

其余照旧：树遍历写成**迭代**；一个任务一个提交 `m9.<n>: 英文摘要 (SPEC-ID…)`；提交前同步 `docs/04` §18 勾选、
§8 偏差表、`docs/05` 数字。

## 不在 M9′

- **宿主进程内的加载项**（形态 B / C）与 **OOXML 片段生成** —— 另立里程碑。
- **实时 `apply`** 与协同编辑。
- **分页 / 排版 / 渲染**（`docs/03` §1.2 永久不做）。
- **删除 `compat_ts`**（`spec/19` 决策 2 已定：保留为测试专用件）。
- **Agent 本身**：提示词工程、模型选择、任务规划不在本项目内。M9′ 只交付**工具面**，
  9.0 的任务集用现成的 Agent 跑通即算验收。
- `.doc` / RTF / ODT；文档比较；OCR / 版式识别。

## 风险提示

1. **文本投影的设计是本里程碑的胜负手**，而且没有先例可抄：投得太细 Agent 淹在噪音里，投得太粗改不准。
   缓解：9.0 的任务集先写，9.2 的投影按任务集反推；每加一类占位符就补一条任务。
2. **锚点在编辑后失效**：Agent 通常是「读一段 → 想 → 改」，中间文档可能已被前一条操作改动。
   缓解：文本锚定操作带 `find` 字符串而不是纯偏移，编译时重新定位；对不上就报错。
3. **表格与嵌套流的投影**：管道表对合并单元格、嵌套表、单元格内多段落都不忠实。
   缓解：占位符 + 下钻（`context(cellNodeId)`），不追求管道表能表达一切。
4. **正则搜索的性能与安全**：`corpus/real` 有百万字符的文档，恶意正则会指数爆炸。
   缓解：用 `regex` crate（线性时间保证），加长度与超时上限。
5. **MCP 会话生命周期**：Agent 崩溃 / 断连时会话泄漏。缓解：空闲超时 + `close` 幂等 + 会话数上限。
6. **「不看 XML」是硬要求还是软目标**：`nodeXml` 是调试出口（`BIND-09`）。若 Agent 在任务集里频繁需要它，
   说明投影缺东西——**当作投影的 bug 处理**，不要顺手让 Agent 读 XML。
7. **依赖增长**：CLI 要 `clap`，MCP 要传输层，`find` 要 `regex`。核心 crate `rsword` 的依赖**不许**因此增加——
   全部放在 `rsword-cli` / `rsword-mcp` 里（`spec/19` 8.5 的 feature 划分要能撑住）。

## 待决（需要项目负责人拍板）

| # | 事项 | 建议 |
| --- | --- | --- |
| 1 | MCP server 的实现形态：原生 Rust（如 `rmcp`）vs 经 wasm / napi 绑定的 node 实现 | 建议：原生 Rust——crate 优先的定位一致，且不引入 node 运行时；`rsword-mcp` 单独 crate，依赖不污染核心 |
| 2 | 任务集用哪个 Agent 验收（Claude Code / 自建 harness / 两者） | 建议：Claude Code 经 MCP 跑，会话记录进 `docs/12`；另留一个确定性 harness 供 CI |
| 3 | 文本投影的语法：自造轻标记 vs 尽量贴 Markdown vs 结构化 JSON + 文本 | 建议：贴 Markdown（Agent 最熟），占位符用 `[类型 #id]` 的显式语法；另给 `--json` 的结构化形态 |
| 4 | 任务集里的真实文档来源与许可 | 项目负责人从 `corpus/real`（`docs/07`–`docs/09` 那批）挑，或另给自有样本 |
| 5 | 桌面小程序（形态 A 的 GUI 版）做不做、什么技术栈 | 建议：M9′ 不做，先 CLI + MCP 验证价值 |
| 6 | crate / 二进制的发布：crates.io、GitHub Release、还是仅源码 | 建议：M9′ 收尾时定；`spec/19` 待决 2 一并 |
