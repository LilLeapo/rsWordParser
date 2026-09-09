# 05 · 现状快照（2026-09-09）

这份文档回答"现在能做什么、不能做什么、数字是多少"。任务清单在 `docs/04-dev-plan.md`，规范在 `spec/`。
数字都可以用文末的命令复现；改动代码后请一并更新这里。

## 结论

**M9′ 9.7 MCP server 实施完成**：`crates/rsword-mcp` 提供原生 stdio 服务，复用同一张工具表、
Agent 会话、预算/游标与编辑审计。原生 Rust 形态**按建议执行、待追认**；缺省 text、structured 可选，
缺省仍待门 5 的客户端实测确认。连接配置与边界见 [19-mcp.md](19-mcp.md)。

最终串行实跑默认 debug/release 各 **952 / 0 / 13**、compat 各 **1071 / 0 / 13**（通过/失败/ignored），
比 9.6 各新增 **28** 条：MCP 集成测试 **24**、独立堆检查 **1**、共享预算/错传输游标测试 **3**。
fmt 干净，两套 clippy/audit/cargo doc **零告警**；八道差分 **242 + 547 已知 / 0 未知**。
save_blocks **204/208 等价、4 份既有具名差异、0 跳过**；未设置 JS 产物目录，未执行可选 JS 字节对照。
体积门 **251** 份 **4,334,070 B → 1,408,718 B（−67.5%）**；**1099** 份模型快照无改动，无流段落仍为 **0**。
核心运行期依赖实查仍为 **5** 个：zip/memchr/thiserror/serde_json/serde。

**16** 个共享工具均有调用与 schema 证据；除 open/close/version 外 **13** 个缺会话测试宏展开并精确对表。
hostile 精确 **38** 份：**34** 份打开并 check，**4** 份点名拒绝；默认 **32** 会话上限、空闲回收、幂等 close、
失败编辑/回执预算与 stdout 断管回滚均验证。MCP diff 的游标也归会话，不再走文件游标入口。
两形态共同分页取较大成本，usage 报实际形态成本；跨形态续读及 CLI/MCP **2 页**续读终态相等，
游标双向错投均具名拒绝。跨传输不要求同预算同页，见 spec/22 v1.4 的三个维度。

独立堆测试保持服务实例存活，最大真实件连同游标/报告打开后增量 **5,694,505–5,694,633 B**，
close 或过期后 **1,424–1,552 B**，连续 **5** 轮无累积增长；测的是在用堆，非 RSS。
两次实际破坏验证：共同成本 max 改 min 后预算断言红；取消会话释放后堆检查报 **5,692,859 B** 未释放。
均逐字节恢复后跑最终全门。整数参数测试先报红再修，非 u64 线型不再回退默认预算或触发 unwrap。
开发期一次全测混用了修改前的测试程序与局部重建的服务器，触发 schema 对表失败；已冻结源码串行重跑，
不把该构建干扰归因于引擎。失败日志保留在 target/m97-first 与 target/m97-probes，最终日志在 target/m97-final。

官方 Python MCP SDK **1.26.0** 在两形态完成握手、列工具、打开/大纲/文本/关闭，协商 **2025-11-25**；
这只是传输冒烟证据，**没有真实 Agent 或桌面 Word 任务通过率**。本机 macOS 已执行；Linux 测试已配置 CI，未在本机执行。
最大真实件大纲复测仍为 **5052 UTF-16 / 5415 B / 1354 代理 token、2 页**，默认限额未提高。
docs/12 的任务分母及缺口不变，门 5 留给评审者发起真实会话；M8′ 待裁定门不判。

以下保留 9.6 及之前各轮交付时的记录。

**M9′ 9.6 文件级 CLI**：`crates/rsword-cli` 提供 rsword，复用共享 Agent 工具表、预算/游标、
会话版本与编辑审计；不增加核心 rsword 的运行期依赖。实际命令包含 outline/text/find/context/model、
ops/preview/summary/diff/media/check/version，MCP server 留到 9.7。接口与完整边界见 [18-cli.md](18-cli.md)。

本轮最终实跑默认 debug/release 各 **924 / 0 / 13**、compat 各 **1043 / 0 / 13**（通过/失败/ignored），
较 9.5 各新增 **23** 条（CLI 单元测试 1 + 真实进程集成测试 22）。
**12** 个实际子命令逐个 E2E/schema 校验；hostile 精确 **38** 份，**34** 份打开并检查、**4** 份沿用点名拒绝，无 panic。
fmt 干净，两套 clippy/audit/cargo doc 零告警；曾发现二进制/库同名 rustdoc 路径冲突，现仅对 CLI 二进制设 doc=false，
核心库文档继续构建，命令文档见 docs/18。
八道差分 **242 + 547 已知 / 0 未知**，默认下游生命周期及 compat_ts/TocOptions.ts_shape 不可见探针通过。
save_blocks **204/208 等价、4 份既有具名差异、0 跳过**；可选 JS 产物目录检查因未设置环境变量未执行。
体积门 **251** 份 **4,334,070 B → 1,408,718 B（−67.5%）**；**1099** 份快照无改动，无流段落门仍为 **0**。
最大真实件大纲复测仍为 **5052 UTF-16 / 5415 B / 1354 代理 token、2 页**，默认预算未提高。

文件编辑先生成候选与完整报告，同目录暂存后发布；既有输出需显式覆盖授权。
第二个文件发布失败或 stdout 回执写出失败均恢复旧输出。跨进程预览核完整输入和请求/附件指纹并重新编译，
报告文件独立分页且绑定完整 SHA-256；原生调试仍经 Agent 事务与审计，不计入文本编译任务通过率。
附件级错误新增 operation/sha256Prefix，实际删除附件 SHA 条件后独立 details 断言报红，恢复后通过。

macOS 本机执行真实 CLI E2E；另以 Rust 1.98.0 交叉构建 aarch64-unknown-linux-musl，file 确认静态 Linux ELF。
Linux 运行 E2E 已配置 CI，但本轮没有执行，不能把交叉构建或 CI 配置当作运行证据。
check 明示无 Word 打开证据，只运行当前输入的可检查不变式与两侧诊断；diff 为完整投影单位序号对齐，非最小 diff。
W1–W11 分母和 docs/12 的缺口不变，M8′ 待裁定门不判。

开发期默认全测两次命中 worker 原有 2 秒初始化上限；单独编辑测试通过，停止改动源码后的完整测试通过。
保留失败日志，未调整时限或测试断言，尚未建立唯一根因；不宣称修复过既有引擎缺陷。

以下保留 9.5 及之前各轮交付时的记录。

**M9′ 9.5 文本锚定编辑、预览与审计**：工具层 `rsword-agent-query` 从显式授权范围编译正向操作，
克隆候选会话整批执行，成功后推进版本并返回有界回执；预览共用执行路径，报告与媒体审计可分页、可还原。
歧义、呈现字符、归一后不完整源区间、辅助 arena 误用、样式冲突、超容量报告均在提交前具名失败。
媒体外置采用已批准的 v1.3 五要素绑定，重放校验缺失/篡改并逐字节验证还原序列；不改原生协议。

实跑默认 debug/release 各 **901 / 0 / 13**、compat 各 **1020 / 0 / 13**（通过/失败/ignored），
比 9.4 增加 **42** 条测试；fmt 干净，两套 clippy/audit/cargo doc 零告警。
八道差分 **242 + 547 已知 / 0 未知**，未改语料或快照。
默认下游生命周期与 compat_ts/TocOptions.ts_shape 不可见探针通过；**1099** 份模型快照无改动。
save_blocks 为 **204/208 等价、4 份既有具名差异、0 跳过**；可选 JS 产物目录门未设环境变量，未执行。
**251** 份带图文档 **4,334,070 B → 1,408,718 B（−67.5%）**；最大真实件大纲仍为
**5052 UTF-16 / 5415 B / 1354 代理 token、2 页**。

W1–W11 的分母保持 **11**，各项实测与缺口在 [12-agent-tasks.md](12-agent-tasks.md) §8；
W7 更新 TOC 尚不执行，批次符号引用与桌面 Word 验证未交付。操作清单与测试双向锁死，
不把编译器支持等同于全部 Agent 任务已验收。接口、独立保真断言与开发期缺陷见 [17-agent-edit.md](17-agent-edit.md)。

本轮补强 ModelFingerprint 的活文本与逻辑 Span 投影：旧门既读不到文字，又忽略延迟物化范围。
补门后修复空白裁剪、范围 affinity、字段边界批注、解包后代误判死、样式类型大小写及 FlowMap 刷新缺口；
指纹的段外标记另按结构边界比较，保留名称/种类/位置，不调用保存/物化自证。
扩展 TEST-07 在 debug/release 各跑 **1000 条 × 100 步**，均为 **61055 生效 / 10009 拒绝 / 3064 保存往返**；
保留每 20 步保存、最小化与失败签名匹配，不通过扩大拒绝集过门。M8′ 待裁定门状态不变。

实际做过 **6** 次代码破坏并确认断言失败：去掉附件长度检查、取消批量倒序、恢复空文本指纹、
取消逻辑 Span 投影、取消搬出子树的存活判定、把零长命中的授权末端选择改错；每次均恢复源码。
末项会把应拒绝的首段末端匹配变成成功编辑，证明不能用全投影默认锚点代替授权边界规则。

以下保留各轮交付时的测量记录。

**M9′ 9.4 预算、截断与游标**：Agent text / outline / find / context / document 与诊断/媒体清单共用完整信封双预算、
最长完整前缀选择和一种游标编码；会话写入统一推进版本，文件续页核规范路径与完整 SHA-256，重开产生新会话锚点。
原生 BIND-10 仍无游标；Agent 复用其选择器，按 part 裁剪必要引用闭包，内容字段不能绕过显式范围。
全语料的规范文本单位拼接、计数和抽样续页有精确门；实际破坏下一单位位置后，断言点名中间段落丢失。

本轮最终默认 debug/release 各 **859 / 0 / 13**、compat 各 **978 / 0 / 13**（通过/失败/ignored），新增 **17** 条测试。
fmt 干净，两套 clippy/audit/cargo doc 零告警；八道差分 **242 + 547 已知 / 0 未知**，默认下游探针通过。
save_blocks 的 208 用例为 **204 等价、4 份既有具名差异、0 跳过**；可选 JS 产物目录门未设环境变量，本轮未执行。
251 份带图文档 **4,334,070 B → 1,408,718 B（−67.5%）**；**1099 份模型快照无改动**。
最大真实件大纲复测仍为 **5052 UTF-16 / 5415 B / 1354 代理 token、2 页**，未调高默认预算。

