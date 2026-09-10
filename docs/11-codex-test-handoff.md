# rsWordParser 全面测试交接：第二台 Mac / Codex + computer use

第二轮执行入口（2026-09-10）：[第二轮语义与公开 API 验收任务](13-round2-semantic-e2e-test-tasks.md)。第一轮已结束；下一轮请以该文件的基线、任务顺序和验收标准为准，本文保留作历史设计依据。

增量任务：用户进一步要求使用 computer use 在 Word 中亲手创建尽可能复杂的真实文档。正在执行本交接的代理请继续现有任务，并接入 [Word 复杂语料补充交接](12-word-authored-complex-corpus-handoff.md)，不必重启或重复已完成测试。

交接日期：2026-09-09。本文是可直接交给新代理的任务说明。使用中文汇报。

## 1. 任务与授权边界

用户希望在另一台 Mac 上全面测试 rsWordParser，使用 Codex 编写、执行自动化测试，使用 computer use 操作 Word 等桌面应用完成实际兼容性验收。用户明确表示不限制耗时、轮数和 token，并要求比常规测试更深入的方案。

请实施测试系统并执行验证，不要只重复输出测试建议。以获得可复现的正确性证据为目标。不存在通过有限测试证明“所有文档绝无错误”的结论。

可以在独立测试副本或分支中新增测试、工具、夹具和报告。已有未提交代码属于用户工作，不得覆盖或清除。编译修复与功能修复必须独立记录，保存原始基线及回归样例，再验证修复版本；不得把修复后结果冒充原版本结果。若有并发修改，先冻结测试快照，避免测到漂移版本。

不自动 push、合并、发布或向别人发送消息。Word 中仅操作测试副本；不覆盖原始语料。账号登录、付费软件安装、隐私文档上传若缺少必要条件，应明确记录并请求相应输入。不要上传文档到在线转换网站。

Codex 的具体版本、工具接口及第二台 Mac 的应用状态尚未核实。先发现本机实际能力，不能假定已有 CLI、浏览器编辑器、Word 自动化接口或 genoffice 集成。

## 2. 项目及权威资料

这是 Rust DOCX 解析与局部补丁编辑内核，不负责布局、渲染。关键承诺是“未编辑内容保持原样”。先读目标机器仓库中的 AGENTS.md，再阅读以下资料；本文不重复其完整规格：

- README.md：项目入口和构建方法。
- docs/03-architecture-v3.md：冻结架构。
- docs/04-dev-plan.md、spec/12-m0-m1-plan.md：实现阶段和范围。
- spec/00-overview.md：术语、规范 ID、单位。
- spec/08-edit.md、spec/09-save.md、spec/11-testing.md：编辑、保存、测试契约。
- spec/05-properties.md、spec/06-model.md、spec/07-resolve.md：属性、模型、解析规则。
- spec/10-compat-ts.md：TS 兼容范围。
- crates/rsword/tests/、crates/rsword/src/edit/tests.rs：现有测试。
- corpus/README.md、corpus/real/README.md、fixtures/resolve/README.md：语料要求。
- tools/export-golden/README.md：TS golden 导出流程。
- .github/workflows/ci.yml、.github/workflows/fuzz.yml、fuzz/README.md：既有执行门。

设计文档与当前代码可能不同步。逐项标记已实现、部分实现、未来能力，不能把未来能力计作已通过。

## 3. 本次会话实际确认的状态

原机器工作目录：/Users/lilleap/code/rsWordParser。目标机器自行确定路径。

检查时 HEAD：4b8032a066ee14d4932f449041f909f0e55b43c2。

`git branch --show-current` 没有输出，接手时核实是否 detached HEAD。

已修改、未提交的文件：

```text
crates/rsword/src/edit/locate.rs
crates/rsword/src/edit/mod.rs
crates/rsword/src/edit/plan.rs
crates/rsword/src/model/build.rs
crates/rsword/src/xml/plan.rs
```

未跟踪内容：

```text
crates/rsword/src/edit/inline_gen.rs
crates/rsword/src/edit/ops.rs
proposal/
```

proposal/ 包含用户方案文档，不是测试必需输入，不默认迁移或使用。

只 clone 上述提交不能得到当前受测实现。必须迁移这五个修改文件和两个新 Rust 文件，或取得包含它们的后续完整版本。普通 git diff 不包含未跟踪文件。接收完整源代码快照、Cargo.lock、语料及必要夹具后，对比文件清单和 SHA-256；原始快照只读留存。本文没有附带源码快照。

本会话运行过 `cargo test --workspace`，编译失败，测试未执行。确认的三个错误：

