# SPEC 19 · M8′ 任务分解（原生协议与独立交付）

> **本文件在 2026-09-08 被整体替换。** 原 SPEC 19 是「M8：编辑器切换到 Rust 引擎」——把 genoffice `apps/docs` 的
> `parseDocx / saveDocx / buildBlankDocx` 换成 rsword wasm 绑定，附带双引擎分派、151 个 vitest、22 个 e2e、5 份像素基线、
> 切换开关与发布说明。项目负责人于 2026-09-08 改定范围：**rsword 是独立的 docx 读写内核，genoffice 退为测试基准**
> （只读地跑它的 TS 引擎生成 `corpus/**/*.expected.json`），不再切换它的引擎、不再迁移它的编辑器、不再删它的代码。
> 原 M8 的全部任务（8.0–8.6）与六道门**撤销**，git 历史里可查（`m8-editor` 分支的 `6ac8618` / `e481f2e` 曾实现过 8.1a / 8.0a，基线在 M7 7.9 之前）。
> 新的 M8′ = 原 `spec/20` M9 里**属于 rsword 的那半边**（9.1–9.4、9.8）前移，再加两件独立交付才需要的事：
> Rust crate 公共 API 定型，以及在删 `compat_ts` 之前先把自快照回归网建起来。

对应 `docs/03` v3.3 §12 的 M8′ 行。格式同 `spec/12`–`spec/18`：每个任务给出产出、依赖的规范条目与完成定义（DoD）。
顺序即建议的实现顺序；同一编号内的子任务可并行。基线：`main` = `32234ce`（M7 全部并入），分支 `m8-native`，工作树 `../rsWordParser-m8n`。

## 目标

（1）**对外只剩一个原生协议**（`spec/21-bind.md`，前缀 `BIND`）：`open → document / resolve / media → apply / save → close`；
（2）**模型 JSON 是 `Document`（`MOD-01`）的投影**——字段是文档事实与声明值，不含 dataURL、原字节切片、TS 形态的半解析字段、
排版决定的字段（`MOD-11` 的禁令延伸到协议）；（3）**每条写路径都是 `EditOp`**（60 个变体全部可 JSON 往返）；
（4）**媒体经 `MediaId` 按需取字节**，格式转换由调用方接管；（5）**Rust crate 的公共 API 定型**——今天 `lib.rs` 把 11 个模块
全部 `pub` 出去、758 个 `pub fn`、318 个公共类型，没有一处是按「对外 API」设计的，这在独立交付下是不可接受的；
（6）**回归网换代**：`*.model.json` 自快照 + `TEST-07` 走协议 + `fuzz_bind`，建成之后 `compat_ts` 降为
`#[cfg(feature = "compat-ts")]` 的测试专用件（**不删**，见「分层决策」2）。

做完之后 rsword 是一个可以独立发布的 crate：`cargo add rsword` → `open` 一份 docx → 读模型 JSON → 发 `EditOp` → 存回字节。

**M8′ 门**（同步写进 `spec/11` TEST-10 M8′ 行）：

1. **协议一致性**：`document()` 对全部语料（799 synthetic + 266 real + 38 hostile）的输出通过 JSON Schema 校验；
   JSON → Rust `DocumentJson` → JSON 幂等（serde 往返逐字节相同）；与 `Document::rebuild` 的字段逐一对应——`MOD-01`–`MOD-11`
   的字段清单做成 checklist 测试，投影**不丢字段**（由 `model_json!` 宏同表展开，见「实现约定」）。
2. **操作全覆盖**：60 个 `EditOp` 变体各至少一条 JSON 往返测试（构造 → `to_string` → `from_str` → 相等）；同一条操作
   经协议 `apply` 与经原生 `EditSession::apply` 产生**逐字节相同**的保存结果（全语料抽样 + `TEST-07` 的 1,000 条序列）。
3. **公共 API**：`cargo doc --no-deps` 零警告；公共面开 `#![warn(missing_docs)]` 且为零；**默认 feature 构建不含 `compat_ts`**
   并能完成 `open → document → apply → save`；`examples/` 至少三个（读、改、Agent 式定位改写）在 CI 里跑；
   公共类型全部 `#[non_exhaustive]` 或有明确的稳定性声明（`BIND-11`）。