另外稳定复现并修复 fc5ada8 已有的 worker 握手竞态：不再复用子进程仍可能写入的 ready inode 来暂存请求。
独立 pending 文件保留原期限与 kill/wait；恢复旧方式时确定性回归报红。
一轮 compat 异常退出没有保留 stderr，不能断言唯一由此竞态导致；最终四套测试均通过。
边界、开发期失败与常驻验证见 [16-agent-budget.md](16-agent-budget.md)。9.5 编译/报告及 CLI/MCP 尚未交付，M8′ 待裁定状态不变。

**M9′ 9.3 大纲、定位与上下文**：工具侧 `rsword-agent-query` 提供标题树/无标题分组、
字面与正则的归一定位、完整原文前置条件，以及按授权流/块范围读取的上下文和对象详情。
两类锚点复用 9.2；reason 非空且属于 CATEGORIES，空流有独立呈现位置。
零长中间 right、所选流/授权范围末端 left，两端一致；中间/末端分支的两次生产代码破坏均触发断言，恢复后重跑。
context 在段/单元格末端也遵循 left，不借用后一单位。

实跑默认 debug/release 各 **842 / 0 / 13**，compat 各 **961 / 0 / 13**（通过/失败/ignored）。
fmt 干净，两套 clippy/audit/cargo doc 零告警；八道差分 **242 + 547 已知 / 0 未知**，默认下游边界探针通过。
save_blocks **204/208 等价、0 跳过**；251 份带图文档 **4,334,070 B → 1,408,718 B（−67.5%）**。
**1099 份模型快照无改动**。全真实件大纲、全语料字面/正则独立扫描与预算、越界、超时回收检查常驻测试。

最大真实件 26 标题的 content 为 **5052 UTF-16**、完整信封 **5415 B / 1354 代理 token**；
默认 limit=4000 UTF-16 / maxBytes=16000 保持不变，实测 **2 页**，测试锁住预算及页数。
查询默认 deadline=250 ms 从向已就绪 worker 提交请求开始，含归一、编译、匹配和响应读取；
不含不接收 pattern/text 的执行器初始化（另限 2 s）。超时 kill + wait；不以输入上限或仅停止等待替代取消计算。
这不是“冷启动加查询 ≤250 ms”的测量结论。regex/Unicode 依赖只在工具侧，核心运行期依赖树未增加它们。
详见 [15-agent-query.md](15-agent-query.md)；统一会话版本推进、通用分页游标与窗口分段续读现由 9.4 接入，
不宣称 CLI/MCP 或 22 项 Agent 端到端任务已经完成，M8′ 门 2 / 门 4 待裁定状态不变。

**M9′ 9.2 文本投影与双向锚点**：`agent::text::project` 提供内部只读投影，
源文字 / 呈现字符分段覆盖 UTF-16，保留 part、原生 FlowId 与对象身份；单元格选择复用父表的绝对子范围。
标题、编号、管道表、辅助流与占位符随同省略统计输出；17 类 fixture 与版本化诊断说明表有成文清单双向校验，
未知诊断保留原字段并回退原 message。列表由 resolve 计算，尚不支持的地区/自定义格式具名降级，不猜编号。
这是索引构件，不是 CLI/MCP 工具；9.4 已提供有界读取与版本推进，编辑编译留给 9.5。

SPAN-01 纯增量补齐 `w14:txbx` / 各 `w:docPartBody`：旧流先编号，新流追加；
无流内容容器报 `SPAN_NO_FLOW`。glossary 不自动展开为正文，报告可寻址身份、段落数和省略原因。
全语料检查精确锁定 **1099** 份成功投影与 **4** 份点名打开失败，无流段落精确为 **0**；
同一文档两次投影 JSON 字节一致，源定位合法且全部字符锚点双向可查；**1099 份模型快照没有改动**。
四条常驻破坏用例分别确认缺映射、错 part、伪造 source 与代理对中点会具名失败。
本轮实跑默认 debug/release 各 **827 / 0 / 13**、compat 各 **946 / 0 / 13**（通过/失败/ignored）；
fmt 干净，两套 clippy/audit/cargo doc 零告警，八道差分 **242 + 547 已知 / 0 未知**。
save_blocks **204/208 等价、0 跳过**（其余 4 份沿用具名预期差异）；251 份带图文档
compat 4,334,070 B → native 1,408,718 B，**−67.5%**。详见 `14-agent-text.md` 的实现边界与测试契约。

**M9′ 9.1 关口已评审通过（2026-09-09）**：[spec/22-agent.md](../spec/22-agent.md) 定义 AGENT-01–10，逐条带验收。
预算默认值已生效，随对应任务落地。9.0 文档与测量已复核通过；B/C 宿主实测仍缺证据，M8′ 门 2 / 门 4 待裁定不变。
9.1 本轮重跑默认 debug/release 各 **792 / 0 / 13**、compat 各 **911 / 0 / 13**；fmt 干净，
两套 clippy/audit 零告警，八道差分 **242 + 547 已知 / 0 未知**。十条规范各有验收小节，尚未执行这些新增验收。


**M9′ 9.0 文档与测量**（基点 e132df4）：[Agent 任务集](12-agent-tasks.md) 定义 22 条（读改各 11），
输入条件已核实，尚未执行 Agent 验收或桌面 Word 打开验收，不报告任务通过率。最大真实件
`misc/large-report.docx` 为 326406 B；真实 document(display=false) 为 **128519 B / 124146 UTF-16 单位**，
按 ceil(bytes/4) 估算 **32130 token**，非 tokenizer 实测。复现脚本 `tools/agent-baseline.sh` 连跑两次结果相同。
outline/text 尚未实现，预算为 9.1 的建议；B/C 宿主实测缺证据，未宣称复现。M8′ 门 2 / 门 4 待裁定状态不变。
本轮实跑默认 debug/release 各 **792 / 0 / 13**、compat 各 **911 / 0 / 13**（通过/失败/ignored）；
fmt 干净，两套 clippy/audit 零告警；八道差分 **242 + 547 已知 / 0 未知**。没有改引擎、规范或语料。

> **范围改定（2026-09-08，项目负责人）**：rsword 是**独立的 docx 读写内核**，不再以「替换 genoffice 引擎」为目标。
> genoffice 退为**测试基准**（只读地跑它的 TS 引擎生成 `corpus/**/*.expected.json`）。交付 **Rust crate 优先**，
> wasm / CLI 是绑定。目标形态是 Word / WPS 的**外挂应用**（文件级工具），对 docx 阅读与修改、后续接入 Agent，**不做渲染**。
> 原 M8（编辑器切换）撤销，原 M9 拆成 **M8′**（原生协议与独立交付）与 **M9′**（Agent 接口层与文件级工具）。
> `compat_ts` **不删**，降为测试专用 feature。详见 `docs/03` v3.3、`docs/04` §17、`spec/19` / `spec/20`。

**M7 完成并已并入 `main`**（2026-09-08，`main` = 32234ce，从 e5bed96 快进 19 条提交）：修订生成（`track_changes`）
与接受 / 拒绝、`EditOp` 全集补齐（60 个变体）、分节符增删、绘图编辑与 z-order、`mc:Fallback` 双生同步、
块字段生成器（TOC / SEQ / INDEX）、空白文档模板、`TEST-07` 随机编辑序列、wasm-bindgen JS 绑定。
六条门全过：**646 测试**、九道 `diff-parse` 门 + 两条 `--via js` 全为 0 未知差异、`save_blocks` 204/208 等价
（外加部件级 189/289 比对）、1,000 条随机编辑序列在 debug 与 release 双跑无失败。
其中 `TEST-07` 一道门就查出并修掉 **17 个引擎缺陷**。任务分解见 `spec/18-m7-plan.md`，逐条进度见 `docs/04` §16。

**M8′ 实施记录**（分支 `m8-native-json`，工作树 `../rsWordParser-m8j`）——`spec/19-m8-plan.md`，8 个任务、6 条门。
**8.0 已收口**（2026-09-08）：文档改定并入；`m8-editor` 的 8.1a 摘进 `m8-native`（`parse_diagnostics`、
`BindBadArgument`、node 实测 harness `tools/js-parity/`、`TOOLS.md`、CI wasm 步骤），8.0a 丢弃；门重跑全绿。
**8.2 已完成**（2026-09-08，分支 `m8-native-json`）：`bind/native/` 模型 JSON 投影——`model_json!` 同表展开
`impl ToJson`（编译期完整解构「不丢字段」）+ JSON Schema + 覆盖测试；30 张生成属性表由 `build/props.rs`
从同一份 TOML 发射投影（`$OUT_DIR/props_json.rs`）。门 1：1,099 份语料过 `document_schema()`
（display 开 / 关两遍，精确点名 4 份 hostile 必须在 `Package::open` 失败）+ 投影确定性 + 重建稳定性 + 键集严格性 + 深度护栏（448 层；规范措辞已随 c19fbb3 回写 v3，见 `docs/04` §8）+ `MOD-01`–`MOD-11`
独立 checklist；门 6 体积：251 份带图文档较 `compat_ts::parsed_doc` **-67.5%**；本次修复后测试 **765 passed**（debug / release 各 0 failed、14 ignored，其中 12 个 ignored doctest）。

**8.3 已完成**（2026-09-09，分支 `m8-native-json`，按项目负责人 c19fbb3 的 BIND-03/04 v3）：
66 变体的线型 / 上下文化转换 / 逐变体测试同源；**57 无损往返 + 9 具名拒绝**，九项另有结构化正向往返。
成文拒绝集见 `10-native-edit-json.md`，与独立常量及实跑分类双向锁死。属性 patch serde/schema 由生成器维护，
Keep / Unset / Set / Patch 的分支不塌缩；EditContext 全字段可选，MutationResult 五字段完整投影。
六族声明操作已公开且按键幂等，参考文献未变条目原字节有断言；原生保存不再隐式清洗，compat 显式沿用文档标志。
协议 apply 失败不提交 DOM / interner / 诊断，逃生口按会话累计并随诊断返回。
实测 **1065 份 synthetic + real** 的结构化插段经协议 / 原生保存逐字节相等，空选项保存原字节不变。
最终 workspace debug / release 各 **862 passed、0 failed、14 ignored**（其中 12 个 ignored doctest），clippy 零告警。
八道差分门仍为 **242 + 547 已知 / 0 未知**；save_blocks **204/208 等价、0 跳过**；门 6 体积
4,334,070B → 1,408,718B（251 份带图文档，**−67.5%**）。会话表、导出、媒体、resolve 与克隆成本已由 8.4 交付（见下）。
包括 hostile 的全语料无编辑保存专门门、TEST-07 协议迁移与 fuzz_bind 按排期在 8.6。