1. crates/rsword/src/edit/locate.rs:235：对 Option<&Inline> 使用 Inline 模式。
2. crates/rsword/src/edit/ops.rs:76：Option<QName> 与 LocalName 模式不匹配。
3. crates/rsword/src/edit/inline_gen.rs:51：with_text 消费 t 后继续使用 t。

行号仅对当时快照有效。另有 unused_mut 警告。不能据此断言这是全部错误；修复后可能出现更多编译或测试失败。

当前代码可见五种编辑操作：InsertText、DeleteRange、SetRunProps、SetParaProps、ReplaceInlines。部分 run 属性编辑、跨段操作等有阶段性限制，接手后按规范核实具体契约。现有单测仍包含 DeleteRange 应返回 Unsupported 的旧断言，需判断是否已过期，不可为了变绿盲改断言。

现有测试覆盖无编辑字节往返、部分文本 TS 差分及底层 XML 修改。全合成语料的单节点修改测试主要调用底层 set_text，不等于五种公开 EditOp 已通过全语料测试。当前可见 fuzz 目标只有 fuzz_xml、fuzz_zip。README 中历史通过记录不是当前快照的测试结果。

## 4. 第一阶段：冻结版本与环境基线

建立独立受测目录和 evidence/ 目录。记录 UTC 时间、源快照哈希、HEAD、dirty diff、未跟踪文件清单及哈希、Cargo.lock 哈希、操作系统、CPU 架构、内存、磁盘余量、Rust 工具链、Codex 版本和可用工具、Word/LibreOffice 版本及构建号。

本地路径、登录信息和文档作者等信息可能敏感，分享报告前脱敏；不要记录令牌或完整环境变量。

依次执行并保留完整日志与退出码：

```sh
rustc --version
cargo --version
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace --locked
```

这些命令用于目标仓库根目录。Cargo.toml 声明最低 Rust 1.88；选择兼容工具链并记录实际版本。CI 设置了 RUSTFLAGS=-D warnings，另做一次同配置检查。不要用会丢失退出码的日志管道。

先记录原始编译失败，再在测试分支做最小编译修复，输出独立补丁及理由。基线一旦可运行，保存测试列表、通过数、失败数、忽略数及日志。缺工具时补齐可补齐的本地依赖；Word 未就绪时继续独立自动化工作。

## 5. 第二阶段：实现独立的自动化判据

优先从当前五种操作及其支持范围开始，逐渐扩展到完整规范。

### 5.1 规范追踪与结果分类

建立功能矩阵：规范 ID、实现范围、前提、正例、反例、边界例、独立判据、执行状态、证据路径。

状态至少包含 PASS、FAIL、BLOCKED、NOT_IMPLEMENTED、SKIPPED_WITH_REASON。预期拒绝必须校验错误类别和状态不变。任意 Err 不能都算成功；没有打开的文档不能静默跳过。已知差异要逐项统计，绑定原因和适用版本。

### 5.2 独立参考模型

用简单数据结构表示段落、Unicode 字符、字符格式、段落属性和原子元素。不调用生产代码的定位、编辑、XML codec 或 resolve 计算预期值。坐标按项目 UTF-16 契约处理，属性区分未指定、显式开启、显式关闭和清除。

对相同编辑操作比较：可见文本、UTF-16 长度、逐字符或规范化区间格式、原子元素位置、返回偏移变化、非目标段落。不同 run 拆分可语义等价，不能直接比较内部 NodeId。

先用手算的小例子校准参考模型。其支持范围必须明确；复杂字段/表格等未建模部分使用单独判据，不能删去后宣称整份文档等价。

### 5.3 小文档穷举和结构变体

枚举短字符串全部合法插入位置、全部删除区间及非法 UTF-16 边界。覆盖中文、emoji、组合字符、XML 特殊字符、首尾空白、Tab、换行、空段落、多 run、多文本段、超链接和不透明节点。

对同一语义生成不同合法 XML：run 拆分、命名空间前缀替换、属性顺序、等价属性表达、允许的包装结构。为变换写明等价前提。覆盖 Transitional、Strict、混合 part。

从 1～2 段、0～3 run、短字符集开始枚举 1～4 步操作组合。记录搜索空间和实际访问状态，设定可恢复的分片机制，不能让组合爆炸阻断其他测试。

### 5.4 有状态随机序列

每个种子同时维护参考模型与真实会话。策略包括合法深入编辑、边界攻击、刚编辑区域反复操作和错误后的继续操作。

每步检查模型与预期、返回 delta、非目标内容、当前投影与完整 rebuild。在接口适用时验证 Span/Field 不变式；不要调用尚未实现的未来接口。