4. **回归网换代**：`corpus/**/*.model.json` 快照进 CI，改动必须由带理由的提交更新；`TEST-07` 改走协议 JSON
   （PR 100 条 / nightly 1,000 条）；`fuzz_bind`（任意 JSON 喂 `apply` / `document` / `resolve*`：不 panic、`Err` 时状态逐字节不变）
   10 分钟无崩溃。
5. **既有门不退**：`--features compat-ts` 下，九道 `diff-parse` 门（七个 scope + `corpus/real` + `save_blocks`）继续 0 处未知差异；
   往返、编辑保真、`TEST-07`、四个 fuzz、hostile 全绿；`cargo test --workspace` 与 `--release` 双绿。
6. **体积与性能**：带图语料（`m6-*`、`image-*`、`hf-images__*`、`corpus/real` 带图的）`document()` JSON 体积相对
   `compat_ts::parsed_doc`（含 dataURL 与 `internal.documentXml`）下降 **≥ 50%**；单操作 `apply` p95 < 5 ms；
   `save` < 50 ms / MB；`.wasm` gzip ≤ 3 MiB。数字进 `docs/05`。

### 实测基线（2026-09-08，`main` = `32234ce`）

| 量 | 值 | 来源 |
| --- | --- | --- |
| 测试 | 646 通过 / 0 失败（debug 与 release 双跑），51 个集成测试文件 | `cargo test --workspace` |
| 语料 | 799 synthetic + 266 real + 38 hostile；1,065 份 `*.expected.json`；208 份 `*.save.<k>.json` | `find corpus` |
| 差分门 | 七个 scope + `corpus/real`：242 + 547 处已知差异，**0 处未知**；`save_blocks` 204/208 等价 + 189/289 部件比对 | `diff-parse`、`save_blocks.rs` |
| 引擎源码 | `crates/rsword/src` 57,457 行；`bind/compat_ts/` 17 文件 12,782 行；`KNOWN_DIFFS.md` 145 行 | `wc -l` |
| 公共面 | `lib.rs` 导出 11 个模块全 `pub`；758 个 `pub fn`；318 个 `pub struct/enum/trait/type`；**零 `missing_docs` 约束** | `grep` |
| `EditOp` | **60 个变体** | `edit/mod.rs` |
| 绑定 | `crates/rsword-js`（wasm-bindgen，7.10）：`version / parse / save / blank`；`bind/js.rs` 是语言中立核心 | `spec/18` 7.10 |
| 依赖 | `zip` / `memchr` / `thiserror` / `serde_json`；**对 genoffice 零构建期与运行期依赖**（`.rs` 里 "genoffice" 出现 0 次） | `Cargo.toml`、`grep` |

## 任务