**8.4 已完成**（2026-09-09，`spec/21` v3.1，分支 `m8-native-json`）：
原生入口为 `bind::native::SessionTable`，`open(bytes, options_json?)` 返回不透明字符串；
各 JSON 参数以 `Option<&str>` / `&str` 传入，结果 JSON 为 `String`，字节出口为 `Vec<u8>`。
`close` 幂等；`apply` 成功才提交；`save` 在克隆上做，成功与失败都不推进会话。
`document` 支持 display / blockRange / fields / depth，并返回 totalBlocks / truncated；
跨块 spans / fields / revisions 始终全量。原 `Document` 模型 schema 不改，另由同份 schema 的
`DocumentResponse` 定义声明可裁剪字段与元数据，8.2 的完整模型 checklist 继续严格生效。
五个批量 resolve 输入 nodeId JSON 数组，`resolveSections` 的隐式节收 `{ "sectionIndex": N }`；
可选 part 缺省主 part。坏 part 返回 BIND_ID_UNKNOWN，坏节点逐项 `{ "error": "BIND_ID_UNKNOWN" }`。
输出 `{ value, provenance }`：run / para / cell 的 props 与逐字段来源分别在两侧；run 的 cs 单列，
cell 的 rpr / ppr / conditions 完整保留。节的 hf 六槽带 Declared / Inherited / Absent；表格保留
列宽来源、条件样式层、有效单元格、行高、边框和边距。原生查询不改模型或 DOM。
媒体表只追加句柄，按内容与 MIME 去重；`partBytes` / `nodeXml` 为只读调试设施。
Clean XML 的 partBytes 从 ZIP 取原编码，脏 part 用当前 DOM 序列化；nodeXml 在临时克隆上补继承声明，
保留原前缀，原会话不变。`diagnostics` 复用 `{ diagnostics, xmlEscapeCount }`，只统计成功提交的逃生口。
wasm 的 `SessionTable` 类与旧兼容五函数共享 `bind_export!`；真实 wasm 的 13 个缺失会话出口、
open / apply / save / close、媒体、查询与调试出口经 Node 验证，`native_parity.mjs` 已接 CI。

定向语料：**1099 成功 + 4 点名拒绝**（synthetic + real + hostile），五个查询分别覆盖
**3779 run / 3693 段落 / 526 单元格 / 1128 节 / 263 表格**；只读出口校验
**9060 part / 344 媒体 / 12165 XML 子树**。属性 JSON 独立解码回引擎型，来源逐字段比较。
破坏性验证：把 run 的 cs 输出取反，`bind_06_run_full_corpus` 立即失败；恢复后重跑全套。
8.4 修复绑定会话路径在 `setHeaderFooter` 预备 part 时的越界索引：该代码由 8.3 的
`69def07` 在 `bind/native/edit/mod.rs::apply_edit_json` 引入，`7a72140` 补上界检查。
u32::MAX 的 sect 经绑定复现 panic，修复后返回 EDIT_TARGET_MISSING，失败后的会话字节与诊断不变。
撤回将其归为原生引擎缺陷的表述；原生 `EditSession::apply` 的 EDIT_BAD_POSITION 守卫不经过此路径。

`cargo bench -p rsword --bench session` 在 **266** 份真实文档中按 ZIP 文件大小取最大者
`corpus/real/misc/large-report.docx`（**326406 B**），预热后 **31** 次采样。批量请求按该文档实际 run 顺序循环取足 1000 个 ID（不要求互异）：

| 操作 | 中位数 | p95 | 最大 |
| --- | --- | --- | --- |
| EditSession::clone（不含释放） | 0.424 ms | 0.608 ms | 0.681 ms |
| resolveRuns（1000 个 ID，含索引、解析参数与 JSON 输出） | 11.523 ms | 11.768 ms | 13.522 ms |

均低于 50 ms，保留克隆实现，不引入回滚式保存或 resolveAll。
最终 workspace debug / release 各 **897 passed、0 failed、13 ignored**（11 个 ignored doctest）；
合并导出宏时移除了旧 wasm_export 的一个 ignored 文档示例，没有删除执行中的测试。fmt 干净、clippy 零告警。
八道差分门 **242 + 547 已知 / 0 未知**；save_blocks **204/208 等价、0 跳过**（4 项既有有意差异）；
既有 BIND-02 模型投影体积门仍为 **251 份，4,334,070B → 1,408,718B（−67.5%）**。
66 个 EditOp 的分类仍为 **57 无损往返 + 9 具名拒绝**。真实 wasm 重建及 Node 验证也包含超大 sect 的具名错误。
复现：`cargo test --workspace`、`cargo test --workspace --release`、`cargo clippy --workspace --all-targets`；
`cargo bench -p rsword --bench session`；`tools/build-js.sh` 后执行
`node tools/js-parity/native_parity.mjs --pkg crates/rsword-js/pkg`。差分与体积沿用本文既有门命令。
8.5 观察版见下；8.6 hostile 无编辑保存专门门与协议随机序列、8.7 文档改名仍待各自任务。

**8.5 已完成观察版**（2026-09-09，BIND-11）：根部重导出稳定核心类型，旧路径仅从文档隐藏，
下游仍可调用。**缩小实际 semver 面推迟到观察期之后（负责人 2026-09-09 决定），门 3 的“缺省小面”
这半条未达成**；默认构建排除 compat_ts 在 8.5 时仍待 8.7，现已落实（见下）。feature 名称已定为 native / wasm / serde / compat-ts，
默认 native；serde 仍是共享运行期依赖，不承诺关闭 feature 就去掉依赖，重复的 dev serde 已删除。
稳定类型及其固有 impl 由 `rsword_api_docs` 下的 rustc 检查文档；成文清单见 [13-public-api.md](13-public-api.md)，
**27 个类型、46 处审计注解位置**与整个 src 的 token 扫描双向锁死，包含宏模板，并锁定祖先可见性。
结构体 / 枚举用 non_exhaustive；固定值对象的豁免逐项登记。DiagCode 发布表禁止删除或更名。

实测破坏性验证：EditContext 新增无文档方法 → audit 构建退出 **101**；摘掉其 impl 注解 → 清单门退出 **101**。
恢复后两道门均绿，CI 中 audit 显式传 `--cfg rsword_api_docs -D warnings`，不是只记录 warning。
read / edit / agent 三个 example 已实际执行并进 CI，README 三段代码与可执行 example 一致。
大纲示例在 large-report 输出标题；编辑示例另存后重开读到替换文字；agent 从模型 JSON 生成 EditOp JSON，
apply / save / reopen 验证结果。不会调用外部模型服务，也不覆盖输入。

本轮 workspace debug / release 均 **901 passed、0 failed、13 ignored**，fmt 干净，clippy 零告警；
`cargo doc --no-deps` 与 audit 均带 `-D warnings` 通过。八道差分门 **242 + 547 已知 / 0 未知**，
save_blocks **204/208 等价、0 跳过**（4 项已登记有意差异；其中 43 份字节相同），
模型投影体积门 **251 份、4,334,070B → 1,408,718B（−67.5%）**；操作分类 **57 无损 + 9 具名拒绝 = 66**。

`cargo bench -p rsword --bench session` 本轮仍在 266 份真实文档中选中
`corpus/real/misc/large-report.docx`（326406 B），预热后 31 次；与 8.4 相同采样方式：

| 操作 | 中位数 | p95 | 最大 |
| --- | --- | --- | --- |
| EditSession::clone（不含释放） | 0.433 ms | 0.555 ms | 0.604 ms |
| resolveRuns（循环取足 1000 个 ID，含索引、参数解析与 JSON 输出） | 11.501 ms | 12.237 ms | 13.241 ms |

仍低于 50 ms，不改变克隆实现；本任务没有性能优化，不把计时浮动解释为性能改进。
`--no-default-features` / `--all-features` 构建均通过；真实 wasm 重建后，Node 会话 parity
验证通过（缺失会话、生命周期、原子性、媒体、查询和只读出口）。

**8.6 实现已落地，验收待裁定**：全语料正式门精确覆盖 **1103 份输入 = 1099 成功 + 4 点名拒绝**，
成功文档均保存原字节不变。原 8.3 的 1065 份 synthetic + real 空保存断言迁移到正式门，
仅扩展 hostile，未重复实现。首次新增 **1099 份 `.model.json`、4,995,524 B**，来自 display=false 的
`SessionTable::document`：799 synthetic、266 real、34 可打开 hostile；其余四份不得伪造快照。
位置 **待追认**，TS expected/save 记录未改。CI 超过 **20** 份快照变动提示复核，本次 1099 份已触发；
单份内容或文件集合漂移仍硬失败。注入 `zzSabotage` 后快照测试实际退出 101，恢复后通过。

TEST-07 记录、解码和执行协议 JSON，与原生会话逐步比较模型及诊断；Err 时状态逐字节不变。
每 20 步保存重开、失败最小化、同失败签名匹配和 ModelFingerprint 两视图断言均保留。
深包装 Walker 改迭代：1099 份文档的新旧两视图输出逐字节相同；5000 层专门回归使用默认栈，
未提高交付测试的栈大小，也未跳过深文档。

新门暴露并修复 settings 修改后投影未刷新、AddComment 拒绝前创建 part，以及部分快照回滚
遗漏新 part/关系、模型告警和 revision id；外层事务改为完整 EditSession 检查点。
`invariants_clean` 同时查会话与包诊断，release 的 SAVE-02 警告不再漏报；门自检主动删除 gridCol，
debug 必须 save Err，release 必须由包级诊断被门捕获。

