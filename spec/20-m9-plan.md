# SPEC 20 · M9 任务分解

对应 `docs/03` 第 12 节 M9 行（「新模型 JSON、原生 `EditOp` 接口、媒体句柄、渲染器接管排版启发式；删除 `compat_ts`」，验收
「编辑器迁移完成」）。`spec/11` TEST-10 没有 M9 行——本文件的「M9 门」补上并同步写进 `spec/11`。格式同 `spec/12`–`spec/19`：
每个任务给出产出、依赖的规范条目与完成定义（DoD）。顺序即建议的实现顺序；同一编号内的子任务可并行。

M9 的内容 = 结束 `docs/03` 所说的「第一阶段」：编辑器与引擎之间只剩一个原生协议，`compat_ts` 与 TS 引擎一起消失。按出处：

- **`docs/03`**：§12 M9 行；§1.2「解析器内的排版决策……全部移到渲染器或由 `resolve` 提供带来源的数据」；§3.5「`compat_ts` 阶段内联
  dataURL；之后改句柄 + 二进制表」；§6.3「`docxIndex / originalXml / rawPPr / rawRPr / rawTcPr / rawTrPrs / sdtShell` 全部由 `NodeId`
  与其 `lex` 替代」「`label` 变为 i18n key」「控制字符协议废除」「核心模型不做逻辑 run 合并……是编辑器视图与 `compat_ts` 的投影行为」；
  §8.1「M9 若前端协议改变再考虑标量或字素单位」；§14「`compat_ts` 成本……限定一个模块并设删除期限（M9）」。
- **`spec/10`** COMPAT-01「生命周期：M1 建立，M9 删除」；`spec/16` 风险 7「`richParas` 是 TS 形态的投影，M9 随 `compat_ts` 一起删」；
  `spec/17` 决策 1「`extras.chartParts` 给原字节只是为 compat，M9 随 `compat_ts` 一起删」与「不在 M6」末条；`spec/14`「不在 M3」：
  `floatSide` 等显示启发式只出现在 `resolve` 视图与 `compat_ts`，不进模型；`spec/18`「不在 M7」：`compat_ts` 的删除、原生 JSON
  `EditOp` 协议、媒体句柄——M9；`spec/18` 待决 5（已拍板）：`docs/03` §8.2 之外的新操作「M9 定原生协议时一并收进 `docs/03` 的下一版」。
- **`KNOWN_DIFFS.md`** 两条「渲染器接管后删除」（`balance-dbcs-spacing__*` 的 `charSpacingTwips`、`charIndents*` 的字号换算）；
  `docs/05`「债务与风险」末条「`compat_ts` 是负担性代码，删除期限定在 M9」。
- **`docs/01`** §13.3「建议输出层默认不内联 base64……改为 `media_id` 引用 + 单独的二进制表；但这改变了 `Block.imageDataUrl` /
  `Run.image.dataUrl` 契约，需要编辑器配合」。

M9 是 M8 之后的**串行**里程碑，工作面**两边都大**：rsword 新增 `bind/native/`（协议层）并删 `bind/compat_ts/`（12,684 行）；
genoffice 重写编辑器与引擎的边界（`convert.ts` 2,724 行、`file-actions.ts` 1,152、`protected-render.ts` 1,501、`ai/protocol.ts` 1,831
是主战场）并删 TS 引擎（`parse.ts` 5,591、`generate.ts` 3,268、`patch.ts` 1,892……共 21,309 行）。分支：rsWordParser `m9-native`，
genoffice `rsword-native`。本计划在 2026-09-07 写成，M7 进行中、M8 未开始；下文基线来自今天两个仓库的实测，**开工前按 M8 并入后的
两个仓库重测**。

## M9 · 原生协议：会话与句柄、模型 JSON、`EditOp` JSON、媒体句柄、`resolve` 查询；渲染器接管启发式；删除 `compat_ts` 与 TS 引擎

目标：（1）编辑器与引擎之间只剩一个协议（`spec/21-bind.md`，前缀 `BIND`）：`open → document / resolve / media → save(ops) → close`；
（2）模型 JSON 是 `Document`（`MOD-01`）的 serde 投影——字段是文档事实与声明值，不含 dataURL、原字节切片、TS 形态的半解析字段、
排版决定的字段（`MOD-11` 的禁令延伸到协议）；（3）编辑器的每条写路径都是 `EditOp`（含 M7 的新操作），`SaveBlock[]` / TS `SaveOptions`
消失；（4）媒体经 `MediaId` 按需取字节，metafile / TIFF 转换成为编辑器侧的惰性服务；（5）TS 解析器里的排版启发式全部搬到渲染器
（分页 / 布局层），用 `resolve` 的带来源数据代替猜测；（6）`bind/compat_ts/`、`KNOWN_DIFFS.md`、TS `docx-engine` 的解析 / 生成 / 补丁代码
删除，`spec/10` 标 `[已撤销]`。做完之后 rsword 是 genoffice docs 唯一的文档引擎。

**M9 门**（同步写进 `spec/11` TEST-10 M9 行）：

1. **协议一致性**：`document()` 对全部语料（799 synthetic + 266 real + 38 hostile）的输出通过 JSON Schema 校验；JSON → Rust
   `DocumentJson` → JSON 幂等（serde 往返）；与 `Document::rebuild` 的字段逐一对应——`MOD-01`–`MOD-11` 的字段清单做成 checklist 测试，
   投影不丢字段；`corpus/**/*.model.json` 快照进 CI，改动必须由带理由的提交更新（替代 TS 差分成为回归网）。
2. **操作全覆盖**：编辑器每条用户可达的写路径（功能区、右键、快捷键、AI 工具、审阅面板）在 `apps/docs/tests` 与 e2e 全跑一遍后，
   逃生口 `InsertBlock{Xml}` / `ReplacePartXml` 的使用计数为 **0**（引擎侧诊断 `BIND_XML_ESCAPE` 计数器）；`EditOp` 每个变体至少一处
   编辑器调用，或在 `docs/11` 明确登记「编辑器无此功能」。
3. **编辑器测试与 e2e**：`apps/docs/tests` 全部通过（49 个引擎相关测试改写成原生协议，其余不变）；`npm run test:e2e` 全绿；
   `docs-visual` 5 份基线**零 diff**（启发式换了地方，像素不变）；9.5 的每条启发式各一份专项视觉 fixture，基线在 M8 引擎下录，搬家后零 diff。
4. **删除完成**：`crates/rsword/src/bind/compat_ts/` 不存在；`KNOWN_DIFFS.md`、`tools/diff-parse`、`tests/compat*.rs`、
   `tests/save_blocks.rs`、`corpus/**/*.expected.json`、`*.save.*.json` 按 9.7 的决定删除；genoffice `packages/docx-engine/src` 的
   解析 / 生成 / 补丁 / 装载 / 归一化 / 空白 / 节 / 注释 / 水印 / 墨迹 / 参考文献 / 符号字体 / 字体表 / 图表 part 补丁 / 公式转换器删除；
   `grep -rnE 'parseDocx|saveDocx|SaveBlock|docxIndex|originalXml|rawPPr|rawRPr|imageDataUrl|\.dataUrl|sdtShell|fieldDisplay|previewText' apps/ packages/`
   只命中 metafile / tiff 转换服务与历史文档；`spec/10` 全部条目标 `[已撤销]`（编号保留），`spec/00` 表更新。