| # | 任务 | 规范 | DoD |
| --- | --- | --- | --- |
| 8.0 | **范围收口与分支归并**：① `docs/03` v3.3、`spec/00` / `spec/10` / `spec/11` / `CLAUDE.md` / `docs/04` / `docs/05` 按新范围改完（本次提交）；② `m8-editor` 分支收口——把 `6ac8618`（m8.1a）里**值得留的**摘到 `main`：`crates/rsword-js` 的 `wasm_export!` 表、`parse_diagnostics` 出口、`DiagCode::BindBadArgument`、`tools/js-parity/{parse,save,blank}_parity.mjs`（node 里实测过 1,065 份文档 + 208 份 save + blank 字节相同）、`TOOLS.md` 钉 `wasm-bindgen-cli` 版本、CI 的 wasm 步骤；**丢弃** `e481f2e`（m8.0a）的 `tools/m8-audit/` 与 `docs/10-m8-engine-switch.md`（genoffice 审计，已无对象）；冲突处 `save_blocks.rs` 取 `main` 的 7.9c 版本再嫁接 `js_binding_save_bytes_parity`；③ `tools/export-golden/README` 写明「genoffice 只读使用」的定位与最后重导提交号 | TEST-02, TEST-03, COMPAT-08 | 文档改完且自洽（`spec/00` 表、`spec/11` 门行、`CLAUDE.md` 三处）；`m8-editor` 的绑定并入 `main` 后九道门与 646 测试重跑全绿；`m8-editor` 分支删除或标废弃 |
| 8.1 | **协议规范 `spec/21-bind.md`**（前缀 `BIND`，`spec/00` §0.2 加一行）：把「分层决策」誊成可验收条目——`BIND-01` 会话与生命周期（`open(bytes) -> SessionId`、`close(id)`；一个会话一份文档；任何失败不留半状态）；`BIND-02` 模型 JSON（`MOD-01` 的投影；字段名 = Rust 字段的 camelCase；单位按 `spec/00` §0.4 原值；`named_enum!` 的字串即 JSON 值；`Run` = 物理 `w:r`；坐标流 UTF-16；**不含** dataURL / 原字节 / 排版字段；`nodeId / partId / spanId / fieldId / revisionId / mediaId` 的稳定性范围）；`BIND-03` `EditOp` JSON（`#[serde(tag = "op")]`；60 个变体清单；`EditContext`；`MutationResult`）；`BIND-04` 保存与 `SaveOptions`（收缩到包级五项：`savedAt / removePersonalInfo / removeDateAndTime / pruneOrphans / normalizeZOrder`，其余翻译入口公开为 `EditOp`）；`BIND-05` 媒体句柄（`media(id) -> bytes`、`addMedia`；不内联）；`BIND-06` `resolve` 查询（五个批量接口）；`BIND-07` 诊断与错误码（`code` + `message`，`DiagCode` 稳定）；`BIND-08` 协议版本（`protocolVersion` 不匹配即拒绝，不静默）；`BIND-09` 只读出口 `partBytes` / `nodeXml`（调试用，生产 lint 禁用）；`BIND-10` **按需取与预算**（`document(opts)` 的块范围、字段裁剪、深度上限——见「分层决策」3）；`BIND-11` crate 公共 API 的稳定性承诺 | 全部 | `spec/21` 评审通过（**关口**：8.2–8.5 的 API 面在它之后才定）；`spec/00` §0.2 表加 `BIND` 行；`docs/03` §8.2 之外的新操作（`spec/18` 待决 5）在此收进清单 |
| 8.2 | **模型 JSON 投影**（`bind/native/json.rs`、`bind/native/schema.rs`）：独立投影层——**不**在 `model/` 类型上直接 `derive(Serialize)`（模型持 `NodeId` / `Range<u32>` / arena 引用，`Display` 里有节点引用，`RevisionMeta` 里有承载节点）。`model_json!` 宏按「Rust 字段 → JSON 字段（可选换名 / 转换函数）」一张表同时展开 `to_json`、schema 条目与门 1 的「不丢字段」checklist 测试；`Option` / `bool` 沿用 `set_some!` / `set_if!`（从 `bind/compat_ts/json.rs` 搬到 `bind/native/`，`compat_ts` 反过来引用它）。覆盖：`main` 块序列、`Table` / 单元格、`sections`、`hfParts`、`notes` / `comments` / `sources` / `inks`、`styles` / `numbering` / `theme` / `fontTable` / `settings` / `CompatFacts`、`revisions`（7.1 的 `RevisionIndex`）、`spans` / `fields`、`display`（**可选投影**，缺省关闭，见「分层决策」4） | MOD-01–MOD-11, BIND-02 | 门 1 全绿；`display` 关闭时全语料 JSON 体积达标（门 6） |
| 8.3 | **`EditOp` / `EditContext` / `MutationResult` 的 JSON**（`edit/mod.rs` 的 serde、`build/props.rs` 给 patch 类型加 serde）：`EditOp` 与 `NewBlock / NewInline / NewAtom / NewField / NewComment / NewImage / NewChart / NewInk` 及属性表 `*Patch` 全部 `Serialize / Deserialize`（`#[serde(tag = "op", rename_all = "camelCase")]`；属性 patch 的 serde 由 `build/props.rs` 生成——`PROP-06` 的 diff / patch 形态已有）。`edit_op_json!` 宏展开每个变体的往返测试与 `spec/21` `BIND-03` 的清单（生成后人工校对），并在 `InsertBlock{Xml}` / `ReplacePartXml` 两处打开 `BIND_XML_ESCAPE` 计数点。`SaveOptions` 里只为 TS 存在的翻译入口（5.6 / 5.7 / 6.6–6.8 的节 / 页眉页脚 / 水印 / 编号 / 样式 / 主题 / 墨迹）**公开为 `EditOp`**，`SaveOptions` 收缩到 `BIND-04` 的五项 | EDIT-01–EDIT-06, PROP-06, BIND-03, BIND-04 | 门 2 全绿；`SaveOptions` 只剩五个包级键，其余有对应 `EditOp` 且各有一处测试 |
| 8.4 | **会话、媒体句柄、`resolve` 查询与部件读取**（`crates/rsword-js` 改为有状态；`bind/native/session.rs`、`bind/native/resolve.rs`）：`SessionTable: BTreeMap<SessionId, EditSession>`；导出 `open / close / document / apply / save / media / addMedia / resolveRuns / resolveParas / resolveCells / resolveSections / resolveTable / partBytes / nodeXml / diagnostics / version`。同形导出用 `bind_export!` 收拢（7.10 的 `wasm_export!` 是它的无会话前身），五个 `resolve*` 用 `resolve_query!`。**会话有状态、保存函数式**：`save(ops)` 在克隆上做，失败不影响读会话（「分层决策」5） | BIND-01, BIND-05, BIND-06, BIND-09, RES-01–RES-12 | 每个导出有「不存在的 `sessionId` → `BIND_NO_SESSION`」单测；五个 `resolve*` 对全语料与 `Resolver::*` 逐字段相等；`EditSession: Clone` 在最大真实文档上实测（超 50 ms 则改「apply 后回滚」，语义相同） |
| 8.5 | **Rust crate 公共 API 定型**（`lib.rs`、`Cargo.toml` 的 feature、`examples/`、`README.md`）：**这是原 `spec/20` 没有的任务，独立交付才需要**。① 公共面收敛：今天 11 个模块全 `pub`，逐个判定「对外 / `pub(crate)` / 藏进 `bind::native`」，对外的加 `#[non_exhaustive]`；② 错误面统一：`Error` / `DiagCode` 作为公共契约冻结，`DiagCode` 的 `as_str` 即 `BIND-07` 的 `code`；③ feature 划分：`default = ["native"]`；`compat-ts`（**测试专用**，默认关）；`wasm`；`serde`；④ `#![warn(missing_docs)]` 打开并补齐公共项文档；⑤ `examples/`：`read.rs`（open → 大纲 + 文本）、`edit.rs`（定位一段 → `ReplaceInlines` → save）、`agent.rs`（模型 JSON 进 / `EditOp` JSON 出，为 M9′ 打样）；⑥ `README.md` 从「genoffice 引擎替换品」改写为「独立 docx 内核」，给三段能跑的代码 | BIND-11 | 门 3 全绿；`cargo build -p rsword`（默认 feature）不编译 `compat_ts`；三个 example 在 CI 里跑 |
| 8.6 | **回归网换代**（`corpus/**/*.model.json`、`tests/model_snapshot.rs`、`tests/random_ops.rs`、`fuzz/fuzz_targets/fuzz_bind.rs`）：① 自快照——全语料 `document()`（`display` 关）落 `*.model.json`，与 `*.expected.json` **并存**；快照测试比对，差异必须由带理由的提交更新（`corpus/README` 写清规则，和「禁止手改 `*.expected.json`」不同：`*.model.json` 是**我们自己的**输出，可以改，但要说明为什么）；② `TEST-07` 的 1,000 条序列改走协议 JSON，`ModelFingerprint` 两视图等价断言不变；③ `fuzz_bind`；④ CI 增 nightly 的协议随机门 | TEST-06, TEST-07, TEST-10 | 门 4 全绿；自快照建成**之后**才允许做 8.7 的降级 |
| 8.7 | **`compat_ts` 降级、性能、体积与收尾**（`Cargo.toml`、`benches/bind.rs`、`docs/`）：① `bind/compat_ts/` 整体挂 `#[cfg(feature = "compat-ts")]`，连同 `tests/compat*.rs`、`tests/save_blocks.rs`、`tools/diff-parse`、`KNOWN_DIFFS.md`；`spec/10` 头部改写生命周期（**不撤销条目**，改为「测试专用」）；② `benches/bind.rs`：open / document / apply / save / media 在最大三份真实文档上；③ JSON 体积对比表（`compat_ts::parsed_doc` vs `document()`）；④ 文档：`docs/03` v3.3 §12 勾掉 M8′、`docs/04` §17 逐条进度、`docs/05` 数字、`spec/11` TEST-10 M8′ 行 | COMPAT-01, TEST-10 | 门 5 与门 6 全绿；`cargo test --workspace`（默认 feature）与 `cargo test --workspace --features compat-ts` 双绿 |