网格修订按评审授权修复：掉格阶段已同步删除当前 gridCol 时只摘历史标记，保留存活当前列宽；
未掉列的纯宽度快照仍走旧路径。外层事务提交前校验所有行几何，失败完整回滚。
不自动补网格，不扩大拒绝集掩盖原回归。`m6-chart__070`、seed **10201016167453558198** 的
**40 → 26** 步序列永久保留，要求最后 reject 成功，得到四列网格与三行四列；另测不同宽度的
后插列与非法快照原子拒绝。`revisions` 的 **24** 个既有测试均通过。
**spec/18 7.4 措辞仍待负责人裁定，门 2 / 门 4 不判**；合法但语义错误的剩余快照交互已单列 docs/04 §8。

完整事务检查点成本经 `cargo bench -p rsword --bench session` 实测：最大真实文档
large-report.docx（326406 B），预热后 31 次采样，准备与释放不计时：

| 操作 | 中位数 | p95 | 最大 |
| --- | --- | --- | --- |
| EditSession::clone | 0.264 ms | 0.334 ms | 0.383 ms |
| resolveRuns（1000 个 ID） | 12.247 ms | 13.132 ms | 13.244 ms |
| 原生 apply（含事务检查点） | 0.854 ms | 0.947 ms | 0.986 ms |
| 协议 apply（含协议边界与事务检查点） | 1.417 ms | 1.525 ms | 1.757 ms |

apply p95 均低于 5 ms；克隆与批量 resolve 低于 50 ms。8.4 的 0.608 ms 与 8.6 的 0.334 ms 都是在同一最大文档刚打开的会话上，预热一次、31 次单次 clone、排除释放，度量口径未变；checkpoint 增加的是 apply 内部克隆次数，已计入 apply 基准，同口径两次测量 0.608 / 0.334 ms（31 样本），属测量噪声，未做优化。

本轮完整 workspace debug / release 均 **911 passed、0 failed、13 ignored**；fmt 干净，
clippy 零告警，audit 与 rustdoc 均带 `-D warnings` 通过。八道差分门仍为
**242 + 547 已知 / 0 未知**；save_blocks **204/208 等价、0 跳过**（4 项既有有意差异），
本轮未另跑真实 Node 保存 parity，不将该条件分支的跳过计为已验证。
体积门 **251 份、4,334,070 B → 1,408,718 B（−67.5%）**；**57 无损 + 9 具名拒绝 = 66**。
TEST-07 **1000 条 × 100 步**在 debug / release 均通过，计数一致：
**61,043 次生效、10,012 次拒绝、3,065 次保存往返**。
另外三次破坏性验证均退出 **101**：禁用网格调和，保留后插列的成功断言失败；
移除最终几何校验，非法快照的 unwrap_err 失败；移除 package diagnostics 链接，
release 门自检失败。每次恢复源码后定向测试通过，并逐字节确认恢复结果。
网格修复后重新运行 `fuzz_bind`：**28,853 runs / 601 秒，无崩溃**，任意 JSON 经 apply、document
与五个 resolve 入口；Err 与只读查询均比较模型 JSON、诊断/逃生计数和保存字节。
使用 `/tmp` 中的运行语料副本，工作区仅保留三个手工种子；10 分钟实跑不替代待裁定的门结论。

**8.7 实现收尾**：默认构建不编译 compat_ts 或旧无状态 JS 入口；开启 `compat-ts` 后完整保留
差分、保存 oracle 与 TS fieldgen fixtures。diff-parse 通过显式 feature 转发及 required-features
隔离，workspace 默认构建不会间接开启 compat。`TocOptions.ts_shape` 默认也不存在。
CI 两种 feature 分别运行测试、clippy、audit、doc 和三个 example；独立下游门实际完成读改存，
并断言 compat_ts 导入为 E0432、ts_shape 字段构造为 E0560，避免误把 doc(hidden) 当作物理门控。
**门 3 的默认排除兼容层这一半已兑现**；其他隐藏公共项仍保留观察期，不承诺已私有化。

旧 `save_01_no_edit_returns_original_bytes_for_all_corpus`（Package 路径）已去掉 `n > 570`
与打不开即 continue：精确 1103 输入、1099 成功、4 点名拒绝，与协议快照门共享 UNOPENABLE。
文档编号为 [10-native-edit-json.md](10-native-edit-json.md)、[13-public-api.md](13-public-api.md)，
两处 include_str 与所有直接引用同步，留 docs/11、docs/12 给 M9′。

`cargo bench -p rsword --bench bind` 的生产默认 feature 数据（预热一次、31 次样本，单位 ms，均为 p95）：
准备状态与释放在计时外；save 测一次 InsertText 后的实际保存，MB 为输入 ZIP 的十进制 MB。

| 文档（corpus/real 下） | ZIP B | open | document | apply | save | save ms/MB | media |
| --- | --- | --- | --- | --- | --- | --- | --- |
| misc/large-report.docx | 326406 | 4.452750 | 0.779667 | 1.949291 | 3.534125 | 10.827390 | 0.000625（新增 1 KiB 句柄） |
| ole/ole-ppt.docx | 47294 | 0.275667 | 0.039167 | 0.086333 | 0.243292 | 5.144247 | 0.006459（1960 B） |
| _round3/_resaved/ink-to-shape--newchart-resaved-by-word.docx | 47098 | 0.613375 | 0.082375 | 0.205000 | 0.637708 | 13.540023 | 0.003667（233 B） |

最大文档没有原有媒体，media 一栏是明确标注的新增句柄实验，不伪称读取了它的源媒体。
开启 compat-ts 后另测体积，原生 JSON 字节数与默认构建相同（document 完整响应、display=false）：

| 文档 | compat ParsedDoc B | native document B | 减少 |
| --- | --- | --- | --- |
| large-report | 483775 | 128519 | 73.4% |
| ole-ppt | 13578 | 7182 | 47.1% |
| ink-to-shape--newchart-resaved-by-word | 140086 | 13246 | 90.5% |

单份 ole-ppt 不满 50% 不改变门 6 的带图全语料聚合口径；不删掉该行来美化数字。
默认 wasm 原文件 **3,365,804 B**、gzip **999,245 B**；compat wasm 原文件 **3,807,435 B**、
gzip **1,155,713 B**，均低于 3 MiB。使用 wasm-release + wasm-bindgen、gzip -n；未安装可选 wasm-opt，
未把省略的优化记成已运行。两种真实 wasm 的原生会话检查均通过，默认另断言旧五个兼容导出不存在。
最终默认 debug/release 各 **792 passed / 0 failed / 13 ignored**；compat 各 **911 passed / 0 failed / 13 ignored**。
fmt 干净，两套 clippy、audit（`-D warnings`）与 rustdoc 均零告警。八道差分 **242 + 547 已知 / 0 未知**；
save_blocks **204/208 等价、0 跳过**，真实 JS 保存 **208/208 字节相等**、blank 六项通过。
体积门 **251 份 −67.5%**；**66 变体（57 无损 + 9 具名拒绝）**与 **1099 份快照**不退。
默认构建 fuzz_bind **25,336 runs / 601 秒**，退出码 0、无崩溃；种子扩充仅写临时目录。
**spec/18 7.4、门 2 / 门 4 仍待裁定；docs/03 §8.2 升版未批准，本次不动。**

**M0 完成，M1 完成**（1.1–1.15 全部落地，M1 门三条都有测试覆盖），**已全部并入 `main`**（2026-09-04）。
**M3 完成**（2026-09-05，分支 `m3-tables`，3.1–3.9 全部落地，**M3 门四条都跑过**：
`diff-parse --scope tables` 311 份 0 未知差异、单元格文本编辑保真 67 份、5,000 层 `xml-deep-table`、
表格操作随机序列 200 步 × 10 份）：表格属性表（3.1）、表格模型（3.2）、`SdtInfo` 完整模型与锁定 / 绑定的
编辑拒绝（3.3）、`resolve` 表格视图（3.4）、`compat_ts` 表格投影与 `--scope tables`（3.5）、
容器级刷新与单元格内编辑（3.6）、表格属性操作（3.7）、行列结构操作（3.8）、随机序列与恶意输入（3.9）。
任务分解见 `spec/14-m3-plan.md`，逐条进度见 `docs/04` §12。

**M2 完成并已并入 `main`**（2026-09-05，fast-forward 21 条提交；2.1–2.10 全部落地，M2 门两条都跑过）：
Span 索引与 Anchor 变换 / 物化（2.1–2.3）、字段子系统与它的模型 / compat（2.4–2.5）、
批注与注释（2.6：读侧 + 三个编辑操作 + `SAVE-05` 新建 part + compat 权威条目列表）、
符号字体（2.7）、`rId` 分配（2.8）、段落 / 书签 / 字段操作（2.9）、`fuzz_instr` 与 M2 门（2.10）。
任务分解见 `spec/13-m2-plan.md`，逐条进度见 `docs/04` §11。
**M4（绘图显示模型）完成并已并入 `main`**（2026-09-05，4.1–4.8 全部落地）：媒体解析、DrawingML
颜色算法、绘图 / 形状 / VML 显示模型、图片与文本框投影、嵌入对象、恶意输入与门。M4 门
`diff-parse --scope drawing` 573 份 0 未知差异，已接进 CI。任务分解见 `spec/15-m4-plan.md`，
逐条进度见 `docs/04` §13。