5. **性能**：带图语料（`m6-*`、`image-*`、`hf-images__*`、`corpus/real` 带图的）`document()` JSON 体积相对 M8 的 `ParsedDoc`（含 dataURL
   与 `internal.documentXml`）下降 ≥ 50%；单操作 `apply` p95 < 5 ms、`save` < 50 ms / MB（`spec/18` 7.9 的建议值转正）；最大真实文档
   `open + document` ≤ M8 的 `parse`；一个会话的 wasm 常驻内存 ≤ 文档字节数的 6×（`docs/03` §14 的 arena 估算）——数字进 `docs/05`。
6. **既有门不退**：rsword 全部 Rust 门（往返、编辑保真、`TEST-07`、四个 fuzz、hostile）继续绿；新增 `fuzz_bind`（任意 JSON 喂 `apply`
   不 panic、`Err` 时状态不变）10 分钟无崩溃；`corpus/real` 的 `real_edits` Word 验收流程改走协议后仍全部 open ok。

### 实测基线（2026-09-07）

| 量 | 值 | 来源 |
| --- | --- | --- |
| 编辑器对 compat 形态字段的消费（`apps/docs/src` 文件数） | `docxIndex` 28、`textboxes` 15、`dataUrl` 15、`imageDataUrl` 7、`originalXml` 7、`rawRPr` 7、`sdtShell` 5、`fieldDisplay` 5、`hfParts` 4、`extras` 3、`strayRuns` 2、`rawPPr` 1、`chartParts` 1 | `grep -l`（M8 8.0 的脚本） |
| 编辑器对 TS 引擎的值导入 | 54 个名字：M8 换掉 3 个；剩 51 个全在 9.3 的映射表里——换成 `EditOp` / 模型字段 / `resolve` 查询，或明确保留为渲染服务 | 同上 |
| 类型导入 | 57 个（`types.ts` 1,710 行）→ 改为协议生成的 TS 类型 | 同上 |
| 编辑器主战场 | `convert.ts` 2,724（`ParsedDoc ↔ ProseMirror`，含 `pmDocToSavePlan` 的签名差分）、`file-actions.ts` 1,152、`protected-render.ts` 1,501、`ai/protocol.ts` 1,831、`editor/revisions.ts`（`applyRevisionsBy` 在编辑器侧改 PM 文档） | `wc -l` |
| TS 引擎待删 | 33 个文件 21,309 行；87 个测试文件（语料源） | `wc -l` |
| `compat_ts` 待删 | 18 个文件 12,684 行 + `KNOWN_DIFFS.md` 三十余条 + `tests/compat.rs` / `compat_table.rs` / `save_blocks.rs` + `tools/diff-parse` | `wc -l` |
| `EditOp` 现状 | 36 个变体（M6 末）+ M7 新增十余个（`InsertAtom / SetNoteContent / SetSdtContent / AcceptRevision / RejectRevision / AcceptAll{author} / RejectAll / RemoveSdtShell / RegenerateBlockField / InsertSectionBreak / DeleteSectionBreak / SetDrawingGeometry / SetDrawingWrap / SetDrawingZOrder / SetShapeStyle / SetTextboxContent / SetMathTokens / RemoveNote`）≈ 50；`docs/03` §8.2 之外的登记在 `docs/04` §8 | `edit/mod.rs`、`spec/18` 决策 12 |
| 排版启发式（要搬家的） | 12 条（9.5 的表） | `docs/03` §1.2、`docs/01` §12、`KNOWN_DIFFS.md`、`compat_ts` 源码 |
| 媒体 | 语料 metafile 媒体 4 份（`emf-image__*`）+ `corpus/real` 的 `image-emf` / `ole-*`；编辑器 `dataUrl` 消费 15 个文件 | `KNOWN_DIFFS.md`、`grep` |
| `resolve` 覆盖 | `RES-01`–`RES-12` 首版 + 表格视图 + 节视图；toggle 的 `strike` 一族只在 Word 网页版测过（`docs/06`） | `docs/05` |
| 保存选项 | Rust `SaveOptions` 今天 20 余项（`save/options/mod.rs`），其中只有 `saved_at / remove_personal_info / remove_date_and_time / prune_orphans / normalize_z_order` 是包级；其余都是 5.5 / 5.7 / 6.6–6.8 编辑操作的翻译入口 | `save/options/mod.rs` |

### 任务

