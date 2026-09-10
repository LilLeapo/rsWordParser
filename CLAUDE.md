# CLAUDE.md

给在这个仓库里工作的 Claude Code 会话的说明。人类同样可读。**当前进度与能力边界看 `docs/05-status.md`**，不要凭印象假设某个功能已经存在。

## 这是什么

`rsword`：**独立的高保真 DOCX 读写内核**（Rust）。解析 `.docx` 为文档模型，接受 `EditOp` 编辑操作，以**字节级局部补丁**写回。不做布局与渲染。

交付 **Rust crate 优先**，wasm / CLI 是它的绑定。目标形态是 Word / WPS 的**外挂应用**（文件级工具：CLI + MCP server），对 docx 阅读与修改，后续接入 Agent 读改内容——所以**专注 parser，不碰渲染**。

> **范围改定（2026-09-08）**：原目标是「替换 genoffice `packages/docx-engine` 的 `parseDocx` / `saveDocx`」，已撤销。
> genoffice 现在**只是测试基准**：只读地跑它的 TS 引擎生成 `corpus/**/*.expected.json`，不改它的任何代码。
> 详见 `docs/03` v3.3 首页的修订说明与 `docs/04` §17。

一句话原则：**文件是真相。** 未编辑的内容一个字节都不动；编辑只发生在被标脏的 XML 节点上。

## 权威顺序

1. `docs/03-architecture-v3.md` —— v3.3 **冻结架构**，是宪法。分层、六个核心类型、不变式在这里定（v3.3 只改范围与里程碑，这三样一字未动）。
2. `spec/*.md` —— 可验收的模块规范，每条带 ID（`XML-12`、`PROP-06`、`EDIT-03`…）。实现与测试都引用这些 ID。规范服从设计；冲突时以设计为准并修订规范。
3. `docs/04-dev-plan.md` —— 执行计划：§5.1 M1 已完成清单、§5.2 M1 门、§8 实现偏差、§9 待决、§10 M2 及以后的排期、§11–§16 M2–M7 逐条进度、**§17 范围改定与 M8′ 逐条进度**、§18 M9′ 逐条进度。
4. `docs/01-ts-parser-reference.md` 与 genoffice 源码 —— **差分基准，不是验收权威**（见下）。

`spec/12`–`spec/20` 是里程碑任务分解（# / 任务 / 规范 / DoD）。**`spec/19` 与 `spec/20` 已于 2026-09-08 整体重写**：原 M8（编辑器切换）撤销，原 M9 的 rsword 半边前移为 **M8′**（原生协议与独立交付）、genoffice 半边撤销，新 **M9′** 是 Agent 接口层与文件级工具。

**M0–M7 已全部并入 `main`**（`main` = 32234ce，2026-09-08）。**M7**（`spec/18`：修订生成与接受 / 拒绝、`EditOp` 全集 60 个变体、分节符、绘图编辑、块字段生成器、空白模板、`TEST-07` 随机序列门、wasm 绑定）**11 个任务与六条门全部完成**：646 测试、九道 `diff-parse` 门 + 两条 `--via js` 全为 0 未知差异、`save_blocks` 204/208、1,000 条随机序列双构建无失败；`TEST-07` 一道门查出并修掉 17 个引擎缺陷。逐条进度见 `docs/04` §16。

**当前：M8′ 实现收尾，门 2 / 门 4 待裁定**（`spec/19`，分支 `m8-native-json`，工作树 `../rsWordParser-m8j`）——原生协议 `spec/21-bind.md`、模型 JSON 投影、`EditOp` JSON、有状态会话与媒体句柄、**Rust crate 公共 API 定型**、`*.model.json` 自快照回归网、`compat_ts` 降为测试专用 feature。逐条进度记 `docs/04` §17。

## TS 不是权威

genoffice 的 TS 引擎是**测试基准**（2026-09-08 起也只是测试基准）。目标是**功能等价或更强**，差分测试只是发现回归的手段，不是目标。

用到 genoffice 的地方只有一处，而且是**只读**的：`tools/export-golden/run.sh` 跑它的 TS 引擎生成 `corpus/**/*.expected.json`。**不改它的任何代码**，不在它那边建分支。