**M3 已并入 `main`**（9181eae，2026-09-05；M0–M4 至此全部在 `main` 上）。`main` 之后又前进了一格
（`dcd653d` "compat: cell-anchored shapes and cell image runs"，另一个会话关掉了单元格锚定形状那
19 处差异），**已合进 `m5-hf`**（c437564）；全域未知差异因此从 82 降到 63 处 / 30 份。
**M5 进行中**（分支 `m5-hf`，
工作树 `../rsWordParser-m4`）：5.1 节属性表、5.2 节模型与 `RES-10` 节视图、5.3 页眉页脚 / 注释 / 批注的
内容流、5.4 compat 页眉页脚投影、5.5 页眉页脚与节的编辑操作、5.6 保存选项、5.7 声明 part 的读写、
5.8 resolve 校准（**Word 实测已完成**）、5.9 恶意输入与随机序列**已落地**——**页眉页脚域清零**，
`diff-parse --scope hf` 是第五道门（573 份 0 未知差异，已接 CI）；编辑位置带 `PartId`，
页眉页脚 part 可读可改可新建。**九个任务与五条门全部完成**（2026-09-06 补上了 5.8 的 Word 实测：`RES-04` 的 toggle 规则
按实测改写：`b` / `i` 走层级异或，`caps` / `strike` 一族仍是最具体胜出，八份 fixture 全部 `verified = true`）。任务分解见
`spec/16-m5-plan.md`，逐条进度见 `docs/04` §14。
**M6 开工**（2026-09-06，分支 `m6-embedded` 从 bf1f906 开，工作树 `../rsWordParser-m6`）：图表 / SmartArt / 画布 /
OLE / 公式 / 墨迹的模型与投影、图表与媒体的写侧、`inks` / `partXml` / `kind:"chart"` / `kind:"image"` /
`replaceImage` 的保存映射。任务分解 `spec/17-m6-plan.md`，逐条进度 `docs/04` §15；今天的 62 处全域未知差异与
20 份保存跳过就是它的工作面。
**M6 完成**（2026-09-06，`m6-embedded` 上 6.1–6.9 全部落地，M6 门五条见 `docs/04` §15）：八道 `diff-parse` 门全部为 0
（`--scope all` 成为第八道，进 CI），保存语料 204 / 208 等价、0 跳过；**已并入 `main`**（连同 M7 计划 `spec/18`，2026-09-06）。

现在这套代码能：打开任意语料文档、输出与 TS 兼容的 `ParsedDoc` JSON（含整个绘图域：图片、文本框
与形状、细横线、嵌入对象）、以字节级局部补丁写回并保证未编辑内容零改动；在文本段落上插入 / 删除 / 改 run 与段落属性 / 整段替换 / 拆分 / 合并；维护范围
（书签 / 批注 / 权限 / 移动）的 `Anchor` 并在保存时物化标记；解析字段（复杂 / 简单 / 嵌套 / 跨段）、
定策略、进模型并按 TS 形态输出，且能编辑它们（插入字段、改链接目标、切换复选框、改表单文字、
改结果格式、换块字段的结果）；建批注与注释条目并按需**新建 part**（`SAVE-05`）；把 TS 的
`SaveBlock[]` 与几乎全部 TS 保存选项（节 / 六个页眉页脚槽 / 逐节页眉页脚 / 水印 / 页面底色 /
保护 / 奇偶页眉 / 批注 / 注释）翻成编辑操作；**在页眉页脚 /
注释 / 批注 / 外部文本框 part 里编辑段落**（位置带 `PartId`），改节属性、整体替换或新建页眉页脚
part、挂"同前"引用、写删文字水印、设页面底色与文档级开关。
**不能**：绘图的**编辑**与写回（M7，读侧已完成；图表 / 图片 / 墨迹的写侧已在 M6 6.6–6.8 落地）、
块字段生成器与修订生成（M7）；
**新建分节符**（给某段加 `sectPr` 断节）也不在 M5（§8 有偏差记录）。

## 能力矩阵

| 层 | 状态 | 已实现 | 缺口（里程碑） |
| --- | --- | --- | --- |
| L0 包层 `package/` | 完成 | zip（0x7075 中和、限额、raw copy）、`[Content_Types].xml`、`.rels` 双族、flavor 判定（Strict / Transitional / Mixed）、`NamespaceContext`、新建 part（`SAVE-05`：内容类型 Override + 关系 + `.rels` 自建）、`MediaStore`（按 part rels 解析媒体、MIME、dataURL） | — |
| L1 无损 DOM `xml/` | 完成 | tokenizer（区间精确、属性顺序 / 引号 / 重复容忍）、`Dirty` 五态与传播、MCE（含 `ProcessContent`）、命名空间作用域、`NodeEdit` 计划、片段解析、规范化比较、XPath 子集 | — |
| L2 范围 `span/` | 完成（范围部分） | `FlowId` / `FlowMap`、内容序列（`SPAN-01`）、`Anchor` / `Affinity`、九种 `RangeKind`、按流构建与配对诊断、文档序 `compare`、按容器倒排、编辑期变换与整体删除策略（`SPAN-06/07`）、物化与保存前校验（`SPAN-08/09`）、拆分 / 合并容器时的锚点重定位 | `SPAN-10` 的另一半：范围端点**落在字段指令区内**时移到原子边界（插入侧已按同一规则处理，端点侧还没做） |
| L2 字段 `span/field/` | 完成 | `FieldSpan` 配对（复杂 / 简单 / 嵌套 / 跨段 / 未闭合诊断）、指令 tokenizer 与 76 个关键字的策略表、`w:ffData` 读写、`FLD-13` 基线校验、`fuzz_instr` | 块字段生成器（`FLD-09` 的内容重算，M7） |
| L3 属性表 `semantic/props/` | 完成 | 32 张表由 TOML 生成（读 / 写 / diff / patch / merge / `plan_apply_*`）、按 flavor 编解码、`Val::Raw` 降级、`PROP-05` 顺序；表格三组表 `TableProps`（含 `tblPrEx`）/ `RowProps` / `CellProps` 与边框 / 边距子表、`MeasureOrPercent` codec（3.1）；节表 `SectionProps` 与四张子表（5.1，`para.toml` 的 `sect_pr` 已接表） | — |
| L3 模型 `model/` | 文本 + 表格 + 字段 + 批注 / 注释 + 绘图 | `Document::rebuild`、块分类 R01–R19（含 R09 字段块）、段落坐标流（`Run`/`Segment`，UTF-16）、`Inline::Field` 与透明字段、`ParagraphFacts`、**表格模型**（`TableBlock / Row / Cell`，穿透 sdt 与修订包裹，声明网格，表格修订，> 64 层 TooDeep，`MOD_TABLE_SHAPE` 诊断；3.2）、跨表格的 `blocks()` / `paragraphs()` / `block_path()`、**内容控件**（`SdtInfo`：16 种控件 / 四态锁 / 数据绑定 / docPart / 占位符；3.3）、绘图 / 形状 / VML 显示模型（`Segment.display` / `ProtectedBlock.display` / `ImageBlock.display`）、声明模型（styles / numbering / theme / settings / fontTable / comments / footnotes / endnotes）、**节模型**（`SectionInfo` + `section_of` + `SectPropsChange`，5.2）、**页眉页脚 / 注释 / 批注的内容流**（`HfPart` / `AuxFlows` / `Note.blocks` / `Comment.blocks`，5.3）、**参考文献源**（`customXml` 里的 `b:Sources`，5.7）、**图表 part 模型**（`ChartPart` / `ChartDisplay`：种类 / 标题 / 类别 / 系列 / 颜色 / 调色板，chartex 降级；`Document.chart_parts` + `DrawingDisplay.chart`，6.1）、**SmartArt 与画布模型**（`DiagramPart` 数据 part 文字树 + 绘图 part 形状、`CanvasDisplay` 子坐标系，`ProtectedBlock.siblings`，6.3）、**公式模型**（`FormulaDisplay`：片段 / token / MathML / LaTeX，两个迭代转换器 `model/omml`，6.5）、**墨迹**（`Document.inks` / `InkInfo`，`SegmentKind::Ink` 长度 0、对分类不可见，6.8）、**修订索引**（`Document.revisions`：全包按文档序的 `RevisionEntry`，24 种 `RevKind`、九种宿主、嵌套 `depth`、`moveFrom` ↔ `moveTo` 配对、会话内稳定的 `RevisionId`；直接扫 DOM，7.1） | — |
| resolve `resolve/` | 首版 + 表格 + 节 | 样式链（basedOn / link）、docDefaults 层叠、每字段 `Provenance`、主题字体与颜色、符号字体解码、heading 级别、DrawingML 颜色算法、**节视图**（`RES-10` 的六槽继承与有效变体，5.2）；**表格视图**（`tblLook`、表格样式链的条件格式、边框 / 边距回退、行高截断、`ColumnView` 的四条列宽启发式与 `hMerge` 折叠、`RES-03` 第 4 层；3.4） | toggle 规则里 `strike` 一族只在 Word 网页版测过、`vanish` 与 `bCs` / `iCs` 没测到（`docs/06-toggle-open-question.md`）、补全 Wingdings 2/3 与 Webdings 映射表 |
| L4 编辑 `edit/` | 段落 / 范围 / 字段操作齐了，单元格内可编辑 | `EditSession`（含范围索引）、`InlinePos` 定位、`MutationPlan` plan/validate/commit、按 part 回滚的事务（DOM + 索引）、`InsertText`、`DeleteRange`（同段）、`SetRunProps`、`SetParaProps`、`ReplaceInlines`、`ReplaceParaProps`、`InsertBlock`/`DeleteBlock`/`MoveBlock`、`SPAN-06/07` 锚点维护、`AddComment`/`RemoveComment`/`SetCommentText`（含 `SAVE-05` 新建 part）、`SplitParagraph`/`MergeWithNext`、`AddBookmark`/`RemoveBookmark`、`InsertField`/`SetLinkTarget`/`ToggleCheckbox`/`SetFormText`/`SetFieldResultProps`/`UpdateBlockField`；**单元格内编辑**（`InlinePos.para` 可为任意深度的 `w:p`，容器级刷新，格尾自动保持 `w:p`；3.6）、`SetTableProps`/`SetRowProps`/`SetCellProps`（3.7）、**行列结构操作**（`InsertRow`/`DeleteRow`/`InsertColumn`/`DeleteColumn`/`MergeCells`/`NewBlock::Table`，声明网格几何 + 书签列区间维护；3.8）；**位置带 `PartId`**（`InlinePos { part, para, offset }` 与 `BlockPos { part, at }`，段落 / 块 / 范围 / 字段操作在页眉页脚 / 注释 / 批注 / 外部文本框 part 里原样可用，只有主 part 才有的 id 显式拒绝；5.5a）、**节与页眉页脚操作**（`SetSectionProps`/`SetHeaderFooter`/`LinkHeaderFooter`/`SetWatermark`/`SetPageColor`/`SetDocumentSettings`，含按 `SAVE-05` 新建 `header{N}.xml`；5.5b） | 新建分节符、绘图的编辑与写回、块字段生成器与修订生成（M7） |
| 保存 `save/` | 六步齐了 | `SAVE-01` 六步编排（含第 3 步 Span 物化）、`SAVE-02` 子集校验（未绑定前缀、`PROP-05` 顺序、`SPAN-09` 范围检查）、`SAVE-05` 新建 part（追加在 zip 末尾；批注 / 注释 / **页眉页脚** / `settings.xml` / **样式 / 编号 / 主题 / customXml**）、扩展命名空间声明、`w:t` preserve、`raw_copy_file` 写回、`SaveOptions`（`saved_at`、`remove_personal_info`、`remove_date_and_time`、批注与注释的权威列表；**节 / 页码 / 首页不同 / 页面底色 / 保护 / 奇偶页眉 / 六个页眉页脚槽 / 逐节页眉页脚 / 水印**翻成 5.5 的编辑操作，5.6；**参考文献 / 编号追加 / 主题 / 样式 upsert** 各翻成声明 part 的计划，5.7）、**图表写侧**（`SetChartData` 只改缓存文本、`NewBlock::Chart` 建图表 part + 内嵌工作簿 + 关系、`ReplacePartXml / ReplacePartBytes` 整 part 替换，新建二进制 part 走 `PartDom::Bytes`，6.6）、**媒体写侧**（`NewBlock::Image` 去重媒体 + 随文 / 九种锚定、`ReplaceImageMedia`、保存前 `prune_orphans` 回收本次会话造成的孤儿关系 / part / `.rels` / Override，6.7）、**墨迹**（`SaveOptions.inks` 权威列表 → `RemoveInks` + `InsertInk`，每条一个媒体 part，6.8） | — |
| 兼容 `bind/compat_ts/` | 文本 + 表格 + 字段 + 批注 / 注释 + 绘图 + 页眉页脚 | `parsed_doc` 整份 `ParsedDoc`（含 `extras`、UTF-16 索引、sdt 拆分）、字段折叠 run 与 `fieldDisplay` / `fieldLabel`、`comments` / `footnotes` / `endnotes` / `commentIds` / `noteRef`、`apply_save_blocks`（original / generated / xml 块）、容忍差分；**表格模型**（`blocks[*].table` 全部字段与 `styles.*.tableDisplay`，含 TS 的 `attachRawTablePr` / 深度 8 扁平化 / `tableSummary` 三处半解析；3.5）、整个绘图域（`image*` / `textboxes[]` / `rule*` / `oleProgId`）、**整个页眉页脚域**（`hfParts` / 六变体 / `hfParagraphs` 的样式层与表格行 / `hfImages` / 水印 / 矢量装饰合成 SVG；5.4）、跨 part 内容流（`Ctx::switch`，外部文本框 part）、**图表块**（`chartDisplay` / `previewText` / `extras.chartParts` 原文、chartex 回退图 → 图片块；6.2）、**SmartArt 与画布块**（`previewText` 节点文字树、`diagramDisplay` 形状 / 缩放 / 分栏、邻居绘图的 `textboxes`；6.3）、**OLE run**（同 run 多图形按 TS `splitImageRun` 拆分、字段包裹的对象、单元格里的对象；6.4）、**公式与 ruby**（`formulaDisplay` 的 tokens / mathml / omml / latex、`runs[].math`、`runs[].ruby`；6.5）、**墨迹**（`inks[]`、`options.inks` 的 `blockIndex` 在块操作落定后解析；6.8） | — |