| # | 任务 | 规范 | DoD |
| --- | --- | --- | --- |
| 9.0 | **基线、盘点与迁移清单**（`tools/m8-audit/` 升级为 `tools/m9-migration/`；`docs/11-m9-migration.md`）：① M8 并入两仓库后重测上表；② 迁移清单生成器：每个 compat 字段消费点 → 目标（模型字段 / `resolve` 查询 / 渲染器本地计算），每个 TS 助手调用点 → 目标 `EditOp`（9.3 的表），每个 `SaveOptions` 键 → `EditOp`；产出 `docs/11` 的三张勾选表；③ 排版启发式清单定稿（9.5 的表），每条造一份专项 fixture docx——生成方式**不依赖 TS 引擎**（rsword `blank()` + `EditOp` 生成，或手工 docx 提交），在 **M8 引擎**下录像素基线（先录再搬，这是门 3 的对照）；④ 门 2 的分母：从功能区 / 右键菜单 / 快捷键表 / AI 工具清单 / 审阅面板枚举「用户可达的写路径」，进 `docs/11` 第四张表 | TEST-10 | 四张表就位，条目数与基线表一致；12 份启发式 fixture 有 M8 基线并进 `e2e/visual-baselines/` |
| 9.1 | **协议规范 `spec/21-bind.md`**（前缀 `BIND`，`spec/00` §0.2 加一行；`docs/03` 增补）：把下文「分层决策」誊成可验收条目——`BIND-01` 会话与生命周期（`open(bytes) -> SessionId`、`close(id)`；一个会话一份文档；任何失败不留半状态）；`BIND-02` 模型 JSON（`MOD-01` 的投影；字段名 = Rust 字段的 camelCase；单位按 `spec/00` §0.4 原值；`ProtectedKind` / `SegmentKind` / `MediaKind` 的 `named_enum!` 字串即 JSON 值；`Run` = 物理 `w:r`；坐标流 UTF-16；不含 dataURL / 原字节 / 排版字段；`nodeId / partId / spanId / fieldId / revisionId / mediaId` 是会话内稳定的不透明整数）；`BIND-03` `EditOp` JSON（`#[serde(tag = "op")]`，位置 `{ part, para, offset }` / `BlockPos`；`EditContext { trackChanges: { author, date }?, defaultRunProps?, markUpdatedFieldsDirty? }`；`apply(id, ops, ctx) -> MutationResult { affectedBlocks, offsetDelta, allocated }`，`Err` 时会话不变——`EDIT-05` 的对外表述）；`BIND-04` 保存（`save(id, ops, options) -> bytes` **函数式**：在会话克隆上 `apply_all` 再 `save_with`，读会话不变；`SaveOptions` 只剩包级五项）；`BIND-05` 媒体（`media(id, mediaId) -> { mime, kind, bytes }`、`addMedia(id, bytes, mime) -> mediaId`；`kind ∈ Raster \| Svg \| Metafile \| Tiff \| Other`；转换在调用方）；`BIND-06` `resolve` 查询（`resolveRuns / resolveParas / resolveCells / resolveSections / resolveTable(id, [ids])` 批量 → `RES-*` 有效值 + `Provenance`；`listMarkers` 见「待决」3）；`BIND-07` 诊断与错误（`Diagnostic` JSON；`Error { code, message }`；`BIND_XML_ESCAPE` 计数）；`BIND-08` 版本（`protocolVersion` 语义化版本，字段只增不改；`version()` 带 rsword 提交号；启动时不匹配拒绝而非静默）；`BIND-09` 部件只读（`partBytes(id, path)`——替代 TS `readDocxPartBase64` 读图表工作簿）；`BIND-10` 调试出口（`nodeXml(id, nodeId)`，生产代码 lint 禁用）。**TS 类型从 Rust 生成**（「待决」2）：schema 文件 `crates/rsword/schema/bind/*.json` 与 genoffice `packages/docx-engine-rs/src/protocol.d.ts` 由同一源生成，CI 校验不漂移（M8 8.1 的 wasm 校验同款）。`docs/03` v3.3：§8.2 增 M5–M7 登记的新操作、新增协议一章、§1.1 / §3.5 / §6.3 / §8.3 的「第一阶段」措辞改完成态——**冻结文档改动需项目负责人批准**（「待决」8） | 00 §0.2, MOD-01, MOD-06, MOD-11, MOD-13, EDIT-01, EDIT-02, EDIT-05, EDIT-06, RES-01, SAVE-07 | `spec/21` 评审通过并合入（评审是 9.2–9.4 的**关口**）；schema 与 `.d.ts` 由同一源生成且 CI 校验；`docs/03` v3.3 合入 |
| 9.2 | **模型 JSON 投影**（`bind/native/json.rs`、`bind/native/schema.rs`）：独立投影层——不在 `model/` 类型上直接 `derive(Serialize)`：模型持 `NodeId` / `Range<u32>` / arena 引用，`Display` 里有节点引用，`RevisionMeta` 里有承载节点。`model_json!` 宏按「Rust 字段 → JSON 字段（可选换名 / 转换函数）」一张表同时展开 `to_json`、schema 条目与门 1 的「不丢字段」checklist 测试（见「实现约定」）；`Option` / `bool` 沿用 `set_some!` / `set_if!`。覆盖：`main` 块序列（`Text / Table / Image / Protected` + `display`）、`Inline`（`Run / Field / Atom`）、`Segment`（`kind`、`utf16Len`、`display`）、表格（`TableBlock / Row / Cell` 声明值 + `tblPrEx`、`grid`）、`SdtInfo`、`sections`（`SectionInfo` 声明值；继承走 `resolveSections`）、`hfParts`、`footnotes / endnotes / comments`（各自 `blocks`）、`styles / numbering / theme / fontTable / settings / sources`（`MOD-10` 声明值）、`media[]`（`{ id, part, mime, kind, byteLen }`，**无字节**）、`chartParts` / `diagram` 的 `display`、`inks`、`fields`（`FieldIndex`：形态 / 指令 / 策略 / 结果范围）、`spans`（`SpanIndex`：种类 / 名字 / 两端 `{ para, offset }`）、`revisions`（7.1 的 `RevisionIndex`）、`warnings`。**不进 JSON**：dataURL；`originalXml / rawPPr / rawRPr / rawTcPr / rawTrPrs / sdtShell / docxIndex`（`nodeId` 替代）；TS 的 `label / previewText / fieldDisplay / textboxes / strayRuns / imageMeta / richParas / HfParagraph` 半解析形态；控制字符折叠（`\f \v \n ‑ PAGE_MARK`——编辑器按 `SegmentKind` 渲染）；px 换算；`sameStyle` run 合并；`floatSide` / 环绕侧猜测；碰撞位移；band；画布分栏；`internal.documentXml` / `extras.elements`。增量：`document(id, { blocks?: [nodeId] })` 按块取（由 `MutationResult.affectedBlocks` 驱动）；首版允许整份重取，门 5 量过再决定 | MOD-01–MOD-13, RES-05, BIND-02 | 门 1 全部；`tests/bind_json.rs`：全语料 schema 校验 + 往返幂等 + 字段 checklist；38 份 hostile `document()` 成功，`warnings` 与降级块一一对应（`MOD-12`）；`m6-*` 带图语料 JSON 里不出现 `data:` 前缀 |
| 9.3 | **`EditOp` / `EditContext` / `MutationResult` 的 JSON 与映射表**（`edit/mod.rs` 的 serde、`build/props.rs` 给 patch 类型加 serde、`docs/11` 第二张表）：`EditOp` 与 `NewBlock / NewInline / NewAtom / NewField / NewComment / NewImage / NewChart / NewInk` 及属性表 `*Patch` 全部 `Serialize / Deserialize`（`#[serde(tag = "op", rename_all = "camelCase")]`；属性 patch 的 serde 由 `build/props.rs` 生成——`PROP-06` 的 diff / patch 形态已在，只加派生）；`edit_op_json!` 宏给每个变体一条 JSON 往返测试并生成 `BIND-03` 的变体清单（见「实现约定」）。**映射表**（编辑器今天调的 TS 助手 / `SaveBlock` / `SaveOptions` → `EditOp`；9.6 逐条落地）见下文「TS 助手 → `EditOp` 映射」。表里两个**缺口**是 M9 内新增的引擎能力：① `NewBlock::WordArt { text, preset, extent, anchor, fill, outline }`——TS `buildWordArtParagraphXml` 发 VML `v:textpath`，M7 明确不做（`spec/18`「不在 M7」）；M9 发 DrawingML `wps:wsp` + `a:bodyPr` 文本效果（Word 2010+ 形态，Strict 包也合法），登记「超过 TS」；② `SetChartData { …, updateWorkbook: bool }`——TS 用 `patchChartWorkbookXlsxBase64` 改内嵌工作簿的 Sheet1 后经 `partBinary` 写回；M9 让 `SetChartData` 自己用 6.6 的 xlsx 生成器重写工作簿（`ReplacePartBytes` 内部调用），编辑器不再碰 xlsx。5.7 今天只在 `save/options/decl.rs` 内部存在的四个声明 part 计划提升为公开 `EditOp`：`AppendNumbering / UpsertStyle / SetTheme / SetSources`。**逃生口**：`InsertBlock{Xml}` / `ReplacePartXml` 保留，每次使用记 `BIND_XML_ESCAPE` 诊断 + 会话计数（门 2 归零） | EDIT-01, EDIT-03, EDIT-04（撤销）, EDIT-06, PROP-06, SAVE-05, SAVE-07, BIND-03 | `edit_op_json!` 展开的往返测试全绿；映射表每行在 9.6 有勾选；两个缺口各有 XPath 验收 + 走 `real_edits` 流程让 Word 打开（WordArt 可见；图表「编辑数据」能打开工作簿且数据一致）；`AppendNumbering / UpsertStyle / SetTheme / SetSources` 的验收 = 5.7 既有测试改调公开操作后不变 |
| 9.4 | **会话、媒体句柄、`resolve` 查询与部件读取**（`crates/rsword-js` 重做为有状态；`bind/native/session.rs`、`bind/native/resolve.rs`）：wasm 侧 `SessionTable: BTreeMap<SessionId, EditSession>`；导出 `open / close / document / apply / save / media / addMedia / resolveRuns / resolveParas / resolveCells / resolveSections / resolveTable / partBytes / nodeXml / diagnostics / version`，同形导出用 `bind_export!` 收拢，五个 `resolve*` 用 `resolve_query!`（见「实现约定」）。**`save(id, ops, options)`**：克隆会话（`EditSession: Clone`——DOM arena、Span / Field / Revision 索引、媒体去重表都是纯数据；最大真实文档上量克隆成本，> 50 ms 则改为「apply 后保存、失败按 `EDIT-05` 快照回滚、成功也回滚到保存前」——对调用方语义相同）→ `apply_all(ops, ctx)` → `save_with(options)` → bytes；读会话不变；编辑器写盘成功后 `close(old)` + `open(newBytes)` 换会话（对应今天的「保存后重解析」）。**媒体**：`media(id, mediaId)` 返回字节 + `mime` + `kind`；编辑器 `MediaCache`（`Blob` URL，按会话失效并 `URL.revokeObjectURL`）；`kind = Metafile \| Tiff` → 惰性调 `metafileToDataUrl / tiffToDataUrl`（转换服务留 TS，`docs/03` §3.5；M8 8.1 的 `DATA_URL_PATHS` 表随 compat 删）；`addMedia` 供图片 / 墨迹 / 图表插入（`EditSession::add_media` 已有，6.7）。**`resolve` 批量查询**：入参 `[nodeId]`，出参每个 id 一条有效值 + `Provenance`（`RES-03`）；一次 wasm 调用批量，不给编辑器逐 run 调用的接口。**内存**：会话持有原字节 + arena + 索引；`close` 释放；wasm 线性内存不缩——门 5 量常驻与峰值 | EDIT-01, EDIT-05, PKG-05, RES-01–RES-12, BIND-01, BIND-04–BIND-10 | `tests/bind_session.rs`：open / apply / save / close 状态机；`Err` 后 `document()` 逐字节相同；两个会话互不影响；`media` 对全语料每个 `MediaId` 取回字节与 zip 条目相同；`save` 的函数式语义（保存后 `document()` 不变）；`TEST-07` 的 100 条序列经 JSON 往返走协议后三条 oracle 仍成立；`resolveRuns` 对全语料与 `Resolver::run` 逐字段相等；克隆成本与常驻内存数字进 `docs/05` |
| 9.5 | **渲染器接管排版启发式**（genoffice `apps/docs/src/renderer/{pagination*,editor/*}`；12 条见下文「排版启发式搬家表」）：每条——先有 9.0 的 fixture 与 M8 基线，再在编辑器 / 分页器里用模型事实 + `resolve` 查询重算，搬完像素零 diff；第 6 条（页宽 4680 twips 猜测）是唯一允许结果变化的：从猜测变成 `resolveSections` 给的真实栏宽，fixture 里构造「猜错」的用例并记录预期变化。`KNOWN_DIFFS.md` 里两条「渲染器接管后删除」随 compat 一起删（9.7） | MOD-11, RES-07, RES-08, RES-10, BIND-06 | 门 3 的 12 份专项 fixture 零 diff（第 6 条按记录的预期变化）；`docs-visual` 5 份零 diff；`resolveSections / resolveTable / resolveRuns` 在编辑器里各有消费者 |
| 9.6 | **编辑器迁移**（genoffice `apps/docs`；按路径拆提交，双轨期保 M8 路径可用）：**a. 文档生命周期**（`file-actions.ts` / `doc-state.ts` / `review-actions.ts`）：`DocState.parsed: ParsedDocFull` → `DocState { session: SessionId, model: DocumentJson }`；打开 = `open + document`；新建 = `blank()` 字节 → `open`；保存 = `pmDocToEditOps(pmDoc, model)` → `save(session, ops, options)` → 写盘 → 换会话 → `modelToPm` 重建并按 `nodeId` 保留光标 / 撤销栈（今天按 `docxIndex` 重定位的逻辑改成按 `nodeId`，失效时退回块序 + 文本指纹对齐）；比较文档 = 第二个会话；崩溃恢复副本 = 同一 `save`（函数式，随时可调）；关标签 = `close`。**b. `convert.ts` → `model-to-pm.ts` + `pm-to-ops.ts`**：`blocksToPmDoc` 改读 `DocumentJson`（块 / 内联 / 段 / display）；PM 节点 attrs 存 `nodeId`；run 合并、控制字符、`ProtectedKind` → i18n 标签在这里；`pmDocToSavePlan` 的签名差分骨架保留，产出改为 `EditOp` 列表（`ReplaceInlines / SetParaProps / InsertBlock / DeleteBlock / MoveBlock` + 表格与文本框专项）；`inlineToRuns` 产出 `NewInline`。**c. 审阅**：`revisions.ts::applyRevisionsBy / acceptAllRevisions / rejectAllRevisions`（今天在编辑器侧改 PM 文档）→ `AcceptAll{author} / RejectAll / AcceptRevision / RejectRevision`（7.4）+ 重取模型；`collectRevisions` 改读 `revisions` 索引；批注 → `AddComment / RemoveComment / SetCommentText`；追踪模式 → `EditContext.trackChanges`。**d. 页面与节**：布局 / 设计 / 页眉页脚面板 → `SetSectionProps / SetHeaderFooter / LinkHeaderFooter / SetPageColor / SetWatermark / InsertSectionBreak / DeleteSectionBreak`；`pagination-sections.ts` 读 `resolveSections`。**e. 插入**：图片 / 图表 / 文本框 / 形状 / 线 / WordArt / 公式 / 题注 / 目录 / 索引 / 脚注 / 书签 / 超链接 / 复选框 / 内容控件 / 墨迹 → 映射表对应操作；AI 工具（`ai/protocol.ts`、`ai/commands.ts`、`ai/tools.ts`）从「生成 XML 片段」改为「生成 `EditOp`」，`create_document` 走 `blank()` + ops。**f. 样式 / 编号 / 主题 / 参考文献 / 保护**：`numbering-actions`、`doc-style-css`、`text-style-resolve`、`ProtectDialog`、sources → `AppendNumbering / UpsertStyle / SetTheme / SetSources / SetDocumentSettings` + `resolveRuns / resolveParas`（样式链不再在编辑器里算）。**g. 只读块渲染**（`protected-render.ts`）：按 `ProtectedKind` + `display`（`Chart / SmartArt / Formula / Ole / Rule / FieldBlockResult`）；不再有 `originalXml` 与英文 `label`。**h. 其他消费者**：`packages/file-parse`（`document()` 坐标流拼文本）、`apps/markdown` docxExport（`blank()` + ops）。**i. 测试**：`apps/docs/tests` 49 个引擎相关测试改写；新增 `pm-to-ops` 单测（每类 PM 变更 → 期望 `EditOp`，表驱动） | BIND-01–BIND-10, EDIT-03 | 门 2、门 3；`docs/11` 四张表全部勾选；`apps/docs/src` 无 compat 字段引用（门 4 的 grep）；每条路径一个提交，提交信息引用 `docs/11` 行号 |
| 9.7 | **删除 `compat_ts` 与 TS 引擎；语料与工具改造**：**rsword**：删 `bind/compat_ts/`、`KNOWN_DIFFS.md`、`tools/diff-parse`、`tests/compat.rs / compat_table.rs / save_blocks.rs`、`fixtures/fieldgen` 的 TS 夹具与 `TocOptions.ts_shape`（7.8）、`save/options/` 里只为 TS `SaveOptions` 存在的翻译（5.6 的节 / 页眉页脚 / 水印等翻译已在 9.3 成为公开操作，`SaveOptions` 收缩到 `BIND-04` 五项）；`spec/10` 全部条目标 `[已撤销]`，`spec/00` 表的 `COMPAT` 行注明；`docs/01` 加「历史文档：TS 引擎已于 M9 删除」头注。**语料**：docx 全部保留（799 + 266 + 38）；`*.expected.json` / `*.save.<k>.json` 按「待决」5 删除，`corpus/README` 记用途与最后一个含它们的提交；新增 `*.model.json` 快照（9.2）与 `*.ops.json`（`TEST-07` 固化序列的协议形态）；`tools/export-golden` 收缩为 docx 导出器（`TEST-02` 改写：只产 docx，不再跑 TS `parseDocx / saveDocx`），并在删除 TS 引擎**之前**跑通一次。**genoffice**：删 `packages/docx-engine/src` 的 `parse*.ts / patch.ts / scan.ts / zip-load.ts / ooxml-normalize.ts / generate.ts / text-patch.ts / resource-cleanup.ts / blank.ts / section.ts / notes.ts / watermark.ts / ink.ts / sources.ts / symbol-fonts.ts / font-table.ts / theme.ts / chart.ts（part 补丁部分）/ math.ts（转换器部分）/ xml-utils.ts`；`packages/docx-engine/tests` 87 个文件按「待决」6 处置（`tests/helpers/build-docx.ts` 留作语料生成器）；`packages/docx-engine` 改名 `packages/docx-engine-rs` 并只含协议 + 生成类型 + 媒体转换服务（`metafile.ts / tiff.ts`）+ `protection.ts` + `list-markers.ts`（「待决」3）+ `bibliographyLine / citationText`；`types.ts` 换成生成的 `protocol.d.ts`；M8 的引擎开关与分派层删除。**CI**：rsWordParser 去掉八道 `diff-parse` 与 `--corpus` 步骤，换成 `*.model.json` 快照比较 + schema 校验 + `fuzz_bind`（fuzz.yml）；genoffice 去掉 TS 引擎测试步骤 | 00 §0.2, COMPAT-01（撤销）, TEST-01, TEST-02, TEST-03（改写）, TEST-10 | 门 4；两仓库 CI 绿；`docs/05` 的「能力矩阵 / 公开 API 边界」重写为协议视角 |
| 9.8 | **性能、模糊、文档与 M9 门**（`fuzz/fuzz_targets/fuzz_bind.rs`、`benches/bind.rs`、`docs/`）：`fuzz_bind`——`arbitrary` 生成 JSON 喂 `apply` / `document` / `resolve*`：不 panic、`Err` 不改状态（`document()` 逐字节相同）；`TEST-07` 序列改走协议 JSON（PR 100 条 / nightly 1,000 条，`spec/18` 7.9 同款）；`benches/bind.rs`：open / document / apply / save / media 在最大三份真实文档上；JSON 体积对比表（M8 `ParsedDoc` vs M9 `document`）。文档：`docs/03` v3.3 收尾（§12 的 M8 / M9 完成态）、`spec/00` 表、`spec/11` TEST-10 M9 行、`docs/04` §18 逐条进度、`docs/05` 全文重写、`README` / `CLAUDE.md`（去掉 TS 差分命令与「TS 不是权威」章节改为历史说明；「四条不变式」不变） | TEST-06, TEST-07, TEST-10 | 门 5、门 6；文档同步；`docs/05`「明确未实现」只剩非 M9 项 |