- TS 的缺陷不跟随。已知例：修订 `w:id` 我们按 `EDIT-06` 取全局最大值 +1，TS 写固定 `0` / `9001`（重复插入会重号）。
- 每一处有意的不同都必须登记，否则差分测试会把它当回归：
  - 解析侧 → `crates/rsword/src/bind/compat_ts/KNOWN_DIFFS.md` 的 ```known-diffs 块（`<文档 glob> <路径 glob>`，`include_str!` 编进库）。
  - 保存侧 → `crates/rsword/tests/save_blocks.rs` 的 `INTENTIONAL` 表。
  - 语义层 → `docs/04` §8 的偏差表。
- **禁止**为了让测试通过去改 `corpus/**/*.expected.json` 或 `*.save.*.json`。语料是 TS 的输出记录，只能由 `tools/export-golden/run.sh` 重新生成。

## 四条不可协商的不变式

1. 无编辑保存 → 输出与输入**字节相同**。
2. 编辑一个段落 → 其他 zip 条目的 CRC 与压缩字节相同；`document.xml` 里其他块的原字节原样出现。
3. 规范状态 = DOM + Span（`xml` + `span`）。`model` 与 `resolve` 是可重建的投影，**禁止**把投影当真相写回。
4. 病态输入（3000 层嵌套等）局部降级为 `Protected`，不 panic、不丢字节。

`Dirty` 传播规则（`XML-12`）：非 `Clean` 节点的祖先不为 `Clean`；`Clean` 节点的后代全为 `Clean`。改 DOM 只能走 `xml::edit` 的原语或 `NodeEdit`，它们保证这条成立。

## 布局

| 路径 | 内容 |
| --- | --- |
| `crates/rsword-cli/` | 文件级 rsword CLI，复用 tools/agent-query 的工具表、预算、游标与编辑事务 |
| `crates/rsword-mcp/` | 原生 stdio MCP，会话与共享工具表复用，连接/计费/待追认边界见 docs/19 |
| `crates/rsword/src/package/` | L0 包层（zip、`[Content_Types].xml`、`.rels`、flavor） |
| `crates/rsword/src/xml/` | L1 无损 DOM（tokenizer、`Dirty`、MCE、命名空间、`plan`、`fragment`、`canon`、`xpath`） |
| `crates/rsword/src/span/` | L2 范围与字段（`content` / `index` / `transform` / `materialize` / `field`） |
| `crates/rsword/src/semantic/props/` | L3 属性表。**生成代码**：`schema/props/*.toml` + `build/props.rs` → `$OUT_DIR/props.rs` |
| `crates/rsword/src/model/` | L3 文档模型投影（`Document::rebuild`、块分类、坐标流）。**2026-09-10 已扁平化为单个 `mod.rs`**：`docs/04` 与 `spec/14`–`19` 里的 `model/<名字>.rs` 是当时的文件，现指 `mod.rs` 内对应区段 |
| `crates/rsword/src/resolve/` | 有效属性只读视图（样式链、主题字体 / 颜色） |
| `crates/rsword/src/edit/` | L4 编辑引擎（`EditSession`、`InlinePos`、`MutationPlan`、操作） |
| `crates/rsword/src/save/` | 校验、序列化、包写回、保存选项 |
| `crates/rsword/src/bind/native/` | 唯一对外协议：模型/操作 JSON、会话、媒体与批量查询 |
| `crates/rsword/src/bind/compat_ts/` | 兼容适配器：`ParsedDoc` JSON、`SaveBlock[]` 映射、差分。**测试专用**（已挂 `compat-ts` feature，默认关），不是对外接口 |
| `crates/rsword/src/bind/js.rs`、`crates/rsword-js/` | 语言中立的绑定核心 + wasm-bindgen 外壳（7.10；M8′ 8.4 改为有状态会话） |
| `crates/rsword/schema/` | `local_names.txt`（名字表）、`props/*.toml`（属性表） |
| `tools/diff-parse`、`tools/xpath-assert`、`tools/gen-fixtures` | 差分、XPath 断言、`fixtures/resolve` 生成（workspace 成员） |
| `fixtures/resolve` | `RES-12` 校准 fixture：文档我们生成，**观察值来自真实 Word**（见那里的 README） |
| `fixtures/revisions` | M7 门第 3 条：四个 case 的 `base` / `tracked` / `accepted` / `rejected`，**四态全由桌面 Word 另存** |
| `fixtures/word-ops` | Word 自己做分节符增删 / z-order / 移动缩放的 `before` / `after`，7.6 / 7.7 的对照件 |
| `corpus/synthetic` | 799 份 docx + `*.expected.json`（TS `ParsedDoc`）+ 208 份 `*.save.<k>.json`（`SaveBlock[]` + 期望 `documentXml`）；`m6-*` 是 M6 的嵌入对象语料 |
| `corpus/hostile` | 38 份恶意 / 畸形输入（`TEST-09`） |
| `fuzz/` | `fuzz_xml`、`fuzz_zip`（不在 workspace 内） |

## 命令

```sh
cargo fmt --all
cargo test -p rsword-cli                    # CLI 真实进程 E2E，CI 同跑 macOS/Linux
cargo test -p rsword-mcp                    # MCP 真实进程、生命周期与独立堆泄漏检查
cargo build -p rsword-cli -p rsword-mcp
node tools/ci/check-agent-transports.mjs target/debug/rsword target/debug/rsword-mcp # 跨传输业务等价及游标双向拒绝
cargo clippy --workspace --all-targets
cargo clippy --workspace --all-targets --features compat-ts      # 必须零告警
cargo test --workspace                      # 默认：原生协议/引擎测试
cargo test --workspace --features compat-ts  # 另加兼容差分测试
cargo test --workspace --release
cargo test --workspace --release --features compat-ts # 必须也跑：enforce 只在调试构建报错，发布构建行为不同
cargo run -p diff-parse --features compat-ts -- --scope text     # M1 门：文本用例未知差异必须为 0
cargo run -p diff-parse --features compat-ts -- --scope fields   # M2 门：再加字段 / 范围 / 批注，仍须为 0
cargo run -p diff-parse --features compat-ts -- --scope tables   # M3 门：再加含表格的文档（按文档筛），仍须为 0
cargo run -p diff-parse --features compat-ts -- --scope drawing  # M4 门：绘图域**路径**（不是按文档筛），仍须为 0
cargo run -p diff-parse --features compat-ts -- --scope hf       # M5 门：页眉页脚域**路径**，仍须为 0
cargo run -p diff-parse --features compat-ts -- --scope embedded # M6 门（进行中）：嵌入对象域（路径 + 期望块 label），目标 0
cargo run -p diff-parse --features compat-ts -- --scope all --json          # 全域差距排名
cargo run -p xpath-assert -- a.docx '//w:p[1]/w:r/w:t/text()'
cd fuzz && cargo +nightly fuzz run fuzz_xml -- -max_total_time=600      # 另有 fuzz_zip / fuzz_instr
GENOFFICE_DIR=~/code/genoffice tools/export-golden/run.sh   # 重导语料（改期望值的唯一合法途径；重导后按 tools/export-golden/README「重导的稳定性与噪音」还原噪音）
tools/export-golden/try.sh <name>.export.test.ts             # 开发新的导出用例文件：只跑它，产物进临时目录，不碰 corpus/
```

## 工作约定

- **一个任务一个提交**：`m<里程碑>.<任务>: 英文摘要 (SPEC-ID…)`，正文说清做了什么与为什么，结尾带 `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`。
- 提交前同步文档：`docs/04` §5.1 勾选清单、§8 偏差表（若有偏差）、`docs/05-status.md` 的数字、必要时 `README.md`。
- 测试函数名引用规范 ID：`xml_12_dirty_propagation`、`edit_03_insert_text_*`、`save_07_*`。集成测试放 `crates/rsword/tests/<域>.rs`，语料发现用 `tests/common`。
- 注释与文档用中文，标识符与提交信息用英文。模块头注明对应的 spec 文件与条目。
- 先确认你在哪个工作树：`git worktree list`。当前开发分支见 `docs/05-status.md`。

## 改代码时的硬规则

- **属性容器（`w:rPr` / `w:pPr` / `w:settings` …）只能通过生成的 `plan_apply_*` 改**。手写 `append_child` 会违反 `PROP-05` 子元素顺序，调试构建下 `save` 会直接报错。
- 新的元素名 / 属性名要先加进 `crates/rsword/schema/local_names.txt`（生成器会造 `LocalName` 变体）。表外名字会 intern 成 `LocalName::Other`，可用但不能用于常量匹配。
- 树遍历写成**迭代**的。语料里有几千层嵌套的文档，递归会栈溢出，表现为测试 SIGABRT。
- 编辑操作的事务边界是 `EditSession::apply` / `apply_all`：`plan`（只读）→ `validate`（只读）→ `commit`（机械写入，不可失败）。任一步 `Err` 必须不留半修改状态。外层事务保存完整 `EditSession` 检查点，失败同时恢复 DOM、包部件/关系、投影/诊断与 id 分配状态（8.6，`EDIT-05`）。
- **同一形状重复三次以上就上声明宏**。已有的：`bind/compat_ts/json.rs` 的 `set_some!`（有值才写）与 `set_if!`（为真才写 `true`）——投影层新写字段用它们，别再手写 `if let Some`；`model/mod.rs` 的 `named_enum!`（无字段枚举 + `as_str` + `Display`），测试里就不用再抄名字表。跨模块用 `macro_rules!` + `pub(super) use`，展开里写 `$crate::…` 全路径。会把函数定义藏起来、让人跳不到声明处的，用共享模块而不是宏。
- 用 python 脚本改 Rust 源码时：先 `cargo fmt`，按**精确字符串**匹配，并逐步打印是否命中（rustfmt 会重排你以为的那一行）。
- 串联多条检查再提交时，逐条捕获退出码；`grep | head` 这类管道会吞掉失败。

## 踩过的坑（别重复）

- `w:type`（小写 `type`）的 `LocalName` 是 `Type`；`[Content_Types].xml` 里大写的 `Type` 才是 `UType`。名字表里同名不同大小写的，大写那个加 `U` 前缀。
- 重复 `styleId` 取**最后一个**（TS 的 `Map` 语义）；默认样式取最后一个 `w:default`，没有则取 ID / 名为 `Normal` 的，不是 ECMA 的 first-of-type。
- `xml:space="preserve"` 会从祖先继承，判定要走祖先链。
- 辅助 part 先按关系找，找不到再按约定路径（`word/settings.xml` 等）——语料里有缺关系的文档。
- 没有 theme part 时仍按内建 Office 调色板解析 `themeColor`。
- `mc:Choice Requires="w14"` 而 `w14` 未声明时走 Fallback（已登记为已知差异）。
- 调试构建有自检：`SAVE-08` 的 `Clean` 子串抽样、`check_dirty_invariants`、`PROP-05` 顺序。它们会让"看起来能跑"的错误实现直接失败，别绕过它们。