## 公开 API 边界（今天可用的）

```rust
// 读
let mut pkg = Package::open(&bytes)?;            // L0
let doc = Document::rebuild(&mut pkg)?;          // L3 投影
let json = bind::compat_ts::parsed_doc(&mut pkg)?;   // TS ParsedDoc 兼容 JSON

// 编辑 + 保存
let mut s = EditSession::open(&bytes)?;
s.apply(EditOp::InsertText { at: InlinePos::new(para, 3), text: "x".into(), props: None }, &EditContext::default())?;
let bytes = s.save_with(&SaveOptions { saved_at: Some(iso), ..Default::default() })?;

// 范围与字段（规范状态的另一半 / DOM 的投影）
let spans = s.spans()?;                          // SpanIndex：书签 / 批注 / 权限 / 移动范围
let fields = &s.document().fields;               // FieldIndex：形态、指令、策略

// M2 的其余操作
s.apply(EditOp::SplitParagraph { at }, &ctx)?;
s.apply(EditOp::AddBookmark { name: "bm".into(), from, to }, &ctx)?;
s.apply(EditOp::AddComment { from, to, comment }, &ctx)?;   // 需要时新建 comments.xml
s.apply(EditOp::SetLinkTarget { link, target }, &ctx)?;

// TS 保存路径
let outcome = bind::compat_ts::apply_save_blocks(&mut s, &final_blocks_json, &options_json)?;
let bytes = s.save_with(&outcome.save_options)?;
```

错误契约：只有"根本不是 docx / 缺主 part / 超限 / 主 part 畸形 / 编辑位置或计划非法 / 目标被锁
（`FLD_LOCKED`）/ 引擎不变式被破坏"返回 `Err`；其余一律局部降级并记 `Diagnostic`。不支持的输入返回 `Error::Edit { code: EditUnsupported }`，
**不改动任何状态**，调用方可以据此判断"这个能力还没到"。

## 实测数字