建议顺序：9.0 → 9.1（协议评审是**关口**，其后才动代码）→ 9.2 / 9.3 / 9.4 并行（rsword 侧；可在 M8 收尾期就开）→ 9.5 与 9.6a–b 并行
（genoffice 侧；9.5 依赖 9.4 的 `resolve` 查询）→ 9.6c–i 按路径逐条提交 → 9.7（最后，门 2 / 3 绿之后）→ 9.8。**双轨期**：9.6 期间编辑器
同时能跑 M8 路径与原生路径（沿用 M8 的开关机制），逐路径切换、逐路径删旧代码；9.7 一次性收尾并删开关。

### TS 助手 → `EditOp` 映射（9.3 定稿，9.6 落地；`docs/11` 第二张表的初稿）

| 编辑器今天调的 TS 助手 / `SaveBlock` / `SaveOptions` | `EditOp` / 模型 / 去处 |
| --- | --- |
| `pmDocToSavePlan` 的 `generated` 块 | `ReplaceInlines` + `SetParaProps`（追踪时引擎做坐标流 diff，7.2） |
| 缺失的 `original` / `original` 重排 | `DeleteBlock` / `MoveBlock` |
| `generateTableModelXml` / `patchTableCellTexts` / `pmTableToModel` | `NewBlock::Table` + 格内 `ReplaceInlines` + `SetCellProps / SetRowProps / SetTableProps` + 行列操作（M3） |
| `generateTocFieldXml` / `generateIndexFieldXml` / `generateCaptionXml` | `InsertBlock{Field(Toc / Index)}` / `RegenerateBlockField`（页码来自分页器）/ `NewBlock::Caption`（7.8） |
| `patchFieldParagraphXml` | `SetFormText` / `SetLinkTarget` / `ToggleCheckbox` / `UpdateBlockField` |
| `buildTextboxParagraphXml` / `buildAnchoredTextboxParagraphXml` / `buildShapeParagraphXml` / `buildLineParagraphXml` | `NewBlock::Textbox / Shape / Line`（7.7） |
| `buildWordArtParagraphXml` | **缺口** → `NewBlock::WordArt`（9.3；DrawingML 形态，登记「超过 TS」） |
| `patchTextboxParas` / `patchTextboxSizes` / `patchTextboxHeights` | `SetTextboxContent` / 文本框内 `InsertText / DeleteRange` / `SetDrawingGeometry` |
| `patchShapeStyles` | `SetShapeStyle` |
| `patchDrawingExtent` / `patchImageParagraphXml` / `applyImageWrap` / `applyImageZOrder` | `SetDrawingGeometry` / `ReplaceImageMedia` / `SetDrawingWrap` / `SetDrawingZOrder`（归一是 `SaveOptions.normalizeZOrder`） |
| `mergePPrFormat` / `setPPrChange` / `stripPPrChange` | `SetParaProps`（`pPrChange` 由 `EditContext.trackChanges` 决定） |
| `mathParagraphXml` / `latexToOmml` / `patchMathTokens` / `mathTokensOf` / `ommlToMathML` / `ommlToLatex` | `NewBlock::MathPara` / `InsertAtom Math{Latex}` / `SetMathTokens` / 模型 `FormulaDisplay.tokens / mathml / latex`（6.5） |
| `applySectionSettings` / `applySectionStartType` / `applyPageNumType` / `readSections` / `readSectionSettings` / `readPageColor` / `DEFAULT_SECTION` | `SetSectionProps` / `SetPageColor` / 模型 `sections` + `resolveSections`（隐式节 = `owner: Implicit`） |
| `SaveOptions.header / footer / *First / *Even / sectionHf / titlePg / evenAndOddHeaders / watermark` | `SetHeaderFooter` / `LinkHeaderFooter` / `SetSectionProps{titlePg}` / `SetDocumentSettings` / `SetWatermark`（5.5） |
| `SaveOptions.comments / footnotes / endnotes`（权威列表） + `nextNoteId` | `AddComment / RemoveComment / SetCommentText`；`InsertAtom NoteRef` / `SetNoteContent` / `RemoveNote`；id 由引擎分配（`EDIT-06`） |
| `SaveOptions.numbering / styleUpserts / themeFonts / themeColors / sources` | `AppendNumbering / UpsertStyle / SetTheme / SetSources`（5.7 的内部计划公开化，9.3） |
| `SaveOptions.protection / writeProtection / removePersonalInfo` | `SetDocumentSettings{…}`；`hashProtectionPassword / verifyProtectionPassword` **留 TS**（WebCrypto；不是文档语义，「待决」4） |
| `SaveOptions.inks` | `RemoveInks` + `InsertInk`（6.8） |
| `SaveOptions.partXml / partBinary`、`findChartWorkbookPath / readDocxPartBase64 / patchChartWorkbookXlsxBase64 / patchChartPartXml / parseChartPartXml` | `SetChartData{updateWorkbook}`（**缺口**，9.3）/ `NewBlock::Chart` / 模型 `chartParts[].display`；`BIND-09 partBytes` 只读 |
| `lumHex` | `ChartDisplay.palette`（`RES-05`） |
| `bibliographyLine / citationText` | **留 TS 渲染层**（引文格式化不是文档语义）；模型给 `sources` 声明值 |
| `PAGE_MARK / TOTAL_PAGES_MARK` | 页眉页脚块里的 `Inline::Field`（PAGE / NUMPAGES 原子），编辑器按字段渲染 |
| `BLANK_BULLET_NUM_ID / BLANK_ORDERED_NUM_ID` / `LINE_KINDS` / `TABLE_HEADER_FILL` | `blank()` 常量 / `LineKind` 枚举 / 编辑器常量 |
| `list-markers.ts`（`computeListMarkers / formatNumber / markerTabAdvance / bulletMarkerScale / customEnumItems`） | **「待决」3**：留渲染器，或进 `resolve::list_markers` 作为 `BIND-06` 的一个查询 |
| `readWatermarkText / parseNotesXml / parseSourcesXml / parseFontTable / symbol-fonts / theme.ts` | 模型字段（`settings / notes / sources / fontTable / theme`）与 `RES-05` |
| `SaveBlock kind:'xml'`、`SaveOptions.partXml` | 逃生口 `InsertBlock{Xml}` / `ReplacePartXml`（计数，门 2 归零） |