建议顺序：8.0 → 8.1（**关口**，评审通过才动代码）→ 8.2 / 8.3 / 8.4 并行 → 8.5 → 8.6 → 8.7（最后）。

## 分层决策（实现前定死）

1. **协议是 `Document`（`MOD-01`）与 `EditOp` 的 serde 投影，不是第三个模型**：JSON 字段名 = Rust 字段名的 camelCase；
   不为调用方方便新造语义字段——调用方要的派生值（px、合并 run、标签文字）自己算或问 `resolve`。
2. **`compat_ts` 保留、降级，不删**（对 `docs/03` v3.2 与原 `spec/20` 决策 10 的改判）。它原本被判为「纯负担、M9 删除」，
   前提是它要作为**对外契约**长期维护。genoffice 退为测试基准之后：它是 1,065 份文档差分的对接点，是目前最强的正确性证据，
   删了就没有外部裁判。做法是 `#[cfg(feature = "compat-ts")]` + 默认关 + 不进公共 API + 不承诺稳定。
   `KNOWN_DIFFS.md`、`tests/save_blocks.rs`、`tools/diff-parse`、`corpus/**/*.expected.json` 同此处置——**一律不删**。
3. **`document()` 首版就支持按需取**（对原 `spec/20` 待决 1「首版整份，量过再加」的改判）。原来的消费者是编辑器：
   一次性拿整份、常驻内存、增量刷新。M9′ 的消费者是 Agent：一份百页文档的完整模型 JSON 是数 MB，塞不进上下文。
   `BIND-10` 从第一版就给块范围、字段裁剪与深度上限；`document()` 无参调用返回整份仍然保留（本地工具与测试用）。