| 指标 | 值 | 来源 |
| --- | --- | --- |
| 源码行数 / 文件数 | 82,306 行 / 190 个（另有生成代码，属性表 32 张） | `find crates tools -name '*.rs' \| xargs wc -l` |
| 测试数 | 765 passed、0 failed、14 ignored（其中 12 个 ignored doctest；单元 + 集成 + 绑定，52 个集成测试文件；debug 与 release 双跑） | `cargo test --workspace` |
| 语料 | **266 份真实 Word 文档**（`corpus/real`，2026-09-07 三轮）+ **32 份 Word 对照 fixture**（`fixtures/{revisions,word-ops}`：Word 自己做操作的前后 / 四态）+ 799 份 synthetic（每份带 `expected.json`；其中 226 份是 M6 的嵌入对象语料 `m6-*`）+ 208 份 `save.<k>.json` + 38 份 hostile（含 4 份绘图、2 份表格、4 份页眉页脚 / 节、6 份嵌入对象、6 份修订 / 分节 / 绘图，7.0⑤） | `ls corpus/*` |
| 往返字节保真 | 593 份文档、3,140 个 XML part 全部字节相同（3 个 part 按预期解析失败：两份不闭合 XML + 二进制页眉） | `tests/xml_roundtrip.rs` |
| 声明模型对照 | 2,897 个样式、6,732 项主题颜色等，1 处已知差异 | `tests/decl.rs` |
| 模型对照 | 445 段类型 / styleId、387 段坐标流文本、22 项列表、9 项级别 | `tests/model.rs` |
| resolve 对照 | 86,465 项 `StyleDisplay`、2,326 项 heading 级别、2,897 项 linked shell | `tests/resolve.rs` |
| 解析差分（文本域） | 256 份用例（2.6 起含带批注 / 注释的文档；嵌入对象文档剔除），156 处已登记差异，**0 处未知差异** | `cargo run -p diff-parse --features compat-ts -- --scope text` |
| 解析差分（字段与 Span 域，M2 门） | 283 份用例（文本域 + 字段 / 标记 / 批注 / 注释），**0 处未知差异** | `cargo run -p diff-parse --features compat-ts -- --scope fields` |
| 解析差分（表格域，M3 门） | 352 份用例（字段域 + 表格，含单元格里的锚定形状与图片），**0 处未知差异** | `cargo run -p diff-parse --features compat-ts -- --scope tables` |
| 解析差分（页眉页脚域，M5 门） | 799 份用例，**0 处未知差异**（按**路径**筛，嵌入对象块上的差异剔除） | `cargo run -p diff-parse --features compat-ts -- --scope hf` |
| 解析差分（嵌入对象域，M6 门） | 799 份用例，**0 处未知差异**（按路径 + 期望块的 label 筛） | `cargo run -p diff-parse --features compat-ts -- --scope embedded` |
| 解析差分（全域） | 799 份用例，242 处已登记差异，**0 处未知差异** | `cargo run -p diff-parse --features compat-ts -- --scope all` |
| resolve 校准 fixture | 8 份（7 个 toggle + 1 个节继承）全部 `verified = true`，观察值来自 2026-09-06 的 Word 网页版实测（方法与结论见 `fixtures/resolve/README.md`，未决部分见 `docs/06-toggle-open-question.md`） | `cargo test -p rsword --test resolve_fixtures` |
| 页眉页脚 / 节的随机序列 | 10 份语料 × 100 步（页眉段落内联编辑 + 五个节 / 页眉页脚操作）：986 次生效、10 次被拒，每步 `refresh == rebuild`、无引擎不变式破坏 | `cargo test -p rsword --test hf_ops -- --nocapture` |
| 保存差分 | 208 份 TS 保存用例：204 份与 `saveDocx` 等价（其中 43 份逐字节相同）、4 份有意不同、0 份跳过 | `tests/save_blocks.rs` |
| **真实 Word 语料** | 266 份（Office LTSC 2021 桌面 Word 写出，三轮：`docs/07` 任务 A 110 + M7 语料 17 + 往返样本 14 + Word 另存件 123 + 收尾轮 2）：全部 XML part 往返字节相同、无编辑保存字节相同、改一字后其他条目 CRC 不变；TS 差分 0 处未知（登记 547 处） | `cargo test -p rsword --test xml_roundtrip --test save`、`cargo run -p diff-parse --features compat-ts -- --corpus corpus/real` |
| **Word 验收本引擎的输出** | 1544 份编辑后文档（12 种编辑 × 180 份真实底稿）由桌面 Word 逐份打开：**open 全部 ok**；第二轮那 9 份 `docPr` 撞号与 4 份图表 mismatch 都已修并在第三轮 9/9、4/4 复验通过。见 `corpus/real/_round3/EDITED3.md` |
| **Word 对照 fixture** | `fixtures/revisions/` 四个 case 各 `base`/`tracked`/`accepted`/`rejected`（Word 自己「接受 / 拒绝所有修订」的结果，M7 门第 3 条的 oracle）；`fixtures/word-ops/` 四组 Word 自己做操作的 `before`/`after`（插 / 删分节符、置于顶层、移动缩放） | `fixtures/{revisions,word-ops}/README.md` | `cargo test -p rsword --test xml_roundtrip --test save`、`cargo run -p diff-parse --features compat-ts -- --corpus corpus/real` |
| 嵌入对象的随机序列 | 5 份图表 + 5 份图片语料 × 100 步（图表数据 / 新图表 / 新图片 / 换图 / 墨迹增删 / 删块 / 插字）：633 次生效、0 次被拒、36 次保存，每步 `refresh == rebuild`、保存后无新的悬空关系与孤儿 part | `cargo test -p rsword --test embedded_ops -- --nocapture` |
| 节与页眉页脚 | 573 份 588 个节（与 TS `readSections` 逐份一致）；43 份带页眉页脚 part（47 个 part / 63 个块，`rId` 集合与 `hasPageNumber` 与 TS 一致）；26 个注释 / 批注条目 32 个块 | `cargo test -p rsword --test section --test hf --test notes -- --nocapture` |
| 节与页眉页脚的编辑操作 | 14 个用例（`SAVE-05` 页眉版、已有 part 只重写该 part、`PROP-05/06` 插入位置与原字节、Strict 水印拒绝、六个操作各一组 XPath 断言、`MOD-13` oracle；另加 4 份 hostile 与 `TEST-07` 的 10 × 100 步随机序列） | `cargo test -p rsword --test hf_ops` |
| 跨 part 编辑 | **43 / 43** 份带页眉页脚的语料（M5 门第 3 条）：改页眉后只重写该 part，其他条目 CRC 与压缩字节不变，正文投影不变 | `cargo test -p rsword --test hf -- --nocapture` |
| 节属性往返 | 604 个 `w:sectPr`：0 处 `PROP_BAD_VALUE`、596 个符合 schema 顺序 | `cargo test -p rsword --test props -- --nocapture` |
| 范围索引 | 573 份 / 3012 个 part 的 31 个标记全部成对认领 → 19 个范围（书签 7、批注 12）；1 处孤儿终点 | `tests/span.rs` |
| Span 编辑与物化 | 29 个用例覆盖 `SPAN-01`–`SPAN-09`（含 4 条变换规则、整体删除策略、物化与原字节保真） | `cargo test -p rsword --test span` |
| 字段索引 | 43 份文档 / 57 个字段（`Atom` 33、`Block` 6、`Picture` 6、`Form` 4、`Link` 3、`Object` 3、`Marker` 1、`Unknown` 1）；3 份 TS 截断夹具本来就缺 `end` | `cargo test -p rsword --test field -- --nocapture` |
| 字段模型与 compat | 19 个用例（配对 / 指令 / 策略 / 坐标流 / R09 / 折叠 run / `fieldDisplay`） | `cargo test -p rsword --test field` |
| 段落 / 书签 / 字段操作 | 17 个用例（`SPAN-06` 拆分与合并、`EDIT-06` 书签分配、`FLD-09`/`10`/`12` 各自的验收行） | `cargo test -p rsword --test para_ops` |
| **JS 绑定（`crates/rsword-js`，wasm-bindgen）** | `parse` / `parse_diagnostics` / `save` / `blank` / `version` 五个函数（8.0② 起；`blank` 收 `BlankDocxOptions` JSON，参数错误一律 `BIND_BAD_ARGUMENT`）。`--via-js` 用 node 跑**真实 wasm 产物**复核：synthetic 799 份与 real 266 份都与原生输出**逐字节相同**（再带着绑定输出走差分，0 处未知差异）；`save` 对 **208 / 208** 份保存用例、`blank` 对 none + 四种字体与原生字节相同。wasm 产物 **2.25 MB**（`wasm-release` 档：体积优先 + LTO + 去符号；gzip 后 **796 KB**） | `tools/build-js.sh`、`cargo run -p diff-parse --features compat-ts -- --via-js --scope all`、`cargo test -p rsword --test js_binding --test save_blocks`、`tools/js-parity/` |
| **`TEST-07` 随机编辑序列（M7 门 5）** | 1,000 条（语料 × 种子 × 100 步，操作全集 + 每步随机 `track_changes` + 随机接受 / 拒绝）：61,057 次生效、9,994 次被拒、2,625 次保存往返；每步 `MOD-13` 投影 == 重建、`FLD-13` 不增缺陷、`EDIT-06` 不重号、无引擎不变式破坏。PR 跑 100 条，`random.yml` 每天跑 1,000 条 | `RSWORD_RANDOM_SEQUENCES=1000 cargo test -p rsword --release --test random_ops -- --nocapture` |
| **性能记录**（非门） | 319 KB 的 `large-report.docx`：`open` 6.2 ms、`InsertText` 0.67 ms、`save_with` 0.5 ms、绑定的 `parse`（含 `ParsedDoc` 投影与 JSON 序列化）4.7 ms；带修订的 23 KB 文档 `AcceptAll` 0.18 ms。建议观察值 `apply` < 5 ms、`save_with` < 50 ms / MB，四份都在一个量级之内 | `cargo bench -p rsword --bench edit` |
| **`RES-04` toggle 歧义探针** | 1,065 份文档、3,698 个 run：撞上歧义 13 次，**全部**来自我们自己为 `RES-04` 造的校准件（`toggle-other-toggles-converted`），校准件之外 **0 次**（判据与阈值见 `docs/06` 第 2 件） | `cargo test -p rsword --test resolve res_04_toggle -- --nocapture` |
| 批注与注释 | 语料 11 份带批注（17 条）、5 条注释条目；13 个用例（三部件关联、结构条目、`commentIds` 三形态、`noteRef` 编号、`SAVE-05` 新建 part、三个编辑操作、compat 权威列表） | `cargo test -p rsword --test notes` |

全域差异（`--scope all`）**为 0**（M6 6.9 后；m6.0a 扩充语料时 452 / 196 份，6.5 后 71，6.8 后 28）：已登记的 242 处见
`KNOWN_DIFFS.md`（TS 装载时改写主 part 的三份、未声明 `mc:Choice Requires` 前缀的五份、单元格里的公式 / ruby、没有 `wp:extent`
的画布、EMF 不在 Rust 侧渲染等）。八道门——文本、字段与 Span、表格、绘图、页眉页脚、嵌入对象、全域——都是 0 并都在 CI 里；
嵌入对象域从 420 走到 0 的过程：214（6.2 图表）→ 170（6.3 SmartArt / 画布）→ 160（6.4 OLE）→ 43（6.5 公式 / ruby）→ 0（6.8 墨迹），
6.9 再把域外的 32 处零散差异修掉 12 处、登记 17 处（`docs/04` §15）。
保存侧 **204 / 208 等价、0 跳过**（6.8 后；剩下 4 份是 `INTENTIONAL`）：图表 13、`partXml` 7、`partBinary` 1、图片 11、
`replaceImage` 6、墨迹 23 全部等价。

## 与 TS 有意不同的地方

政策见 `docs/04` §8 开头：TS 是参考实现不是权威，目标功能等价或更强。当前登记在册的语义差异：

| 处 | 我们的行为 | 理由 |
| --- | --- | --- |
| 修订 `w:id` | `EDIT-06` 全局最大值 +1 | TS 固定 `0` / `9001`，重复插入会重号（3 份保存用例因此不等价，见 `INTENTIONAL`） |
| `saved_at` 单独设置 | 不触发保存 | 否则"打开→保存字节相同"的不变式被时间戳打破 |
| `remove_date_and_time` | 独立开关，删批注 / 修订的 `w:date` | OOXML 本来就有 `w:removeDateAndTime`；TS 没有这个能力（**超过 TS**） |
| 新建 `w:t` 的 `xml:space` | 一律写 `preserve` | Word 不再 trim，语义更安全 |
| 段落 `w14:paraId` / `w:rsid*` | 编辑时复用原段落节点，属性保留 | TS 重建成裸 `<w:p>` 会丢（**超过 TS**） |
| `rawRPr` | 取原文字节区间 | TS 重新序列化，我们的是字节等价，对合并更友好 |
| EMF/WMF/TIFF | 标 `MediaKind::Metafile` / `Tiff`，原字节 dataURL，不转换 | `docs/03` §3.5 冻结：转换是可插拔服务，不在 Rust 侧做（4 份 `emf-image__*`） |
| VML 文本框里的随文图片 | 照常给出图片 run | TS 只在宿主段落上预取媒体，框里的 `a:blip` 拿不到 dataURL 就把整个 run 丢了；Word 是画得出来的（**超过 TS**） |
| 目标带反斜杠的 `HYPERLINK` 字段 | 照常折出链接，地址原样保留 | TS 的 `convertibleHyperlink` 正则 `"([^"\\]+)"` 遇到反斜杠整个不认（Windows 路径），我们按"引号里的反斜杠是字面量"处理（**超过 TS**） |
| 认不出的绘图值（畸形 `style` / `coordsize` / VML `path`） | 一律不给该字段 | 猜一个出来会画错；转不出的路径整条不给，实心包围盒比不画更糟 |
| 权威列表删掉批注后 | 空掉的 `commentReference` run 整个删掉 | TS 留一个 `<w:r></w:r>`（`comments__001.save.2` 因此不等价，见 `INTENTIONAL`；共 4 份） |
| 保存前的资源回收 | 只删**本次会话**让引用数归零的关系 / part（`prune_orphans`，缺省开） | TS `cleanupDocxOwnedResources` 连文件里原有的孤儿也删；用户没碰过的东西不该在一次保存里消失 |
| `replaceImage` 目标没有 `a:blip` | 不动 + `EDIT_UNSUPPORTED` 诊断，不分配媒体 | TS 静默返回原 XML 但媒体照样加进包 |
| 墨迹锚点不是段落 | 跳过 + `EDIT_BAD_POSITION` 诊断，不分配媒体 | TS 静默跳过 |
| 墨迹的判据 | `wp:anchor` 里 `docPr/@name` 以 `aidocs-ink` 开头即算，run 带 `rPr` 也算 | TS 正则要求 `<w:r><w:drawing>` 紧邻 |

解析侧的已登记差异（数字 / 符号字体 / 表格显示等）在 `crates/rsword/src/bind/compat_ts/KNOWN_DIFFS.md`。

## 明确未实现

- **字段边界**：`DeleteRange` 覆盖**已识别**字段的结构 run 时整 run 保留、只删文本段——这是**正确**行为
  （透明字段的结果可编辑，字段本身不该跟着消失）；只有畸形 / 未闭合字段的 `fldChar` 才记
  `EDIT_ANCHOR_UNMOVED`。范围端点落在指令区内时还没有按 `SPAN-10` 移到原子边界。