### 排版启发式搬家表（9.5；每条一份 fixture + M8 基线 + 搬后零 diff）

| # | 启发式（TS 位置） | 新家 | 数据来源（模型 / `resolve`） |
| --- | --- | --- | --- |
| 1 | `allowOverlap=0` 碰撞位移（tdf#134114；`imageMeta` → `compat_ts` `image.wrap / offset`） | 分页器的浮动对象布局 | `AnchorGeom.allow_overlap` + 同段兄弟锚点几何 |
| 2 | 画布溢出文本分栏 `colGeom`（`extractLockedCanvas`；`compat_ts/diagram.rs`） | 画布渲染 | `DiagramDisplay` 子形状 EMU + `chOff / chExt` |
| 3 | WordArt 字号压缩 `widthPt / (0.62 × len)`（`vmlWordArtBox`） | 形状渲染 | `VmlDisplay.textpath` + `style` 宽高 |
| 4 | 连线抓取带（`wrapSquare` + 连线 `prst`） | 形状渲染 | `ShapeDisplay.prst / xfrm` |
| 5 | `wrapTopAndBottom` band（`bandTopPx / bandBottomPx`） | 分页器 | `AnchorGeom.wrap = TopAndBottom` + 几何 + `rel_v` |
| 6 | 页宽 4680 twips 猜测（环绕侧 `bothSides`；表格 `floatSide`） | 布局，用真实栏宽（**允许结果变化**） | `resolveSections` 的 `pgSz / pgMar / cols`（`RES-10`） |
| 7 | 表格样式填充 / 粗体 / 颜色烘进单元格（`applyTableStyleDisplay`；`compat_ts/table.rs` 与 `styles.*.tableDisplay`） | 表格渲染查 `resolveTable` | `TableView`（`RES-08`）条件格式与 `Provenance` |
| 8 | `balanceSingleByteDoubleByteWidth` 字距缩放（`KNOWN_DIFFS`） | 行度量 | `CompatFacts.balance_single_byte_double_byte_width` + `@genoffice/font-metrics` |
| 9 | `withCharIndents` 字符单位缩进换算（`KNOWN_DIFFS`） | 行度量 | `ParaProps.ind.*_chars` + `resolveRuns` 的字号 |
| 10 | z-order 归一（`normalizeImageZOrders`；`imageZOrderNormalized`） | 显式 `SaveOptions.normalizeZOrder`；渲染按 `relative_height_raw` 排序 | `AnchorGeom.relative_height_raw` |
| 11 | 页眉页脚里浮动表延后、文本框段落提出（`hfParagraphs`；`COMPAT-05`） | 页眉页脚渲染 | `HfPart.blocks`（表格与文本框就是块 / 段） |
| 12 | 相邻同格式 run 合并 `sameStyle`、控制字符折叠、passthrough 决策树与英文 `label` | 编辑器 `model-to-pm`（视图层） | `Run` 物理 run + `SegmentKind`；`ProtectedKind` 是 i18n key |