4. **显示模型不进默认 JSON**：`ChartDisplay` / `VmlDisplay` / `DiagramDisplay` / `AnchorGeom` 是为渲染器造的，
   Agent 不需要 WordArt 的 EMU 坐标。已建成的不删，做成 `document(opts.display = true)` 的可选投影。
5. **会话有状态、保存函数式**：读会话在 `open` 之后只被换会话改变；`save(ops)` 在克隆上做，失败不影响读会话；
   调用方写盘成功才换会话。实时 `apply`（每次击键一条操作、引擎成为编辑期真相）另立里程碑。
6. **id 会话内稳定、跨会话无意义**：`nodeId`（arena 稳定，`MOD-13`）、`revisionId`、`spanId / fieldId / mediaId`；
   调用方不得持久化 id；换会话后按块序 + 文本指纹重对齐。
7. **偏移单位仍是 UTF-16**（`docs/03` §8.1 留给 M9 的决定，v3.3 拍定**不改**）。
8. **媒体不进 JSON、转换不进 Rust**：`MediaId` + 按需字节；metafile / TIFF 转换是调用方的服务（`docs/03` §3.5 冻结）。
9. **不是文档语义的东西不进引擎**（`docs/03` §1.2）：密码哈希、引文格式化、编号显示的**文字与缩进**留给调用方；
   编号计算本身（`lvlRestart` / `numStyleLink` / `startOverride` / 跨文档序累加）是文档语义，进 `resolve::list_markers`。