把 save/reopen 也作为动作：每步保存、随机间隔保存、长时间不保存、连续保存、重复重开。重开后按测试稳定标识重新映射段落，不复用旧 NodeId。随机失败必须记录预期成功/拒绝及前提。

先执行 1,000 种子 × 100 步，再按成本扩展到 10,000 种子 × 最多 1,000 步。规模是计划，不是验收结果。优先增加新状态和新结构，不单纯重复相同路径。

### 5.5 变形关系

- 插入再删除刚插入的范围：满足前提时恢复语义，不要求恢复原 XML 字节。
- 编辑两个独立段落，交换顺序：语义一致。
- 相同属性重复设置：语义幂等。
- 操作序列插入保存重开：最终语义一致。
- 等价 run 拆分后编辑：语义一致。
- 添加无关不透明 part：正文结果不变，不透明内容保留。

### 5.6 字节保真与独立结构验证

使用另一套 XML 解析实现验证输出良构、命名空间、相关属性及关系。可用本机可安装的独立库或工具，记录版本；不能只用 rsword 自身重解析。

测试侧独立定义操作允许变化范围，不以生产 dirty_nodes 为唯一依据。无编辑时比较整个文件；有编辑时检查未涉及 ZIP 条目的压缩数据及规范承诺保留的元数据，检查未修改 XML 片段的对应位置、顺序、出现次数和内容。

仅验证“Clean 片段仍为输出子串”不能排除重复、错位、误删。相同文本的重复节点应加入专门反例。修改后的文件不要求 ZIP CRC 必须不同，因为 CRC 不是无碰撞判据。

### 5.7 差分、故障注入、变异测试

按 tools/export-golden/README.md 准备匹配版本的 genoffice。固定 TS 版本和 golden，解析差分和编辑后差分分别报告。TS 是兼容判据，不是所有 OOXML 行为的最终真理。

在实际可失败阶段设置测试专用故障点，验证失败前后 DOM、模型、dirty 状态、可观察诊断及保存输出的契约，并继续执行合法操作。不要依靠不可控的真实 OOM 测试回滚；资源压力放在受限子进程。

定向变异至少覆盖 UTF-16 偏移、属性清除、邻接节点误删、失败残留和未编辑条目重写。记录被测试捕获的变异及未捕获原因，不把等价变异当缺陷。

## 6. 第三阶段：fuzz 与资源行为

先复用 .github/workflows/fuzz.yml 的 fuzz_zip/fuzz_xml 配置。在 fuzz/ 目录运行，例如：

```sh
cargo +nightly fuzz run fuzz_xml -- -max_total_time=600 -timeout=20 -rss_limit_mb=4096
cargo +nightly fuzz run fuzz_zip -- -max_total_time=600 -timeout=20 -rss_limit_mb=4096
```

前提是 nightly 和 cargo-fuzz 已安装。种子来源和版本留档；先 smoke，再按 CPU 小时扩展。构建 fuzz_edit 时使用有效 DOCX + 结构化操作流，避免绝大部分输入都在 ZIP 头部被拒绝。保存和重开动作纳入流。

恶意输入放独立进程，设置时间、内存、磁盘限制。记录 panic、超时、资源拒绝和慢例；超时需复跑定位，不能自动算崩溃。合法大文档测解析、编辑、保存、重开时间和峰值内存随规模的变化，固定硬件和重复测量条件，避免与 GUI 验收并发争抢资源。

## 7. 第四阶段：computer use / Word 验收

自动化内核测试使用代码与 CLI；computer use 用于真实桌面行为。没有实际编辑器集成时，不能声称在界面里测试过 rsword：先由 Rust 测试驱动生成编辑产物，再让 Word 打开这些产物。若要测试 genoffice 端到端，必须证明该应用实际调用本次受测内核，并保留版本与调用证据。

准备一个小型测试驱动：输入 DOCX、声明式操作序列和输出目录，输出保存文件、操作回执、模型摘要及诊断。路径、坐标、操作 schema 由实际 API 确定；这是待实现工具，仓库目前未确认存在对应 CLI。

GUI 用例表至少包含：输入来源、预期修改、指定操作序列、原始文件、内核输出、Word 再保存副本、预期观察项和自动断言结果。

每个用例执行：