## 分层决策（实现前定死）

1. **协议是 `Document`（`MOD-01`）与 `EditOp`（`docs/03` §8.2 + `docs/04` §8 登记）的 serde 投影，不是第三个模型**：JSON 字段名 =
   Rust 字段名的 camelCase；不为编辑器方便新造语义字段——编辑器要的派生值（px、合并 run、标签文字）自己算或问 `resolve`。
2. **规范状态与投影的边界不变**（`docs/03` §6.8）：JSON 是投影的投影；编辑器**永远**不能把 JSON 改一改送回来——只有 `EditOp` 能改
   文档。`nodeXml` 是调试出口，生产代码 lint 禁用。
3. **会话有状态、保存函数式**：读会话在 `open` 之后只被换会话改变；`save(ops)` 在克隆上做，失败不影响读会话；编辑器写盘成功才换会话。
   这保住了今天「PM 文档是编辑期真相、保存时差分」的架构，撤销栈与崩溃恢复副本都不动。实时 `apply`（每次击键一条操作、引擎成为编辑期
   真相）另立里程碑。
4. **id 会话内稳定、跨会话无意义**：`nodeId`（arena 稳定，`MOD-13`）、`revisionId`（7.1）、`spanId / fieldId / mediaId`；编辑器不得
   持久化 id；换会话后按块序 + 文本指纹重对齐（今天按 `docxIndex` 重定位的逻辑换个键）。
5. **偏移单位仍是 UTF-16**（`docs/03` §8.1 留给 M9 的决定：**不改**——ProseMirror 位置也是 UTF-16 code unit，改单位只多一次换算）。
6. **排版启发式只搬家、不顺手改进**：搬家的验收是像素零 diff（门 3）；想改进先搬完再立项。第 6 条是唯一例外（猜测变事实）。
7. **媒体不进 JSON、转换不进 Rust**：`MediaId` + 按需字节；metafile / TIFF 转换服务留 TS（`docs/03` §3.5 冻结）；编辑器缓存 Blob URL、
   随会话失效。
8. **不是文档语义的东西不进引擎**（`docs/03` §1.2）：密码哈希、引文格式化留 TS；列表编号显示计算归属见「待决」3。
9. **逃生口有名有姓**：`InsertBlock{Xml}` / `ReplacePartXml` 保留，每次使用记诊断 + 计数，M9 门要求为 0；M9 之后新功能若想走逃生口，
   先登记为债。
10. **TS 差分退役，自快照上位**：`*.expected.json` 的价值随 compat 消失；回归网换成 rsword 自己的 `*.model.json`（改动带理由）+
    `TEST-07` + Word fixture（`fixtures/{resolve,revisions,word-ops}` 不变——它们对照的是 Word，不是 TS）。
11. **删除是最后一步且不可逆**：9.7 只在门 2 / 3 绿之后做；不留 M8 的引擎开关（双路径维护成本不可接受）；`export-golden` 的 docx 导出器
    在删 TS 引擎之前跑通。
12. **冻结文档的修订**：`docs/03` v3.2 的「第一阶段」措辞（§1.1、§3.5、§6.3、§8.3 末段）随 M9 改为完成态；新操作清单进 §8.2；协议成一章。
    分层与六个核心类型不变，所以是 v3.3 不是 v4（「待决」8）；改动需项目负责人批准。

## 实现约定：多用声明宏（用户要求，2026-09-05 与 2026-09-07 再次强调；与 `spec/14` / `spec/16` / `spec/17` / `spec/18` / `spec/19` 同一条）