- **字段**：解析、模型、compat 与编辑操作都在（`FLD-01`–`FLD-12`）。还缺的：**块字段生成器**
  （TOC 重算的内容由调用方给，`UpdateBlockField` 只提供机制，生成器在 M7）、compat 的
  `instrField` / `fldBeginXml` 重发（要 begin run 原字节，M7）、跨段 `Block` 字段的边界编辑
  （结果段落只读）。
- **新建 part**：`SAVE-05` 已落地（批注 / `commentsExtended` / 脚注 / 尾注 / `settings.xml` / 缺失的
  `.rels` 都能建，内容类型 Override 与关系同步写，新 part 追加在 zip 末尾）。5.5 起**页眉页脚 part**
  与 `settings.xml` 也能按需新建（`word/header{N}.xml` 取第一个空闲号），5.7 起再加上样式 /
  编号 / 主题与 **customXml**（`item{N}.xml` + `itemProps{N}.xml` + item 自己的 `.rels`）。
  6.6 起图表 part 与内嵌工作簿、6.7 起媒体 part 也能新建（`SAVE-05` 的机制扩到二进制 part）。
- **页眉页脚 / 节**：读侧齐了（节模型 5.2、part 内容流 5.3、compat 投影 5.4，域已清零），写侧也齐了
  （5.5 的六个编辑操作 + 5.6 的保存选项，TS 那 39 份用例全部等价）。还没有的是**新建分节符**
  （给某段加 `sectPr` 断节，M7 与段落结构操作一起做）与 `RES-04` 的 toggle 校准（5.8）。
- **表格**：模型、`resolve` 视图、compat 投影、单元格内编辑与行列操作都在（3.1–3.9）。还没有的：
  表格的修订生成（`tblPrChange` / `trPr/ins` 等，M7）、整表再生成的原生等价物（M7）。
- **绘图**：读侧完整（显示模型 + 投影 + 门）；写侧有新图片（随文 / 锚定）、`xml.replaceImage` 与保存前的
  孤儿回收（6.7），还没有的是 `applyImageZOrder` 的 `relativeHeight` 回写（M7）。另外几块按分层留给后面：页眉页脚里的图片（M5）、
  墨迹读写都在（6.8：`inks[]` + `SaveOptions.inks`）。图表（6.1 / 6.2）与 SmartArt / 画布（6.3）的模型与投影都在：92 份图表语料的
  `chartDisplay` / `extras.chartParts` / `previewText`、23 份图示语料的 `previewText` / `diagramDisplay` / `textboxes`
  与 TS 无差异，chartex 回退图成图片块，画布按子坐标系缩放并做 LO 对齐的分栏。外部文本框 part 与单元格里的
  锚定形状与图片都已可用。
- **保存选项**：已支持 `savedAt` / `removePersonalInfo` / `removeDateAndTime` / `comments` / `footnotes` /
  `endnotes`、节、页眉页脚、水印、页面颜色、页码、编号、样式 upsert、保护、主题（M5）、`partXml` / `partBinary`
  与 `kind:"chart"`（6.6）、`kind:"image"` / `replaceImage`（6.7）、`inks`（6.8）——TS `SaveOptions` 已全部覆盖。
- **修订生成**：`EditContext.track_changes` 字段存在但仍被忽略（M7 7.2 / 7.3）。**读侧的索引已经有了**
  （7.1：`Document.revisions`，全包一张 `RevisionIndex`，24 种 `RevKind`、宿主、嵌套层数、搬移配对、
  会话内稳定的 `RevisionId`、`EDIT-06` 的全局 `w:id` 最大值），接受 / 拒绝（7.4）还没有。

## 如何验证

```sh
cargo fmt --all --check && cargo clippy --workspace --all-targets   # 零告警
cargo test --workspace && cargo test --workspace --release          # 默认 feature
cargo clippy --workspace --all-targets --features compat-ts
cargo test --workspace --features compat-ts
cargo test --workspace --release --features compat-ts
tools/ci/check-native-default.sh                                   # 隔离下游生命周期 + 编译拒绝探针
cargo test -p rsword --test model_snapshot                          # 自快照 + 全语料无编辑原字节
RSWORD_RANDOM_SEQUENCES=1000 cargo test -p rsword --test random_ops -- --nocapture
RSWORD_RANDOM_SEQUENCES=1000 cargo test -p rsword --release --test random_ops -- --nocapture
# fuzz_bind 使用临时 corpus，避免把 libFuzzer 自动扩充的种子写回工作区，见 fuzz/README.md
cargo run -p diff-parse --features compat-ts -- --scope text                             # M1 门第一条：0 未知差异
cargo run -p diff-parse --features compat-ts -- --scope fields                           # M2 门：字段与 Span 域 0 未知差异
cargo run -p diff-parse --features compat-ts -- --scope tables                           # M3 门：表格域 0 未知差异
cargo run -p diff-parse --features compat-ts -- --scope drawing                          # M4 门：绘图域路径 0 未知差异
cargo run -p diff-parse --features compat-ts -- --scope hf                               # M5 门：页眉页脚域路径 0 未知差异
cargo run -p diff-parse --features compat-ts -- --scope embedded                         # M6 门：嵌入对象域 0 未知差异
cargo run -p diff-parse --features compat-ts -- --scope all                              # 第八道门（M6 6.9）：全域 0 未知差异
cargo run -p diff-parse --features compat-ts -- --corpus corpus/real                     # 真实 Word 语料（266 份）：0 未知差异
tools/build-js.sh --features compat-ts && cargo run -p diff-parse --features compat-ts -- --via-js --scope all # 绑定等价门：真实 wasm 产物与原生逐字节相同（需 node ≥ 22，见 TOOLS.md）
cargo test -p rsword --test real_edits -- --ignored                 # 重新生成给 Word 验收的编辑后文档
cd fuzz && cargo +nightly fuzz run fuzz_embedded -- -max_total_time=600 # M6 门第 4 条：图表 / 图示 / OMML / 画布解析无崩溃
cd fuzz && cargo +nightly fuzz run fuzz_instr -- -max_total_time=600 # M2 门：指令 tokenizer 无崩溃
cargo test -p rsword --test edit                                    # M1 门第二条：其他条目 CRC 不变
cargo test -p rsword --test save_validate                           # M1 门第三条：Strict 改字仍 Strict
cargo test -p rsword --test save_blocks -- --nocapture              # 保存差分明细与跳过原因
```

## 债务与风险

- `SPAN-10` 只做了插入侧（插入点落在字段原子旁边时取原子边界），范围**端点**落进指令区时还没有移到
  原子边界；等有真实用例再补。
- 语料导出自 genoffice `f105f36` **加 32 个脏文件**（`manifest.jsonl` 首行有记录）。已复核并接受：脏文件里只有 `src/generate.ts`（改动集中在 `patchTableCellTexts`）与 `tests/nested-table-edit.test.ts` 属于 `docx-engine`，`parseDocx` 未被改动，所以 573 份 `.expected.json` 等价于干净基线；`nested-table-edit` 的两份保存用例走 `kind:'xml'` 原样拼接，对 `generate.ts` 不敏感。genoffice 侧再改 `docx-engine` 时需要重导。
- ~~`TEST-04` 的全语料 L4 编辑保真~~ 已补（`tests/save.rs::test_04_corpus_edit_fidelity`，400+ 份文档）：每份做一次 `InsertText`，断言投影文本正确、其他 zip 条目的 CRC 与压缩字节不变、重解析后 `compat_ts` 的块投影**只有目标块变了**——规范里"重解析后其他段落模型相等"这条 oracle 现在有了。M0 留下的 L1 `set_text` 全量扫描继续保留（它覆盖到不是正文顶层段落的文档）。
- toggle 属性（bold / italic 等的层叠语义）：**已按 2026-09-06 的 Word 实测校准，且规则按字段分**——
  `b` / `i` 走 `有效值 = docDefaults ⊕ 段落样式层 ⊕ 表格样式层 ⊕ 字符样式层`，`strike` / `caps` /
  `smallCaps` / `dstrike` 实测**不抵消**，走"最具体的声明胜出"；层级内部的 `basedOn` 链两者都是
  普通的"子覆盖父"。这条规则只作用于 `resolve` 公开视图，`compat_ts` 的 `runs[].bold`
  发的是声明值（复现 TS 形态），所以差分门不受影响。**还没定死的那一块**（`strike` 一族只在
  Word 网页版测过，与 ECMA-376 §17.7.3 字面冲突）连同下一步怎么测，写在
  `docs/06-toggle-open-question.md`；不挡进度的依据也在那里（语料 + 真实文档 24,177 个 run 里
  撞上歧义的是 0 个）。
- 语料在 Span / 字段这两个域上很薄：573 份里只有 15 份带范围标记（31 个标记、19 个范围）、43 份带字段
  （57 个）。M2 的行为正确性主要靠 `tests/{span,field,notes,para_ops}.rs` 的 78 个单元用例，不能只看
  差分数字。
- 绘图域的门按**路径**筛（`is_drawing_path`），不是按文档：绘图文档同时背着 M2/M3/M5/M6 的差异，
  按文档筛这道门永远关不上。代价是「域的边界」写在代码里，加新字段时要同步——`diff.rs` 里那条
  单测钉着边界（表格里的图归 M3、页眉页脚的归 M5、图表公式归 M6）。
- 绘图语料同样薄：125 个 `w:drawing` 与 43 个 `w:pict` 集中在几十份合成文档里，真实 Word 文档的
  组嵌套与画布布局比这复杂得多。恶意输入那 4 份（`corpus/hostile/drawing-*`）覆盖的是降级路径，
  不是排版正确性。
- `compat_ts`（17 文件 12,782 行）**不再判为待删的负担**（2026-09-08 改判）：genoffice 退为测试基准之后，
  它是 1,065 份文档差分的对接点、目前最强的正确性证据。M8′ 8.7 把它降为 `#[cfg(feature = "compat-ts")]` 的
  测试专用件——默认构建不含、不进公共 API、不承诺稳定，但**保留**。见 `spec/19` 决策 2。
- **公共 API 从未设计过**：`lib.rs` 把 11 个模块全部 `pub` 出去，758 个 `pub fn`、318 个公共类型，没有
  `missing_docs` 约束、没有稳定性承诺。独立交付之前这是最大的一块债，M8′ 8.5 处理。