10. **逃生口有名有姓**：`InsertBlock{Xml}` / `ReplacePartXml` 保留，每次使用记诊断 + 计数（`BIND_XML_ESCAPE`）。
    M8′ **不**要求计数为 0（那是原 M9 门 2 的编辑器指标，已随之撤销）；M9′ 用它衡量 Agent 路径的成熟度。
11. **删除是最后一步且要有替代品**：8.7 的降级只在 8.6 的自快照网建成、门 4 绿之后做。

## 实现约定：多用声明宏（用户要求，2026-09-05 / 09-07 再次强调；与 `spec/14` / `spec/16` / `spec/17` / `spec/18` 同一条）

M8′ 的**同形重复**是历次里程碑里最多的：几十个结构要投影成 JSON、60 个 `EditOp` 变体要往返、十几个导出同形、五个 `resolve`
查询同形。判断标准仍是**同一形状重复三次以上就收成 `macro_rules!`**；每个宏同时展开实现、schema 与测试三样，让
「投影不丢字段」「变体都能往返」「导出都会映射错误」不靠人记：

- `model_json!`：一张「Rust 类型 → { JSON 字段 ← Rust 字段 [via 转换函数] }」的表，展开 `impl ToJson`（`set_some!` /
  `set_if!` 处理 `Option` / `bool`）、`schema()`（该类型的 JSON Schema 片段）、`#[test] json_fields_cover_struct`
  （键集与表里的字段集比对——门 1 的「不丢字段」）。**不**在 `model/` 类型上 `derive(Serialize)`。
- `edit_op_json!`：`EditOp` 变体清单，展开线型、上下文化转换、每个变体一条往返测试、`BIND-03` 的变体与字段清单、
  `BIND_XML_ESCAPE` 的计数点。**`EditOp` 不做无上下文 serde**（这里曾写「`EditOp` 本身用 `serde` derive
  （结构简单、无 arena 引用）」——**错了**：`NewElement` 经 `QName` 携带 per-Dom 的 `Interned` 句柄，
  见 `spec/21` BIND-03 v2）：线型 `EditOpJson` derive，引擎型不 derive，转换带 `&Dom` / `&mut Dom`。
- `bind_export!`：导出「`sessionId` + JSON 字符串入 → JSON / `Vec<u8>` 出 + `Error → { code, message }` 映射」，
  十几个同形；展开导出函数与「不存在的 `sessionId` → `BIND_NO_SESSION`」的单测。7.10 的 `wasm_export!` 并入它。
- `resolve_query!`：五个 `resolve*`（run / para / cell / section / table）同形——「`[nodeId]` 入 → 每个 id 一条
  `{ value, provenance }`」，展开导出、JSON 投影与「对全语料与 `Resolver::*` 逐字段相等」的测试。
- 沿用：`named_enum!`（`as_str` 直接就是 JSON 字符串，`schema_enum!` 从同一名字表生成枚举 schema）、`set_some!` /
  `set_if!`、`xpath_asserts!`、`fixture_tests!`、`oracle_tests!`。