M9 的 Rust 侧是历次里程碑里**同形重复最多**的一个：几十个结构要投影成 JSON、约 50 个 `EditOp` 变体要往返、十几个 wasm 导出同形、
五个 `resolve` 查询同形。判断标准仍是**同一形状重复三次以上就收成 `macro_rules!`**；每个宏同时展开实现、schema 与测试三样，让「投影不
丢字段」「变体都能往返」「导出都会映射错误」不靠人记：

- `model_json!`：一张「Rust 类型 → { JSON 字段 ← Rust 字段 [via 转换函数] }」的表，展开三样：`impl ToJson`（`set_some!` / `set_if!`
  处理 `Option` / `bool`）、`schema()`（该类型的 JSON Schema 片段，字段名与可选性来自同一张表）、`#[test] json_fields_cover_struct`
  （用 `serde_json::to_value` 的键集与表里的字段集比对——`Document` 及其子结构每个一条，门 1 的「不丢字段」）。**不**在 `model/` 类型
  上 `derive(Serialize)`：`NodeId` / `Range<u32>` / arena 引用不该原样出去。
- `edit_op_json!`：`EditOp` 变体清单，展开：每个变体一条 JSON 往返测试（构造 → `to_string` → `from_str` → 相等）、`BIND-03` 的变体
  与字段清单（`spec/21` 的表由它生成后人工校对）、`BIND_XML_ESCAPE` 的计数点（只在 `InsertBlock{Xml}` / `ReplacePartXml` 两行打开）。
  `EditOp` 本身用 `serde` derive（结构简单、无 arena 引用），宏只管测试与清单。
- `bind_export!`：wasm 导出「`session_id` + JSON 字符串入 → JSON / `Vec<u8>` 出 + `Error → JsValue { code, message }` 映射」，
  十几个同形；展开导出函数与「不存在的 `session_id` → `BIND_NO_SESSION`」的单测。M8 8.1 的 `wasm_export!` 是它的无会话前身，M9 合并。
- `resolve_query!`：五个 `resolve*`（run / para / cell / section / table）同形——「`[nodeId]` 入 → 每个 id 一条 `{ value, provenance }`」，
  展开导出、JSON 投影与「对全语料与 `Resolver::*` 逐字段相等」的测试。
- 沿用：`named_enum!`（`ProtectedKind / SegmentKind / MediaKind / RevKind / SectType / LineKind …` 的 `as_str` 直接就是 JSON 字符串，
  `schema_enum!` 从同一名字表生成枚举的 schema）、`set_some!` / `set_if!`、`xpath_asserts!`、`fixture_tests!`、`oracle_tests!`（`TEST-07`
  走协议）。
