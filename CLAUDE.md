# CLAUDE.md

给在这个仓库里工作的 Claude Code 会话的说明。人类同样可读。**当前进度与能力边界看 `docs/05-status.md`**，不要凭印象假设某个功能已经存在。

## 这是什么

`rsword`：高保真 DOCX 编辑内核（Rust），用来替换 genoffice `packages/docx-engine` 的 `parseDocx` / `saveDocx`。解析 Word 文档为编辑器可消费的模型，接受编辑操作，以**字节级局部补丁**写回。不做布局与渲染。

一句话原则：**文件是真相。** 未编辑的内容一个字节都不动；编辑只发生在被标脏的 XML 节点上。

## 权威顺序

1. `docs/03-architecture-v3.md` —— v3.2 **冻结架构**，是宪法。分层、六个核心类型、不变式在这里定。
2. `spec/*.md` —— 可验收的模块规范，每条带 ID（`XML-12`、`PROP-06`、`EDIT-03`…）。实现与测试都引用这些 ID。规范服从设计；冲突时以设计为准并修订规范。
3. `docs/04-dev-plan.md` —— 执行计划：§5.1 M1 已完成清单、§5.2 M1 门、§8 实现偏差、§9 待决、§10 M2 及以后的排期、§11 M2 逐条进度、§12 M3 逐条进度、§13 M4 逐条进度、§14 M5 逐条进度、§15 M6 逐条进度、§16 M7 逐条进度。
4. `docs/01-ts-parser-reference.md` 与 genoffice 源码 —— **参考实现，不是验收权威**（见下）。

`spec/12-m0-m1-plan.md`、`spec/13-m2-plan.md`、`spec/14-m3-plan.md`、`spec/15-m4-plan.md`、`spec/16-m5-plan.md`、`spec/17-m6-plan.md`、`spec/18-m7-plan.md` 是里程碑任务分解（# / 任务 / 规范 / DoD）。
M0–M6 已全部并入 `main`；M5（页眉页脚 / 节 / 声明 part / resolve 校准）**九个任务与五条门全部完成**（`RES-04` 的 toggle 规则已按 2026-09-06 的 Word 实测校准），逐条进度与门的实测见 `docs/04` §14。**M6**（嵌入对象：图表 / SmartArt / 画布 / OLE / 公式 / 墨迹、媒体写侧）**九个任务与五条门全部完成**（2026-09-06；八道 `diff-parse` 门含 `--scope all` 都在 CI），逐条进度与门的实测见 `docs/04` §15。**M7**（`spec/18`：修订生成与接受 / 拒绝、`EditOp` 全集、块字段生成器、绘图编辑、随机序列门、JS 绑定）**进行中**（2026-09-07 从 `main` = e5bed96 开 `m7-edit`，工作树 `../rsWordParser-m7`），开工基线已在 `docs/04` §16 重测。

## TS 不是权威

genoffice 的 TS 引擎是参考实现。目标是**功能等价或更强**，差分测试只是发现回归的手段，不是目标。

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
| `crates/rsword/src/package/` | L0 包层（zip、`[Content_Types].xml`、`.rels`、flavor） |
| `crates/rsword/src/xml/` | L1 无损 DOM（tokenizer、`Dirty`、MCE、命名空间、`plan`、`fragment`、`canon`、`xpath`） |
| `crates/rsword/src/span/` | L2 范围与字段（`content` / `index` / `transform` / `materialize` / `field`） |
| `crates/rsword/src/semantic/props/` | L3 属性表。**生成代码**：`schema/props/*.toml` + `build/props.rs` → `$OUT_DIR/props.rs` |
| `crates/rsword/src/model/` | L3 文档模型投影（`Document::rebuild`、块分类、坐标流） |
| `crates/rsword/src/resolve/` | 有效属性只读视图（样式链、主题字体 / 颜色） |
| `crates/rsword/src/edit/` | L4 编辑引擎（`EditSession`、`InlinePos`、`MutationPlan`、操作） |
| `crates/rsword/src/save/` | 校验、序列化、包写回、保存选项 |
| `crates/rsword/src/bind/compat_ts/` | 兼容适配器：`ParsedDoc` JSON、`SaveBlock[]` 映射、差分 |
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
cargo clippy --workspace --all-targets      # 必须零告警
cargo test --workspace                      # 调试构建
cargo test --workspace --release            # 必须也跑：enforce 只在调试构建报错，发布构建行为不同
cargo run -p diff-parse -- --scope text     # M1 门：文本用例未知差异必须为 0
cargo run -p diff-parse -- --scope fields   # M2 门：再加字段 / 范围 / 批注，仍须为 0
cargo run -p diff-parse -- --scope tables   # M3 门：再加含表格的文档（按文档筛），仍须为 0
cargo run -p diff-parse -- --scope drawing  # M4 门：绘图域**路径**（不是按文档筛），仍须为 0
cargo run -p diff-parse -- --scope hf       # M5 门：页眉页脚域**路径**，仍须为 0
cargo run -p diff-parse -- --scope embedded # M6 门（进行中）：嵌入对象域（路径 + 期望块 label），目标 0
cargo run -p diff-parse -- --scope all --json          # 全域差距排名
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
- 编辑操作的事务边界是 `EditSession::apply` / `apply_all`：`plan`（只读）→ `validate`（只读）→ `commit`（机械写入，不可失败）。任一步 `Err` 必须不留半修改状态。`commit_plan` 会为事务碰过的每个 part 记写前镜像。
- **同一形状重复三次以上就上声明宏**。已有的：`bind/compat_ts/json.rs` 的 `set_some!`（有值才写）与 `set_if!`（为真才写 `true`）——投影层新写字段用它们，别再手写 `if let Some`；`model/macros.rs` 的 `named_enum!`（无字段枚举 + `as_str` + `Display`），测试里就不用再抄名字表。跨模块用 `macro_rules!` + `pub(super) use`，展开里写 `$crate::…` 全路径。会把函数定义藏起来、让人跳不到声明处的，用共享模块而不是宏。
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