- **不上宏**：`SessionTable` 的生命周期（两个函数）；`examples/`；feature 门控。
- 宏带文档注释与 ```ignore 用例；跨模块用 `macro_rules!` + `pub(super) use`，展开里写 `$crate::…` 全路径；
  会把函数定义藏起来、让人跳不到声明处的，用共享模块而不是宏。

其余约定照旧：树遍历写成**迭代**；属性容器只走 `plan_apply_*`；一个任务一个提交 `m8.<n>: 英文摘要 (SPEC-ID…)`；
提交前同步 `docs/04` §17 勾选、§8 偏差表、`docs/05` 数字。

## 从 M0–M7 带过来的债（M8′ 内解决）

| 债 | 位置 | 解决任务 |
| --- | --- | --- |
| 公共 API 从未设计过（11 个模块全 `pub`、758 个 `pub fn`、无 `missing_docs`） | `lib.rs` | 8.5 |
| `compat_ts` 12,782 行长期挂在默认构建里 | `bind/compat_ts/` | 8.7（降级，不删） |
| `docs/03` §8.1「M9 若前端协议改变再考虑标量或字素单位」 | — | v3.3 已拍定不改，记 `docs/04` §8 |
| `spec/18` 待决 5：`docs/03` §8.2 之外的新操作收进下一版 | `docs/03` §8.2 | 8.1 |
| `save/options/` 的 TS `SaveOptions` 翻译层（5.6 / 5.7 / 6.6–6.8） | `save/options/` | 8.3 公开为 `EditOp`，收缩到五项 |
| `TocOptions.ts_shape` 与 `fixtures/fieldgen` 的 TS 夹具（7.8） | `span/field/generate/` | 随 `compat-ts` feature 一起门控（8.7） |
| `compat_ts::parsed_doc` 经 `serde_json::Value` 中转 | `bind/compat_ts/mod.rs` | 不动（测试专用件，性能不再是对外指标） |
| `docs/06` toggle 未决（`strike` 一族桌面版复核） | `resolve` | 不挡 M8′；仍是项目负责人择机 |
| `m8-editor` 分支上 8.1a 的绑定与 node 实测 harness 尚未并入 `main` | `crates/rsword-js` | 8.0② |

## 基线与复用

| 来自 | 复用什么 | 在哪个任务 |
| --- | --- | --- |
| M7 7.10 / `m8-editor` 8.1a `crates/rsword-js` | wasm 构建与产物、错误映射、`wasm_export!`、node 实测 harness | 8.0② / 8.4 |
| M7 7.1 `RevisionIndex` / `RevisionId` | `revisions` JSON 与 Accept / Reject 引用 | 8.2 |
| M7 7.9 `ModelFingerprint`、`TEST-07` 生成器、`fuzz_edit` | 协议往返 oracle、`fuzz_bind` 的种子与断言 | 8.6 |
| M6 6.7 `EditSession::add_media`、6.6 xlsx 生成器 | `addMedia`；`SetChartData` | 8.4 / 8.3 |
| M5 5.6 / 5.7 `save/options/*` | 翻译逻辑 → 公开 `EditOp` | 8.3 |
| M4–M6 显示模型（`Display`、`ChartDisplay`、`DiagramDisplay`、`FormulaDisplay`、`VmlDisplay`） | 可选 `display` 投影 | 8.2 |
| `resolve` `RES-01`–`RES-12`（含 `TableView`、节视图、`Provenance`） | `resolve*` 查询 | 8.4 |
| 属性表生成器 `build/props.rs` | patch 类型的 serde 派生 | 8.3 |
| `model/macros.rs::named_enum!`、`bind/compat_ts/json.rs::set_some! / set_if!` | 投影层的两个基础宏（`json.rs` 搬到 `bind/native/`，`compat_ts` 反过来引用） | 8.2 |

## 不在 M8′

- **Agent 接口层**（文本投影、大纲与定位、变更摘要、预算）与 **CLI / MCP server** —— M9′（`spec/20`）。
- **实时 `apply`**（每次击键一条 `EditOp`、引擎成为编辑期真相）与协同编辑 —— 决策 5 保留保存时差分；另立里程碑。
- **分页 / 排版进引擎**（`docs/03` §1.2 永久不做）。
- **删除 `compat_ts` / `KNOWN_DIFFS.md` / `tools/diff-parse` / `corpus/**/*.expected.json`** —— 决策 2 明确不删。
- **genoffice 的任何改动** —— v3.3 之后它只是测试基准。`tools/export-golden/` 保留且只读使用。
- `.doc` / RTF / ODT；napi 形态；多线程 wasm。
- **`resolve` 的新规则**（toggle 复核、Wingdings 2/3 补全）——按 `docs/06` 择机，不在 M8′ 门。
- **引擎新能力**：WordArt 新建、图表工作簿同步等原 `spec/20` 9.3 的两个缺口——没有编辑器逼门，推迟到有真实需求时再立项。

## 风险提示（实现前确认）

1. **公共 API 一次定型的难度**：758 个 `pub fn` 逐个判定是苦工，且判错了以后要破坏性变更。缓解：8.5 先只承诺
   `bind::native` + 六个核心类型 + `EditSession` 的稳定，其余标 `#[doc(hidden)]` 或 `unstable` feature，留出一版的观察期。