- **不上宏**：`pm-to-ops`（TS）；三个新操作（WordArt、工作簿同步、声明 part 操作公开化——形状各异）；`SessionTable` 的生命周期（两个函数）。
- 宏带文档注释与 ```ignore 用例；跨模块用 `macro_rules!` + `pub(super) use`，展开里写 `$crate::…` 全路径；会把函数定义藏起来、让人跳不到
  声明处的，用共享模块而不是宏。

TS 侧不适用宏，但同形重复用数据表驱动：`pm-to-ops` 的「PM 变更种类 → `EditOp`」表、`model-to-pm` 的「`SegmentKind` → 渲染」表、
`MediaCache` 的「`MediaKind` → 转换器」表，各带一条「枚举值全覆盖」的单测（TS 类型从 schema 生成，枚举穷尽由 `never` 检查保证）。

其余约定照旧：树遍历写成**迭代**；属性容器只走 `plan_apply_*`；一个任务一个提交 `m9.<n>: 英文摘要 (SPEC-ID…)`（genoffice 侧前缀 `rsword:`）；
提交前同步 `docs/04` §18 勾选、§8 偏差表、`docs/05` 数字。

## 从 M0–M8 带过来的债（M9 内解决）

| 债 | 位置 | 解决任务 |
| --- | --- | --- |
| `compat_ts` 12,684 行与 `KNOWN_DIFFS.md`（「负担性代码，删除期限 M9」） | `bind/compat_ts/` | 9.7 |
| `KNOWN_DIFFS.md` 两条「渲染器接管后删除」（`balance-dbcs-spacing`、`charIndents`） | `KNOWN_DIFFS.md` | 9.5 #8 / #9 |
| `extras.chartParts`、`runs[].image.xml`、`richParas`、`HfParagraph` 等原字节直出 / TS 折行投影 | `compat_ts` | 9.2（不进 JSON） |
| `docs/03` §8.1「M9 若前端协议改变再考虑标量或字素单位」 | — | 决策 5（不改，记 `docs/04` §8） |
| `spec/18` 待决 5：`docs/03` §8.2 之外的新操作收进下一版 | `docs/03` §8.2 | 9.1 / 9.8 |
| M8 的引擎开关与双引擎分派层 | genoffice `packages/docx-engine/src/engine.ts` | 9.7 删 |
| M8 包装层的 `DATA_URL_PATHS` metafile 转换表 | genoffice `packages/docx-engine-rs` | 9.4 改为按 `MediaId` 惰性转换 |
| `save/options/` 的 TS `SaveOptions` 翻译层（5.6 / 5.7 / 6.6–6.8） | `save/options/` | 9.3 公开为 `EditOp`、9.7 收缩到五项 |
| `TocOptions.ts_shape` 与 `fixtures/fieldgen` 的 TS 夹具（7.8） | `span/field/generate/` | 9.7 删 |
| `compat_ts::parsed_doc` 经 `serde_json::Value` 中转（M8 8.6 若未改） | `bind/compat_ts/mod.rs` | 随 9.7 消失 |
| `docs/06` toggle 未决（`strike` 一族桌面版复核） | `resolve` | 不挡 M9：`resolveRuns` 发的是校准过的规则；复核仍是项目负责人择机 |
| `docs/01` 作为兼容目标、`CLAUDE.md`「TS 不是权威」章节 | `docs/01`、`CLAUDE.md` | 9.7 / 9.8 标历史 |

## 基线与复用

| 来自 | 复用什么 | 在哪个任务 |
| --- | --- | --- |
| M7 7.10 / M8 8.1 `crates/rsword-js` | wasm 构建、产物提交与 CI 校验、错误映射、`wasm_export!` | 9.4 |
| M7 7.1 `RevisionIndex` / `RevisionId` | `revisions` JSON 与 Accept / Reject 引用 | 9.2 / 9.6c |
| M7 7.9 `ModelFingerprint`、`TEST-07` 生成器、`fuzz_edit` | 协议往返 oracle、`fuzz_bind` 的种子与断言 | 9.4 / 9.8 |
| M6 6.7 `EditSession::add_media`、6.6 xlsx 生成器 | `addMedia`；`SetChartData{updateWorkbook}` | 9.4 / 9.3 |
| M5 5.6 / 5.7 `save/options/*` | 翻译逻辑 → 公开 `EditOp` | 9.3 |
| M4–M6 显示模型（`Display`、`ChartDisplay`、`DiagramDisplay`、`FormulaDisplay`、`VmlDisplay`） | JSON `display` | 9.2 |
| `resolve` `RES-01`–`RES-12`（含 `TableView`、节视图、`Provenance`） | `resolve*` 查询 | 9.4 / 9.5 |
| 属性表生成器 `build/props.rs` | patch 类型的 serde 派生 | 9.3 |
| `model/macros.rs::named_enum!`、`bind/compat_ts/json.rs::set_some! / set_if!` | 投影层的两个基础宏（`json.rs` 搬到 `bind/native/` 后 compat 才能删） | 9.2 |
| genoffice `convert.ts` 的签名差分骨架、`e2e/docs-visual` 机制 | `pm-to-ops`、启发式基线 | 9.6b / 9.0 |
| M8 `docs/10` 审计表 | 用户可见差异的既有登记（M9 不再产生新的 TS 差异） | 9.6 |

## 依赖与被阻塞

| 事项 | 状态 |
| --- | --- |
| M8 完成并缺省 `rs` | **前置** |
| 协议评审（9.1）与 `docs/03` v3.3 批准 | 项目负责人；9.2–9.4 的 API 面在它之后才定 |
| 真实 Word：WordArt 新建（9.3 缺口 ①）、工作簿同步后「编辑数据」可打开（缺口 ②）——走 `real_edits` 流程 | 项目负责人 |
| 列表编号显示计算归属（「待决」3）影响 9.6f 与 9.7 的保留清单 | 待决 |
| 门 2 的分母（用户可达写路径清单） | 9.0 从功能区 / 快捷键表 / AI 工具清单生成 |
| Linux CI 像素门 | 门 3 只能在 CI 判 |
| genoffice 侧评审与合并 | 项目负责人 |

## 不在 M9

- **实时 `apply`**（每次击键一条 `EditOp`、引擎成为编辑期真相）与协同编辑——决策 3 保留保存时差分；实时模式是独立里程碑。
- **分页 / 排版进引擎**（`docs/03` §1.2 永久不做）；`RegenerateBlockField` 的页码仍由分页器给。
- `.doc` / RTF / ODT；napi 形态；多线程 wasm。
- **`resolve` 的新规则**（toggle 复核、Wingdings 2/3 补全）——按 `docs/06` 择机，不在 M9 门。
- **引擎新能力**：除 9.3 的两个缺口与四个声明 part 操作公开化外不新增——比较文档、`moveFrom / moveTo` 生成、tracked `MergeCells`、
  VML 形状编辑仍按 `spec/18`「不在 M7」的清单留在后面。
- genoffice 其他应用（sheets / slides / pdf / markdown 的非导出部分）的架构。
- 编辑器 UI 重设计；只换协议不换交互。

## 风险提示（实现前确认）

1. **编辑器迁移量**：`convert.ts` 2,724 行 + 28 个文件用 `docxIndex` + `protected-render.ts` 1,501 + `ai/protocol.ts` 1,831——M9 最大的
   工作面，且在另一个仓库。按路径拆提交、双轨期保 M8 路径可用（决策 11 把删除放最后）。
2. **像素基线是「搬家不走样」的唯一证据**：只有 5 + 12 份 fixture；渲染差异可能藏在没被覆盖的文档上。`docs-visual` 的语料机制要能不依赖
   TS 引擎生成（9.0），且 12 份专项 fixture 要各自只触发一条启发式。
3. **JSON 体积与首屏**：去 dataURL 与 `internal.documentXml` 后主要体积在文本与 display；最大真实文档的 `document()` 若超百毫秒再上按块取
   （9.2 的增量接口）——先量再做，不要先做增量。
4. **wasm 会话内存**：一个会话 = 原字节 + arena + 索引；多标签页多会话；`close` 必须可靠（标签关闭 / 崩溃恢复 / 比较文档的第二会话都要调）；
   线性内存不缩。
5. **id 对齐**：换会话后 PM 文档里的 `nodeId` 全部失效，重对齐（块序 + 文本指纹）失败时退回整体重建——`blocksToPmDoc` 后的 `unchanged`
   检查路径已有类似兜底，沿用。
6. **协议漂移**：TS 类型从 Rust 生成并 CI 校验；手写 `.d.ts` 一定漂。`protocolVersion` 不匹配时启动拒绝，不静默。
7. **`resolve` 暴露后的性能**：编辑器对每个 run 问一次就是 O(n) 次 wasm 调用；`BIND-06` 只给批量接口；若仍慢，`document()` 顺带返回一份
   `resolveAll`——9.4 先量再定。
8. **删除的不可逆**：TS 引擎删除后 `TEST-02` 失去 TS 端；`export-golden` 的 docx 导出器改造在删除**之前**完成并跑一次；`*.expected.json`
   删除前 `corpus/README` 记最后提交号。
9. **两个仓库的原子性**：协议版本两边同时升；`RSWORD_COMMIT` + `protocolVersion` 在启动时检查。
10. **AI 生成路径**：AI 今天产出 XML 片段（`ai/protocol.ts`），改为 `EditOp` 意味着提示词与解析器一起改；逃生口计数（门 2）会最先被它撞上。
11. **两个缺口是 M9 内新增的引擎能力**（WordArt、工作簿同步），需要 Word 核对；不要让它们阻塞其他路径的迁移——可先走逃生口并登记，门 2 要求最后归零。
12. **`TEST-07` 走协议后变慢**：JSON 往返 × 1,000 序列；PR 只跑 100 条（`spec/18` 7.9 同款）。
13. **`EditSession: Clone` 的成本**：保存函数式依赖克隆；arena 与索引是纯数据但最大真实文档可能有百万节点。9.4 先量，超 50 ms 换「apply 后回滚」实现，
    对调用方语义相同。

## 待决（需要项目负责人拍板）

| # | 事项 | 建议 |
| --- | --- | --- |
| 1 | 协议形态：`document()` 首版整份 vs 一开始就带按块取 | 建议：首版整份 + `affectedBlocks`，9.2 量过再加按块取 |
| 2 | TS 类型生成：`serde` + `schemars` → JSON Schema → `json-schema-to-typescript`，还是 `ts-rs`；是否允许 rsword 加 `serde` derive 依赖（今天只有 `serde_json`） | 建议：`serde` + `schemars`（schema 同时服务 `fuzz_bind` 与门 1 校验）；TS 用 `json-schema-to-typescript` |
| 3 | 列表编号显示计算（`list-markers.ts`）归渲染器还是 `resolve::list_markers` | 建议：进 `resolve`（`docs/03` §7 把「编号」列为 resolve 职责；`lvlRestart` / `numStyleLink` / `startOverride` 是文档语义，跨文档序累加是纯函数）；渲染器只做文字与缩进 |
| 4 | `hashProtectionPassword / verifyProtectionPassword` 留 TS 还是进 Rust（`sha2` 依赖） | 建议：留 TS（WebCrypto 已有、非文档语义） |
| 5 | `*.expected.json` / `*.save.<k>.json`：删除 vs 归档到 `corpus/_ts-golden/` | 建议：删除（git 历史里有；死数据不该留在树里），`corpus/README` 记最后提交号 |
| 6 | genoffice `packages/docx-engine/tests` 87 个 TS 引擎测试 | 建议：随引擎删测试，留 `tests/helpers/build-docx.ts` 作语料生成器，`export-golden` 只产 docx |
| 7 | 保存模式：函数式 `save(ops)`（决策 3）vs 实时 `apply` | 建议：函数式；实时模式另立里程碑 |
| 8 | `docs/03` 的版本号：v3.3 增补 vs v4 | 建议：v3.3（分层与六个核心类型不变，只加协议章、新操作与完成态措辞） |
| 9 | 两个缺口（WordArt、工作簿同步）进 M9 还是登记为债走逃生口 | 建议：进 M9（否则门 2 归不了零） |
| 10 | `packages/docx-engine` 改名 `docx-engine-rs` 还是保留原名只换内容 | 建议：保留原名 `@genoffice/docx-engine`（editor 的 import 路径不动），M8 的 `docx-engine-rs` 并回来 |