1. 对原始 DOCX 做哈希并以测试副本打开，记录 Word 版本、页面数、相关段落/格式和必要截图，关闭原始副本。
2. 用受测驱动执行操作，保存完整回执和输出哈希；先完成内核输出的字节/结构检查。
3. 用 Word 打开内核输出，核实当前窗口文件身份，记录所有修复、损坏、兼容提示。普通只读或受保护提示与文件损坏分开归类。
4. 定位目标段落，验证文字、格式、空白、Tab/换行和非目标内容。截图包含足够上下文，并关联自动化证据。
5. 对长文档按用例检查页眉页脚、列表编号、表格、图片、字段等受影响或需要保持的区域。无法观察的项目标记未验证，不猜测。
6. 若测试 Word 往返，另存到独立 word-resaved 路径，重开验证，再用内核解析检查语义。Word 自己重写文件后，不能再用该文件判定内核原始字节保真。
7. 关闭文档，处理保存对话框时确认文件名和目标目录，绝不覆盖原件。

使用实际工具暴露的 UI 状态/截图定位控件，不猜测坐标，不依赖未经核实的快捷键。Word 弹窗必须留证；不得批量点“修复”后算通过。修复后可另存用于诊断，但该用例仍记录原始异常。

推荐 GUI 场景：中文商务文档、混合字体、emoji、首尾空白、列表、跨页段落、表格图片、页眉页脚、Strict、未知扩展，以及当前内核明确不支持但应原样保留的内容。用户未明确提供测试授权的私有文档不自动选取。外部语料记录来源和许可。

视觉回归优先在同一台 Mac、同一 Word 版本、同一字体环境比较原始与输出。必要时分别导出 PDF 做页面定位和差异辅助；截图差异只是线索，文字编辑引起重排可能完全合理。缺字体或环境变化独立归类。LibreOffice 可补充证据，不能替代 Word 并报告为 Word 通过。

## 8. 失败产物、缩减与修复纪律

建议产物布局：

```text
evidence/<run-id>/
  environment.json
  source-manifest.json
  commands.jsonl
  capability-matrix.csv
  results.jsonl
  logs/
  gui/<case-id>/
  failures/<failure-id>/
    input.docx
    operations.json
    before.docx
    after.docx
    expected.json
    actual.json
    diagnostics.json
    reproduce.md
  report.md
```

建议每条结果字段：run_id、case_id、spec_ids、source_hash、seed、operation_index、oracle、status、duration、artifact_paths。种子不能替代操作序列；生成器版本变化后种子可能不再重现。

自动缩减按删除操作、减少 part/段落/run、缩短文本、简化属性和 XML 的顺序进行。保持同类失败和输入合法性前提。原始失败样本永远保留，缩减样本另存。每个确认缺陷给出最小复现、预期、实际、影响范围、怀疑位置及证据；修复后验证原始样例、最小样例和关联测试，再继续未完成探索。

## 9. 完成条件与持续运行

不设武断的“跑满时间就通过”。完整交付至少需要：

- 原始基线及修复版本分开留档；构建和必要检查状态明确。
- 当前受支持功能都有正例、反例、边界例和多轮序列证据。
- 可打开与应拒绝语料逐例统计，无静默丢弃。
- 支持范围内不存在未解释的语义、保真、回滚失败。
- 所有发现都有可复现产物，修复项有回归验证。
- Word 验收已执行，或明确列出因应用/账号/环境缺失而未执行的项目。
- 报告给出实际种子、操作步数、CPU 时间、覆盖缺口和判据冲突，不宣称绝对正确。

长任务分批运行并检查点落盘。维护 progress.md：已完成批次、正在运行的命令/进程、当前失败、下一个独立任务、恢复命令。对外定期简短汇报新增证据和剩余风险。不要因缺 Word 而停掉所有 CLI 工作，也不要因 CLI 通过而结束 GUI 验收。

## 10. Suggested skills

仅在目标环境确实可用时读取相应技能；技能路径可能不同。

- diagnosing-bugs：定位编译失败、语义错误、保存回归与性能异常。
- tdd：将最小失败样例变成修复前失败、修复后通过的测试。
- code-review / review：检查独立修复补丁及其规范符合性，遵循目标环境技能要求。
- pdf：需要导出 PDF 并做页面级检查时使用。
- documents：需要制作特定 DOCX 测试文档时使用，不用于替代原始保真语料。
- handoff：会话续接时更新进度与复现入口。

遵循 computer use 实际工具文档。无需为了使用技能扩展无关任务。若技能要求外部写入或审批，结合已有授权与具体动作判断，不能杜撰许可要求。

## 11. 接手后的第一轮动作

先核验源快照是否包含未提交编辑实现，读取仓库指令，落盘环境证据并重现编译基线。随后隔离最小编译修复，运行既有测试，建立功能矩阵；优先实现五种操作的独立参考模型、小文档边界穷举、保存重开序列和保真检查。同时核实 Word 与 computer use 是否可用。第一轮汇报应包含实际基线结果、受测版本指纹、已发现缺陷、测试工具落地情况及接下来的执行批次。