2. **`compat_ts` 门控的连锁**：17 个文件、`tests/compat*.rs`、`save_blocks.rs`、`diff-parse`、`fixtures/fieldgen`
   都要挂 feature，CI 要跑两套构建。缓解：8.7 一次性做完，且放在最后。
3. **自快照的信噪比**：1,065 份 `*.model.json` 一旦有个字段改名就会整体飘红，容易养成「无脑更新快照」的习惯。
   缓解：`corpus/README` 写死「快照变更必须在提交信息里说明原因与影响面」；CI 对快照 diff 超过 N 份的提交打醒目提示。
4. **JSON 体积与 schema 规模**：`Document` 及其子结构几十个类型，schema 手写不可维护——必须由 `model_json!` 同表生成，
   否则会漂。TS 类型若将来要生成，走同一份 schema。
5. **`EditSession: Clone` 的成本**：保存函数式依赖克隆；arena 与索引是纯数据但最大真实文档可能有百万节点。
   8.4 先量，超 50 ms 换「apply 后回滚」实现，对调用方语义相同。
6. **`resolve` 暴露后的性能**：调用方对每个 run 问一次就是 O(n) 次跨边界调用；`BIND-06` **只给批量接口**；
   若仍慢，`document()` 顺带返回一份 `resolveAll`——8.4 先量再定。
7. **协议漂移**：`protocolVersion` 不匹配时启动拒绝，不静默；schema 进 CI 校验。
8. **`TEST-07` 走协议后变慢**：JSON 往返 × 1,000 序列；PR 只跑 100 条（`spec/18` 7.9 同款）。
9. **失去 TS 裁判的时点**：决策 2 已经把「删除」换成「降级」，所以本里程碑内不会失去裁判。但 `corpus/**/*.expected.json`
   的再生依赖 genoffice 的 TS 引擎仍然存在且能跑——`tools/export-golden/run.sh` 至少每个里程碑跑通一次，别让它烂掉。
10. **引擎类型持 per-Dom 句柄**：`QName` 可能装着 `NsId::Other(Interned)` / `LocalName::Other(Interned)`，
    `Interned` 只在产生它的 `Dom` 的 `Interner` 里有意义（`xml/interner.rs`）。任何「无上下文序列化」的假设
    （derive serde、跨会话搬运、句柄持久化）落笔前都要先验证类型里没有 `Interned`——8.1 v1 的 BIND-03 就栽在
    这上面（`spec/21` v2 改为线型与引擎型分离，`edit_op_from_json` / `edit_op_to_json` 带 Dom 上下文）。

## 待决（需要项目负责人拍板）

| # | 事项 | 建议 |
| --- | --- | --- |
| 1 | TS / 其他语言类型生成：`serde` + `schemars` → JSON Schema，还是 `ts-rs`；是否允许 rsword 加 `serde` derive 依赖（今天只有 `serde_json`） | 建议：`serde` + `schemars`（schema 同时服务 `fuzz_bind` 与门 1 校验），类型生成留给调用方 |
| 2 | crate 发不发 crates.io，什么名字（`rsword` 已被占用与否未查） | 建议：M8′ 内先不发，`README` 给 git 依赖写法；名字与发布在 M9′ 交付时一起定 |
| 3 | 8.5 公共面的保守程度：一次全定 vs 只定 `bind::native` 留观察期 | 建议：只定 `bind::native` + 六个核心类型（风险 1） |
| 4 | 编号显示计算（原 `spec/20` 待决 3）归 `resolve::list_markers` 还是调用方 | 建议：进 `resolve`（决策 9 已按此写，需你确认） |
| 5 | `*.model.json` 快照放 `corpus/` 内还是单独 `snapshots/` 目录 | 建议：放 `corpus/` 内与 `*.expected.json` 并列，一份文档的所有期望值在一处 |
| 6 | `m8-editor` 分支：并入后删除，还是留作历史 | 建议：8.0② 摘完后删分支，git 历史里 reflog 与 `362b555` 可查 |
