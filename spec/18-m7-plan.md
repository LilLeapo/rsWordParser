# SPEC 18 · M7 任务分解

> **已完成**（2026-09-08，11 个任务与六条门全部通过，并入 `main` = 32234ce；逐条进度见 `docs/04` §16）。
> 文中凡是把 **M8「编辑器切换到 Rust 引擎」**当作下游动机的地方（7.10 的立项理由、「不在 M7」的推给 M8 / M9 的清单），
> 都已随 2026-09-08 的范围改定作废：genoffice 退为测试基准，原 M8 撤销，接续的是 **M8′**（`spec/19`，原生协议与独立交付）
> 与 **M9′**（`spec/20`，Agent 接口层与文件级工具）。7.10 的绑定本身仍然有效，M8′ 8.4 在它之上改为有状态会话。

对应 `docs/03` 第 12 节 M7 行（「L4 + 保存：`EditOp` 全集、Span 变换、修订生成（`track_changes`）、保存前校验、部件写回、
`SaveBlock[]` 兼容；与 `saveDocx` 差分」，验收「现有 roundtrip / text-patch / table-edit / textbox-edit / ai-track-revisions
场景通过；随机编辑序列测试通过」）与 `spec/11` TEST-10 的 M7 行（「`COMPAT-08` XPath 等价全部通过；`TEST-07` 1,000 序列
无失败；`fuzz_edit`」）。格式同 `spec/12`–`spec/17`：每个任务给出产出、依赖的规范条目与完成定义（DoD）。顺序即建议的
实现顺序；同一编号内的子任务可并行。

那一行是 2026-09-04 写的；M1–M6 已经把其中大半提前做掉了：`EditOp` 今天有 36 个变体（M6 再加 6 个）、Span 变换与物化
（M2）、`SAVE-02` 校验与 part 写回（M1 / M2 / M5）、`SaveBlock[]` 兼容与保存差分（M1 起，M6 门第 2 条收口到 0 跳过）都已在
`main`。所以 **M7 的内容 = 历次里程碑明确推给 M7 的东西的总和**，按出处：

- **修订生成**：`EditContext.track_changes` 存在但被忽略（`docs/05`「明确未实现」）；表格 / 节 / 图表 / 图片 / 墨迹操作的修订
  （`spec/14` 3.7、`spec/16` 5.5、`spec/17`「依赖与被阻塞」）。
- **接受 / 拒绝修订**：`spec/08` EDIT-03 的整张表；`SPAN-07` 把 MoveFrom / MoveTo 暂按删除处理（`docs/04` §8）。
- **`EditOp` 全集里还没有的**：`InsertAtom`、`SetNoteContent`、`SetSdtContent`、`Accept / Reject`（`docs/03` §8.2）；跨段
  `DeleteRange`、`SPAN-10` 端点规则、跨 part `MoveBlock`（`XML-12` E′）。
- **新建 / 删除分节符**（`docs/04` §8 `SetSectionProps` 行：「分节符的增删留到 M7 与段落结构操作一起做」）。
- **块字段生成器**（`FLD-09`：TOC / SEQ / INDEX）与**公式作者方向**（`latexToOmml`、`InsertAtom Math`、token 编辑；`spec/17`
  「不在 M6」）。
- **既有绘图的编辑**（尺寸 / 环绕 / 位置 / z-order 回写 / 形状样式 / 文本框内容与 Fallback 同步；`spec/15` 末段、`spec/17`
  「不在 M6」）。
- **`TEST-07` 1,000 序列、`fuzz_edit`**（`spec/11`）；**空白文档模板**（TS `blank.ts`：编辑器「新建」与 AI 生成的起点，M8 切换前
  必须有，今天没有任何里程碑包含它）。

M7 是 M6 之后的**串行**里程碑：等 `m6-embedded` 并入 `main` 后从 `main` 开分支 `m7-edit`（建议工作树 `../rsWordParser-m7`）。
本计划在 2026-09-06 写成，当时 M6 只完成到 6.2，下文的基线数字分别标了来自 `main`（bf1f906）还是 `m6-embedded`（8a3034e）；
**开工前按并入后的 `main` 重测一遍**（`spec/16` / `spec/17` 都这么做过）。M6 留下的东西 M7 直接站上去：`NewBlock::Image / Chart`、
`MediaStore::add`、`to_anchor_xml`（6.7）、`model/omml/`（6.5）、`--scope embedded` 与 `--scope all` 是第七、第八道门。

## M7 · 编辑引擎收口：修订生成与接受 / 拒绝、`EditOp` 全集、块字段生成器、绘图编辑、随机序列门

目标：（1）`track_changes` 开启时每个**内容**操作都产生 Word 形态的修订（`w:ins / w:del / w:delText / *PrChange /
trPr/ins|del / cellIns|cellDel / sectPrChange`），并且**拒绝全部修订能回到操作前、接受全部修订等于不追踪地做一遍**；
（2）文档里的每一种修订（`MOD-09` 16 种 + run 级 5 种）都能单独或按作者接受 / 拒绝；（3）`docs/03` §8.2 的 `EditOp` 全集落地，
外加分节符与绘图编辑；（4）块字段有生成器；（5）随机编辑序列与编辑 fuzz 成为常驻门；（6）空白文档模板。做完之后 L4 与保存层
对 M8（编辑器切换）没有已知缺口，剩下的只是绑定形态（见「待决」）。

**M7 门**（`spec/11` TEST-10 M7 行的具体化）：

1. **修订三条 oracle**（`tests/tracked_ops.rs`，对每个「可追踪」操作 × 语料样本）：① **拒绝还原** `apply(op, track = A)` →
   `RejectAll { author: A }` 后 `ModelFingerprint` 与操作前相等；② **接受等价** `apply(op, track = A)` → `AcceptAll` 后
   `ModelFingerprint` 与 `apply(op, 不追踪)` 相等；③ **往返** 追踪保存 → 重解析 → `Document.revisions` 恰好多出预期的条目
   （种类 / 作者 / 日期 / 个数），`compat_ts` 的 `runs[].ins / del / rPrChange`、`pPrChangeInfo`、`paraMarkDel`、`rowRevisions` /
   `cellRevision` 按 TS 形态出现。`ModelFingerprint` 的定义见「分层决策」3。
2. **接受 / 拒绝覆盖**：`MOD-09` 每种修订至少一个语料或构造用例，Accept 与 Reject 各一条 XPath 断言（`spec/08` 验收清单
   「每种修订各一用例」）；语料里 21 份带修订的文档（run 级 12、`pPrChange` 4、move 4、表格 2、段落标记 1；`revisions__*` /
   `table-revisions__*` 13 个 stem）逐份 `AcceptAll` 与 `RejectAll` 成功、`SAVE-02` 无 `EngineInvariantViolation`、重解析后
   `Document.revisions` 为空。
3. **真实 Word 对照**（`fixtures/revisions/<case>/{base,tracked,accepted,rejected}.docx`，`TEST-08` 同一机制）：我们对
   `tracked.docx` 做 `AcceptAll` / `RejectAll` 的 `ModelFingerprint` 与 Word 自己「接受所有修订」/「拒绝所有修订」另存的文档相等。
   **fixture 已就位**（2026-09-07 第三轮，`docs/09`）：四个 case `run-edits`（run 插删 + `rPrChange`）、`para-split-merge`
   （拆合 + `pPrChange`）、`table-and-move`（表格行列 + 跟踪移动 + `tcPrChange` / `tblGridChange` / `tblPrExChange`）、
   `tracked-two-authors`（两个作者，验 `AcceptAll { author }` 的过滤），每个四态齐全，说明见 `fixtures/revisions/README.md`，
   制作记录见 `corpus/real/_round3/REVISIONS.md`。**写测试前必须知道的三条 Word 实测行为**：

   - **`ModelFingerprint` 必须忽略 run 边界**：拒绝一处 `rPrChange` 后 Word 不会把切开的 run 合并回去
     （`run-edits/rejected.docx` 那段是三个 run，字符与 `base.docx` 完全一致）。按 run 逐个比较会误判。
   - **Word 的「拒绝所有修订」不撤销单元格合并**：`table-and-move/rejected.docx` 行数与文字都回到 `base`，但首行仍是
     合并的一格（`w:gridSpan="2"`）。所以 7.4 的 `CellMerge` Reject 目前 `Err(EDIT_UNSUPPORTED)` 与 Word 并不冲突——
     Word 自己也不还原；要么照 Word 做（删标记、保留合并），要么把差异登记进 `docs/04` §8。
   - **批注没有真正的两层嵌套**：在界面上对一条回复再点「答复」，Word 保存出来的 `w15:paraIdParent` 仍指向线程根
     （`corpus/real/revisions2/comment-nesting.docx`：5 条批注、3 条根、最大深度 1）。M7 不需要支持嵌套线程。

   7.6 / 7.7 的「人工核对」也换成了同一机制：`fixtures/word-ops/{insert-next-page,delete-break,z-order,move-resize}/`
   是 Word 自己做那个操作的 `before` / `after` 两份，实现后直接与 `after.docx` 比形态，不再需要等人打开看
   （已复算的形态变化见 `fixtures/word-ops/README.md`；其中 **删掉分节符后由后一节的页面设置接管**，与 7.6 写的
   「合并后取后节属性」一致）。
4. **保存差分**（`COMPAT-08`）：`tests/save_blocks.rs` 跳过数保持 0（M6 门第 2 条），比较范围从 `documentXml` 扩到**每个被改写的
   XML part**（7.0 重导语料记录 `changedParts`）；新增的比较项全部等价或登记 `INTENTIONAL`。`apps/docs/tests/ai-track-revisions.test.ts`
   的 10 个场景与 `revisions.test.ts`「tracked change save fidelity」的 4 个场景各有一条原生等价测试。
5. **`TEST-07`**：1,000 条随机序列（100 份语料 × 10 个种子 × 100 步；操作全集 + `track_changes` 每步随机 + 随机 Accept / Reject）
   每步 `refresh == rebuild`、`SPAN-09` / `FLD-13` 通过、无 `EngineInvariantViolation`；每 20 步保存 → 良构 → 重解析
   `ModelFingerprint` 相等 → 继续。PR CI 跑 100 条，nightly 跑 1,000 条。`fuzz_edit` 10 分钟无崩溃（`TEST-06`）。
6. **既有门不退**：八道 `diff-parse` 门（`text / fields / tables / drawing / hf / embedded / all`）继续为 0；`corpus/hostile` 新增
   6 份修订 / 分节 / 绘图病态输入解析成功、局部降级、无编辑保存字节相同（`TEST-09`）。

### 实测基线（2026-09-06）

| 量 | 值 | 来源 |
| --- | --- | --- |
| 语料 | `main` 573 份 / 162 份保存用例 / 26 份 hostile；`m6-embedded` 799 / 208 / 32 | `ls corpus/*` |
| 保存差分 | `main` 138 等价 + 4 `INTENTIONAL` + 20 跳过；`m6-embedded` 143 / 208，61 份被 M6 阻塞（M6 门第 2 条要求归零） | `cargo test -p rsword --test save_blocks -- --nocapture` |
| `INTENTIONAL` | 4 条，其中 3 条是修订 `w:id`（`revisions__007.save.1/2/3`：我们按 `EDIT-06` 全局 max + 1，TS 写 0 / 9001）——M7 后仍保留 | `tests/save_blocks.rs` |
| 带修订的解析语料 | run 级 `ins / del` 12 份、`pPrChangeInfo` 4、`moveRevision` 4、行 / 格修订 2、`paraMarkDel` 1；块级 `blockRevision` 0、`rPrChange` 0 | `grep -l … corpus/synthetic/*.expected.json` |
| 带修订 run 的保存用例 | 2（`revisions__001.save.1`、`revisions__007.save.3`）；块级修订 2（`revisions__007.save.1/2`，TS 形态 `<w:ins><w:p>`） | 同上 |
| 字段 / 节 | 带 TOC 的文档 8 份（`fieldDisplay.kind = tocLine`）；带分节段落 13 份；保存用例里**没有**生成 TOC / SEQ / INDEX 的（三个生成器在 `apps/docs` 功能区调用，不经 `docx-engine` 测试） | 同上 |
| `EditOp` 变体 | 36（M6 再加 `SetChartData / ReplacePartXml / ReplacePartBytes / ReplaceImageMedia / InsertInk / RemoveInks`） | `edit/mod.rs` |
| `track_changes` 的消费者 | 0（`edit/mod.rs:46` 只有字段）；`apply_save_blocks` 对 `run.rPrChange` 报 `EditUnsupported`（`save_blocks.rs:1117`） | `grep -rn track_changes crates/rsword/src` |
| 修订模型 | `Revision` 16 种（块 / 段 / 表 / 节）+ `RevisionCtx { ins, del, move_from, move_to, props_change }`（run）；没有 `RevisionId`，没有跨 part 的索引 | `model/block.rs`、`model/inline.rs` |
| 测试数 | `main` 438；`m6-embedded` 452 | `cargo test --workspace` |

语料在修订上很薄：TS 测试只覆盖「解析既有修订」与「重发编辑器给的标记」，`rPrChange` 解析侧 0 份、块级修订 0 份、`cellMerge` /
`numberingChange` / `delInstrText` 各 0 份。所以本里程碑的正确性主要靠三条 oracle（门 1）与真实 Word fixture（门 3），差分只是
回归网；缺的种类用构造文档补（`tests/common::docx_with_body`）。

### 任务

| # | 任务 | 规范 | DoD |
| --- | --- | --- | --- |
| 7.0 | **语料、工具与 fixture 骨架**（`tools/export-golden`、`tests/save_blocks.rs`、`tools/gen-fixtures`、`corpus/hostile`）：① 导出器给每个 `saveDocx` 用例记 `changedParts: { path: xml }`（保存前后 zip 条目字节不同的 XML part：`docProps/core.xml` / `settings.xml` / `comments*.xml` / `footnotes.xml` / `header*.xml` / `styles.xml` / `numbering.xml` / `theme1.xml` / customXml / `[Content_Types].xml` / `.rels`）与 `changedBinary: { path: sha256 }`，`*.save.json` 加 `version: 2`，按 `TEST-02` 重导（唯一合法途径；先按 README「重导的稳定性与噪音」核对既有 stem 不漂移、`outputSha256` 噪音还原），与 M6 的语料**一起重导一次**；② `save_blocks.rs` 对每个记录的 XML part 按 `xml::canon` 比较，`.rels` 按 (Type, Target, TargetMode) 集合比较（rId 编号是分配细节，`COMPAT-09` 加一条），`ignore_attr` 的 `w:rsid*` / `w14:paraId` / `w15:*Id` 扩到所有 part，二进制 part 比 sha256，`documentXml` 比较不变；③ `fixtures/revisions/`：`gen-fixtures revisions` 生成 3 份 `source.docx`（无修订的起点：多段正文 + 表格 + 两节）与每份一张「请在 Word 里开修订做这些操作，然后三存：`tracked` / 接受所有 → `accepted` / 拒绝所有 → `rejected`」的操作单（README，格式同 `fixtures/resolve/README.md`），`tests/revision_fixtures.rs` 用 `fixture_tests!` 展开，`verified = false` 只记不断言（5.8 同款）；④ `fixtures/fieldgen/`：一次性脚本 `tools/export-golden/fieldgen.ts` 把 TS `generateTocFieldXml / generateCaptionXml / generateIndexFieldXml / latexToOmml` 对固定输入的输出落盘（生成器在 `apps/docs` 里调用，语料里没有）；⑤ hostile 6 份（生成器进 `hostile.export.test.ts`）：`rev-nested-wrappers`（`w:ins` / `w:del` 交替嵌套 500 层）、`rev-move-unpaired`（`moveFrom` 无 `moveTo` 孪生、`w:name` 对不上、range 跨段）、`rev-change-empty`（`rPrChange` / `pPrChange` / `tblPrChange` 无内层容器或多个内层）、`rev-del-with-t`（`w:del` 里是 `w:t`；`w:ins` 里有 `w:delText`）、`sectpr-in-cell`（单元格段落带 `sectPr`）、`drawing-anchor-no-extent`（`wp:anchor` 缺 `extent`、`relativeHeight` 溢出 u32、`wp:positionH` 无子元素）；⑥ 基线数字按并入后的 `main` 重测，写进 `docs/04` §16 开头 | TEST-02, TEST-03, TEST-08, TEST-09, COMPAT-09 | 重导后 `save_blocks` 等价数不降（`changedParts` 带出的新差异全部修掉或登记）；`changedParts` 覆盖的用例数 / part 数写进 `docs/05`；6 份 hostile 满足 `TEST-09` 三条；两个 fixture 目录与 README 就位（观察值空着，测试只记不断言） |
| 7.1 | **修订索引与 `RevisionId`**（`model/revision.rs`、`model/build.rs`、`edit/session.rs`）：`Document.revisions: RevisionIndex`——把散在 `Run.rev`（`RevisionCtx`）、`Block.revisions`、`TextBlock.revisions`、`Row / Cell / TableBlock / SectionInfo.revisions` 与 `FieldSpan`（`delInstrText`）上的修订收成一张**跨 part** 的表 `RevisionEntry { id: RevisionId, part: PartId, kind: RevKind /* 21 种 = MOD-09 16 + RunInsert / RunDelete / RunMoveFrom / RunMoveTo / RunPropsChange，named_enum! */, node /* 承载元素 w:ins / w:rPrChange / … */, meta: RevisionMeta, owner: RevOwner /* Run(NodeId) \| Block \| ParaMark(p) \| Row \| Cell \| Table \| Section \| Field(FieldId) */, depth: u16, pair: Option<RevisionId> /* moveFrom ↔ moveTo，按 w:moveFromRangeStart/@w:name 配对 */ }`，文档序（part 顺序：主 part → 页眉页脚 → 脚注 / 尾注 → 批注），`by_author(&str)`、`iter_inner_first()`；`RevisionId(u32)` **会话内稳定**：`EditSession` 持 `HashMap<(PartId, NodeId), RevisionId>` + 单调计数器，`refresh` / `rebuild` 对仍存在的承载节点复用旧 id（arena 里 `NodeId` 稳定），新节点拿新 id；无会话的 `Document::rebuild` 从 0 编号。`RevisionMeta.id` 解析成 `Option<u32>`（`EDIT-06` 取 max 用；非数字保留原串、不参与 max）；会话缓存「全包最大修订 `w:id`」，plan 阶段按需预留区段，**任何 part 变脏时失效重扫**。`Settings.track_revisions` 进 `SettingsPatch`（`settings.toml` 第 96 行已建模读侧）。compat 不变（TS 没有修订列表）。`docs/03` §6.6 说 `RevisionId` 挂在每个修订上，我们放索引里、`Revision` / `RevisionCtx` 不加字段——登记 `docs/04` §8 | MOD-09, MOD-13, EDIT-06 | `tests/revisions.rs`：21 份带修订语料的条目数 / 种类 / 作者与 `compat_ts` 的 `ins / del / pPrChangeInfo / moveRevision / rowRevisions / cellRevision / paraMarkDel` 逐份对齐；4 份 `moveRevision` 文档每个 moveFrom 都有 `pair`；`hostile/rev-move-unpaired` → `REV_UNPAIRED_MOVE` 诊断且 `pair = None`；`rev-nested-wrappers` 500 层 `depth` 正确、迭代遍历无栈溢出；随机 `InsertText` 后 `refresh` 保留既有 `RevisionId`（`MOD-13`）；`SetDocumentSettings { track_revisions: Some(true) }` 写出 `w:trackRevisions` 且位置符合 `PROP-05` |
| 7.2 | **修订生成：内联与段落**（`edit/track.rs`、`edit/ops.rs`、`edit/inline.rs`）：规则见下文「修订生成规则」表。机制：`Tracker`（plan 阶段的辅助，不是新的执行阶段）提供 `wrap_new_runs(runs) -> New w:ins{id, author, date}`、`mark_deleted(runs)`（包 `w:del`、`w:t → w:delText` / `w:instrText → w:delInstrText` 改名走 `NodeEdit::Rename`）、`snapshot_change(container, kind)`（`*PrChange` = 容器现有子元素的 `Clean` 克隆，排除 `*Change` 自身与 `in_change = false` 的字段，插在容器末尾）、`para_mark(p, Ins \| Del)`（`pPr/rPr` 缺则新建，`w:ins` / `w:del` 按 `run.toml` `order` 首位）；`w:id` 来自 7.1 的预留；`w:date` = `RevisionAuthor.date` 原串（引擎无时钟）。覆盖 `InsertText / DeleteRange / SetRunProps / SetParaProps / ReplaceParaProps / SplitParagraph / MergeWithNext / InsertField / SetLinkTarget / SetFormText / SetFieldResultProps / ReplaceInlines`（后者追踪时做**坐标流 diff**：Myers，UTF-16 单元，聚到 run 边界；相等段保留原节点、删除段 `w:del`、插入段 `w:ins`；非文本内联按 canon 字节比较；标记按新位置原样重发）。**同作者规则**（Word）：落在 A 自己的 `w:ins` 内插入 → 直接插；删 A 自己插的 → 真删；删他人 `w:ins` 里的 → `w:ins` 内嵌 `w:del`；落在 `w:del` 内插入 → `Err(EDIT_IN_DELETED)`；落在他人 `w:ins` 内插入 → 拆开外层 `w:ins`（属性克隆）把新 `w:ins` 夹在中间。**追踪下 `DeleteRange` 不移动锚点、`offset_delta = 0`**（内容还在坐标流里，`SPAN-06` 删除规则不调用）。compat：`apply_save_blocks` 的 `run.rPrChange` → `NewRun.props_change: Option<NewRevision { author, date, id, old: NewElement }>` 发 `w:rPrChange`（去掉 `save_blocks.rs:1117` 的拒绝）；编辑器给的 `ins / del` 继续走 `NewInline::Ins / Del`，compat 路径永远 `track_changes = None` | EDIT-03, EDIT-05, EDIT-06, SAVE-04, PROP-05, SPAN-06 | 门 1 的三条 oracle 对这 12 个操作在 ≥ 20 份文本域语料上通过（`oracle_tests!` 展开）；`EDIT-03` 验收行「在干净 run 中间插字 → 追踪时出现 `w:ins`，`w:p` 开标签字节不变」「追踪时合并 → 段落仍分开，`pPr/rPr/w:del` 出现；Accept 后真正合并」；同作者规则五种各一用例；调试构建 `PROP-05` 自检通过（`w:ins` 是段落标记 `rPr` 第一个子元素、`rPrChange` 是 `rPr` 最后一个）；`ai-track-revisions` 场景 1 / 2 / 5 / 8 / 9 的原生等价（insert → ins；replace → del + ins；保存为 `w:ins` / `w:del` 且 Word 可接受 / 拒绝的形态；`rPrChange` 记旧格式；`pPrChange` 记旧对齐）；`save_blocks`：`rPrChange` 不再是跳过原因 |
| 7.3 | **修订生成：块、表格、节、其他 part**（`edit/track.rs`、`edit/table_ops.rs`、`edit/section_ops.rs`、`edit/media_ops.rs`）：`InsertBlock` 段落 → 内容 run 包 `w:ins` + 段落标记 `pPr/rPr/w:ins`；表格 → 每行 `trPr/w:ins`；`NewBlock::Xml / Wrapped` → 块级 `w:ins` 包裹（TS 形态，解析器已认）；`DeleteBlock` 段落 → 内容 `w:del` + 标记 `w:del`（段落保留），表格 → 每行 `trPr/w:del`，其他 → 块级 `w:del`；`InsertRow / DeleteRow` → `trPr/w:ins \| w:del`（行保留）；`InsertColumn` → 新格 `tcPr/w:cellIns` + `tblGridChange`（旧 `tblGrid` 克隆）；`DeleteColumn` → 格保留 + `tcPr/w:cellDel` + `tblGridChange`；`MergeCells` → **第一阶段** `Err(EDIT_UNSUPPORTED_TRACKED_MERGE)`（`cellMerge` 的 `vMergeOrig` 形态等门 3 的 fixture 校准后再做）；`Set{Table,Row,Cell}Props` → `tblPrChange / trPrChange / tcPrChange`；`SetSectionProps` → `sectPrChange`（快照不含页眉页脚引用：`section.toml` `in_change = false`）；`MoveBlock` → `Err(EDIT_UNSUPPORTED_TRACKED_MOVE)`（`docs/03` §8.2 第一阶段；调用方用 Delete + Insert）；`SetHeaderFooter` 内容替换 → 在页眉 part 里按段落规则 del + ins；M6 的 `InsertBlock{Image / Chart}` → 段落规则；`ReplaceImageMedia` → 旧 run `w:del` + 克隆 run（换 blip）`w:ins`；`UpdateBlockField` → 旧结果块 del、新结果块 ins。**不追踪**（Word 也不记修订或另有机制；记 `REV_NOT_TRACKED` 诊断、照常执行）：`AddBookmark / RemoveBookmark / AddComment / RemoveComment / SetCommentText / ToggleCheckbox / LinkHeaderFooter / SetWatermark / SetPageColor / SetDocumentSettings / SetChartData / ReplacePart* / InsertInk / RemoveInks` 与 7.7 的绘图几何 / 样式操作 | EDIT-03, EDIT-05, PROP-05, XML-12 | 门 1 的 oracle 对这些操作在 ≥ 10 份表格文档、10 份页眉文档、5 份带图文档上通过；`EDIT-03` 表格验收行的追踪版（`InsertRow` 新行带 `trPr/w:ins` 且 `tcPr` 与模板行字节相同；`DeleteRow` 后行仍在且带 `trPr/w:del`；`DeleteColumn` 后该列 `w:tc` 仍在且带 `cellDel`、`tblGridChange` 里是旧网格）；`ai-track-revisions` 场景 7（含表格的结构替换记为块级修订）；`revisions.test.ts` save-fidelity 4 场景的原生等价（顶层段落 / 表格包裹往返、重发、新修订铸 id 且互不相同、未碰的修订段落字节相同）；tracked `MoveBlock / MergeCells` → `Err` 且 DOM / Span / Model 与操作前一致（`EDIT-05`） |
| 7.4 | **接受 / 拒绝修订**（`edit/revision_ops.rs`）：`AcceptRevision { rev: RevisionId }` / `RejectRevision { rev }` / `AcceptAll { author: Option<String> }` / `RejectAll { author }`（`docs/03` §8.2 的 `AcceptAll / RejectAll` 加作者过滤——编辑器今天按作者接受 / 拒绝，`apps/docs/.../revisions.ts::applyRevisionsBy`；登记 §8）。语义按 `spec/08` EDIT-03 那张表，补定：顺序 = 文档序、**先内层后外层**（`iter_inner_first`；`w:ins` 套 `w:del` 先处理 `del`）；`ParaMarkDelete` Accept = 无追踪 `MergeWithNext`（下一段须在同容器；本段是单元格最后一段时留一个空 `w:p`——表格通则）；`ParaMarkInsert` Reject 同上合并；`RunPropsChange` Reject = `rPr` 子元素**整体换成** `rPrChange/rPr` 子元素的克隆再删 `rPrChange`（不走 patch——未建模子元素也要还原；typed `old` 只服务模型与 compat）；`ParaPropsChange / TablePropsChange / RowPropsChange / CellPropsChange / SectPropsChange` 同法（保留 `rPr` / `sectPr` / 页眉页脚引用）；`TableGridChange` Reject = `tblGrid` 还原为 `tblGridChange` 快照所记的**列宽序列**；当前表格中已被本轮修订删除的列（`Dirty::Deleted`）不还原其 `gridCol`——还原的是「宽度」，不是「快照的子元素树」（2026-09-09 项目负责人裁定；原措辞「换成快照克隆」逐字描述的是 `restore()` 的缺陷行为，见 `docs/04` §8）；`MoveFrom / MoveTo` 按 `pair` 成对处理（Accept：MoveFrom 内容 `Deleted`、MoveTo 解包；Reject 反之），range 标记 `w:moveFromRangeStart/End` / `moveToRange*` 一起删，孤儿（`pair = None`）按 Delete / Insert 处理 + 诊断；`CellInsert` Reject / `CellDelete` Accept = 删格 + 网格收缩（复用 `DeleteColumn` 的 `gridSpan / tcW` 算法；整列都删则删 `gridCol`）；`CellMerge` Accept = 删标记，Reject = `Err(EDIT_UNSUPPORTED)`（`vMergeOrig` 还原待 fixture）；`NumberingChange` Reject = `numPr` 换成 `numberingChange` 快照；`FieldInstrDelete` Accept = `delInstrText` run `Deleted`，Reject = 改名 `instrText`；`DeletedText` 随 `Delete`（Reject 时 `w:delText → w:t`）。`SPAN-07` 的 MoveFrom / MoveTo 行改回规范原文「由修订操作决定」：`DeleteRange` 覆盖整个 move 范围只在**未追踪**下当删除。Anchor：解包（`move_within_part`）不改文档序、范围不动；内容 `Deleted` 走 `SPAN-06/07` | EDIT-03, EDIT-05, SPAN-06, SPAN-07, MOD-09 | 21 种修订各 Accept / Reject 一条 XPath 断言（`accept_reject!` 表展开，缺语料的用构造文档）；21 份语料 `AcceptAll` / `RejectAll` 成功 + 重解析 `revisions` 为空（门 2）；`ai-track-revisions` 场景 3 / 4（accept all 删掉划线段、新文字去标记；reject all 删掉插入段、原文去标记）与 8 / 9 的 reject 半边（还原旧格式 / 旧对齐）；`fixtures/revisions` 观察值就位则门 3 通过；`AcceptAll { author: Some(A) }` 只动 A 的、B 的原字节不动；`hostile/rev-move-unpaired` 的 `AcceptAll` 成功且诊断；`AcceptAll` 是一个事务：中途注入失败 → `ModelFingerprint` 与操作前一致 |
| 7.5 | **内联原子、注释 / sdt 内容、跨段删除、公式作者方向**（`edit/atom_ops.rs`、`edit/note_ops.rs`、`edit/sdt_ops.rs`、`model/omml/latex_to_omml.rs`）：`InsertAtom { at, atom: NewAtom }`——`Break(Page \| Column \| TextWrapping { clear })` → `w:r/w:br`；`Symbol { font, code }` → `w:r/w:sym`（`RES-05` 符号字体反向：PUA ↔ 字节）；`NoteRef { kind, content: Vec<NewBlock> }` → 新条目（`w:id` 按 `EDIT-06`，首段前置 `w:footnoteRef` run + 空格，`FootnoteReference` 样式）+ `w:r/w:footnoteReference`，part 不存在按 `SAVE-05` 建；`Image(NewImage)` → run 内 `wp:inline`（复用 6.7 生成器，不另起段）；`Math(NewMath::Omml(String) \| Latex(String))` → `m:oMath` 原子。`SetMathTokens { math: NodeId, tokens: Vec<String> }`（TS `patchMathTokens`：`m:t` 个数不等 → `Err(EDIT_MATH_TOKEN_COUNT)`）；`NewBlock::MathPara { omml, align }`（TS `mathParagraphXml`）。**`latexToOmml`**（TS `math.ts` 724–1087）逐字移植：递归下降解析器带深度上限 256（用户输入，不是文档；超限 `Err(EDIT_MATH_TOO_DEEP)`），输出与 TS 逐字相等（对照 `fixtures/fieldgen/`）。`SetNoteContent { kind, id, content }`、`RemoveNote { kind, id }`（删条目 + 引用 run；run 空则整 run 删）；`DeleteRange` 覆盖 `FootnoteRef` → 同时删条目（`EDIT-03` 既有措辞）。TS `text-patch` 场景（改批注 / 脚注文字保留加粗与超链接）在我们这里就是 `InlinePos { part: 注释 part }` 上的 `InsertText / DeleteRange`——补两条原生等价测试。`SetSdtContent { sdt, inlines }`（`ContentLocked` → `Err(EDIT_SDT_LOCKED)`、`data_binding` → `Err(EDIT_SDT_BOUND)`，3.3 的判定复用）、`RemoveSdtShell { sdt }`（内容 `move_within_part` 到父、`w:sdt` `Deleted`；Word「删除内容控件」）。**跨段 `DeleteRange`**：`from.para ≠ to.para` 且同容器 → 一条计划里拆成 首段尾部删除 + 中间块 `DeleteBlock` + 末段头部删除 + `MergeWithNext`；追踪时末段并入用段落标记 `w:del`（不真合并）；跨容器 → `Err(EDIT_CROSS_CONTAINER)`。**`SPAN-10` 另一半**：范围端点落在字段指令区（begin..separate）→ 起点移到原子前、终点移到原子后（`span/transform.rs`，与插入侧同一规则） | EDIT-02, EDIT-03, EDIT-06, SAVE-05, SPAN-06, SPAN-10, FLD-10, RES-05 | 每个原子一条 XPath 验收 + 坐标流长度 1（`EDIT-02`：原子前后偏移差 1）；`NoteRef` 新建 part 满足 `SAVE-05`（其他条目 CRC 不变）；`latexToOmml` 夹具逐字相等、深度 300 的输入 → `Err` 不爆栈；`text-patch` 两场景（批注里的加粗 run 与超链接、脚注里的斜体在改字后原字节）；`SetSdtContent` 锁定 / 绑定拒绝且状态不变；跨段删除：三段文档从第 1 段中删到第 3 段中 → 剩一段、书签端点按 `SPAN-06` 落到删除点，追踪版三段仍在、中段带 `w:del`、首段标记带 `w:del`；`SPAN-10` 用例：书签起点落在 `instrText` 内 → 保存后标记在 `fldChar begin` run 之前 |
| 7.6 | **分节符与跨 part 搬移**（`edit/section_ops.rs`、`xml/edit.rs`、`model/section.rs`）：`InsertSectionBreak { after: NodeId /* 段落 */, kind: SectType }`——段落 `pPr` 里新建 `w:sectPr` = 该段所属节的活 `sectPr` 的 `Clean` 克隆（含页眉页脚引用——第一节保住自己的页眉；位置 `rPr` 之后 `pPrChange` 之前，`PROP-05`），再对**原 `sectPr`**（现在描述后一节）`SetSectionProps { type: kind }`；`titlePg` / `pgNumType` 随克隆走。段落不是块容器（body / `sdtContent`）的直接子节点（在表格里）→ `Err(EDIT_BAD_POSITION)`（Word 也不允许在单元格里分节）。`DeleteSectionBreak { sect: NodeId /* 段落级 sectPr */ }`：`sectPr` `Deleted`，块并入后一节（Word 语义：合并后取**后**节属性）；body 级 → `Err`。`Document.sections` / `block_range` / `section_of` 由 5.2 的 `refresh_blocks` 重算，`RES-10` 继承按新节表；compat 的 `Section break paragraph` 分类随 facts 自然出现。追踪：不产生修订（Word 用段落标记 ins + `sectPrChange` 的组合形态，等门 3 的 fixture 后再定），记 `REV_NOT_TRACKED`。**跨 part `MoveBlock`**：`rehome_subtree`（`XML-12` E′）落地——按目标作用域重解析子树用到的全部前缀，能绑到同 URI 的复用目标前缀，绑不上的在子树根声明（`Dom::declare_for_new_subtree`）；源 `Deleted`、目标 `New`；范围：整个落在被搬块内的跟着走（换 `FlowId`），一端在外的按 `SPAN-07`「容器删除」处理（端点落到删除点）；块字段被劈开 → `Err(EDIT_SPLIT_FIELD)` | EDIT-03, EDIT-05, PROP-05, RES-10, MOD-10, XML-12, XML-14, SPAN-07 | 单节文档在中段后插分节符 → `sections().len() == 2`，第一节 `sectPr` 是段落级且与 body 级除 `w:type` 外 canon 相等，两节页眉引用相同、`RES-10` 六槽有效值不变；`hf-variants__007`（已有多节）再插 → 三节且原有 part 引用不变；删除刚插的分节符 → `ModelFingerprint` 回到操作前；单元格内 → `Err` 且状态不变；`sections__*` 13 份与 `--scope hf` 无回归；跨 part 搬：正文段落搬到页眉 → 主 part 与页眉 part 各自脏、前缀按页眉 part 的声明、其他 part CRC 不变、搬走段落里的书签消失（`SPAN-07`）、保存良构且 `SAVE-02` 无前缀未绑定 |
| 7.7 | **绘图与形状的编辑**（`edit/drawing_ops.rs`、`model/drawing.rs` 生成方向、`save/options`）：`SetDrawingGeometry { drawing: NodeId, extent_emu: Option<(u32, u32)>, rot_deg: Option<Option<u16>>, flip_h: Option<bool>, flip_v: Option<bool>, crop: Option<Option<SrcRect>> }` → `wp:extent/@cx @cy` + `a:xfrm/a:ext` + `wp:effectExtent`（旋转外接框，6.7 的公式）+ `a:xfrm/@rot @flipH @flipV` + `a:srcRect`；只改属性 → 元素 `SelfDirty`；文本框 / 形状的 `wps:spPr/a:xfrm` 同一操作（TS `patchTextboxHeights / patchTextboxSizes / patchDrawingExtent`）。`SetDrawingWrap { drawing, wrap: Option<ImageWrap> /* None = 随文 */, pos: Option<AnchorPos { h, v }>, z_order: Option<i32> }` → `wp:inline ↔ wp:anchor` 切换**只重建外壳**：`wp:extent / effectExtent / docPr / cNvGraphicFramePr / a:graphic` 用 `move_within_part` 搬进新壳（原字节保住），`simplePos / positionH / positionV / wrap*` 按 6.7 `to_anchor_xml` 生成；`tight / through`：同类环绕保留原 `wp:wrapPolygon`，否则生成矩形多边形（TS 落成 `wrapSquare`，我们更强，登记）；`AnchorPos` 的每一轴 = `Offset(emu) \| Align(…)` + `relative_to`。`SetDrawingZOrder { drawing, z: i32 }` → `relativeHeight = 251658240 + z`，`behindDoc` 由 `ImageWrap::Behind` 决定。**z-order 归一**（TS `normalizeImageZOrders` + `applyImageZOrder`，`docs/01` §13.7 M7 行）：`SaveOptions.normalize_z_order: bool`，缺省 **false**（不动未编辑字节）；compat 在投影给出 `imageZOrderNormalized` 时置 true；开启时主 part 全部 `wp:anchor` 按文档序稳定重排 0..n。`SetShapeStyle { shape, fill: Option<Option<Rgb>>, outline: Option<Option<Rgb>> }` → `wps:spPr/a:solidFill \| a:noFill`、`a:ln/a:solidFill \| a:noFill`。**文本框内容与 Fallback 同步**：`w:txbxContent` 里的段落本来就可编辑（`InlinePos.para` 任意深度）；新规则——编辑位置落在 `mc:Choice` 的 `w:txbxContent` 内 → 同一 `mc:AlternateContent` 里 `mc:Fallback` 的同序 `w:txbxContent`（VML 孪生）内容整体换成编辑后 Choice 内容的深克隆（`Deleted` + `New`）；位置落在 `mc:Fallback` 内 → `Err(EDIT_TARGET_FALLBACK)`；`SetTextboxContent { box, blocks }` 整体替换走同一路；`SetShapeStyle` / `SetDrawingGeometry` 同步孪生 `v:shape/@style @fillcolor @strokecolor`。`NewBlock::Textbox { extent, anchor: Option<…>, fill, outline, blocks }` / `NewBlock::Shape { preset: PresetGeom, extent, anchor, fill, outline, text }` / `NewBlock::Line { kind: LineKind /* TS LINE_KINDS */, from, to }`：DrawingML `wps:wsp`；文本框与形状发 `mc:AlternateContent`（`Choice Requires="wps"` + VML Fallback，与 TS / Word 同形；Strict 包只发 Choice）；WordArt（VML `v:textpath`）不在 M7。追踪：以上都不产生修订（Word 不把图片格式改动记为修订），记 `REV_NOT_TRACKED` | EDIT-03, EDIT-05, MOD-11, SAVE-03, SAVE-07, SAVE-08, XML-12, XML-14 | `image-wrap__*` 的 11 份文档逐份 `SetDrawingWrap` 切到每种 `ImageWrap` 再切回 → `ModelFingerprint` 相等且 `a:graphic` 子树字节原样（`SAVE-08`）；`SetDrawingGeometry` 改尺寸后重解析 `imageMeta.widthPx / heightPx` 相等、未碰属性原字节；z-order 归一：LibreOffice 写 1, 2, … 的语料开启归一后 `relativeHeight` 为 251658240 + 0..n 且稳定，关闭时字节不变；`textbox-edit`（TS `patchTextboxParas` 场景）原生等价：改 Choice 文本框一段 → Fallback 孪生同步、其他段原字节；Fallback 内位置 → `Err`；三种 `NewBlock` 各一条 XPath + 重解析后 `textboxes[]` / `Drawing object` 的分类与 TS 同形态；Strict 包新建文本框无 VML |
| 7.8 | **块字段生成器与空白文档**（`span/field/generate/{toc,seq,index}.rs`、`edit/field_ops.rs`、`save/blank.rs`）：`BlockFieldGenerator`（`FLD-09`）三实现。**TOC**：读指令开关 `\o "1-3"`（级别范围）、`\u`（outlineLvl：`Resolver::heading_level` + 段落 `outlineLvl`）、`\t "Style,1,…"`（自定义样式 → 级别）、`\h`（超链接）、`\n`（不要页码）、`\z` / `\p` / `\w`（记录、不影响）；条目 = 正文（含表格内）按文档序的匹配段落，文本 = 坐标流去字段 / 去脚注引用；输出每条一段：`pStyle TOC{n}`（缺样式时按 5.7 `StyleUpsert` 从空白模板补）、右对齐点线制表位（位置 = 节 `pgSz.w − mar.left − mar.right`；TS 固定 9350，`TocOptions.ts_shape` 时照写）、`noProof`、`\h` → 给每个标题段落加隐藏书签 `_Toc{9 位}` + 条目包 `w:hyperlink w:anchor`（TS 不做，登记为「超过 TS」）、页码 = `TocOptions.page_numbers: Option<&HashMap<NodeId, u32>>`（调用方的分页结果；`\n` 或 `None` → 不写）写成 `PAGEREF _Toc… \h` 字段（`ts_shape` 时写纯数字）；begin 的 `w:dirty="true"` 由 `EditContext.mark_updated_fields_dirty` 控制。**SEQ 题注**：`NewBlock::Caption { label, text }` → TS `generateCaptionXml` 形态，编号 = 位置之前同 label 的 SEQ 字段数 + 1。**INDEX**：从 `FieldIndex` 的 XE 条目取词、去重、trim，`\c "2"`；排序 `Collation::CodePoint`（缺省）或 `Collation::Given(Vec<String>)`（调用方排好）——TS 用 `localeCompare('zh-CN')`（ICU），登记为差异。`InsertBlock { at, block: NewBlock::Field(NewBlockField::Toc(TocOptions) \| Index(IndexOptions)) }`：多段按 `FLD-08 / FLD-12` 形态（begin + instr + separate 在首段开头、end 在末段末尾）；`UpdateBlockField { field, blocks }` 既有机制不变，新增 `RegenerateBlockField { field, options }` 直接调生成器。**`EditSession::blank() -> EditSession`**（`Package::blank()`）：TS `blank.ts` 的 part 集合逐字移植成常量（`document.xml` 单空段 + A4 `sectPr`、`styles.xml` Normal / Heading1–6 / ListParagraph / Hyperlink / TOC1–9、`numbering.xml` bullet `numId 1` / decimal `numId 2`、`settings.xml`、`[Content_Types]`、`.rels`），导出 `BLANK_BULLET_NUM_ID / BLANK_ORDERED_NUM_ID`。compat 债：`instrField / fldBeginXml` 重发（`docs/05`「明确未实现」）——复核 `NewInline::Xml` 路径已覆盖，关掉这条 | FLD-08, FLD-09, FLD-12, EDIT-03, SAVE-05, RES-01, COMPAT-08 | `fixtures/fieldgen/` 的 TS 输出与 `ts_shape` 模式生成结果 canon 相等（TOC 3 组输入、SEQ 2、INDEX 2）；8 份 `tocLine` 语料每份 `RegenerateBlockField` → 重解析后 `fieldDisplay.tocLine` 条目的 `left / level` 与文档标题一致、begin 带 `w:dirty`、外层 sdt 与 begin / end run 原字节；`\h` 模式：每条目 `w:hyperlink/@w:anchor` 指向存在且成对的 `_Toc` 书签；`Resolver::heading_level` 与 compat `blocks[].level` 在全语料逐段一致（`docs/06` 第 3 件）；`blank()`：每个 part 与 `blank-template__001.docx` 对应 part canon 相等、`parsed_doc` 与 `blank-template__001.expected.json` 差异为 0、`blank()` 上插三级标题 + 两种列表后保存 → 重解析 `type / level / list` 正确（TS `blank-template` 第二场景） |
| 7.9 | **随机序列、fuzz、恶意输入、性能记录与 M7 门**（`tests/random_ops.rs`、`fuzz/fuzz_targets/fuzz_edit.rs`、`tests/common/fingerprint.rs`、`.github/workflows`）：`ModelFingerprint`（分层决策 3）进 `tests/common`；`TEST-07` 生成器：操作全集（含 7.2–7.8 的新操作、Accept / Reject 随机挑 `Document.revisions` 的条目、`track_changes` 每步随机 `None / Some(A) / Some(B)`）、权重表、种子可复现（失败时打印 `(doc, seed, step)` 与二分最小化后的序列，固化为回归测试）；PR CI 100 条（`cargo test --test random_ops`），nightly workflow `random.yml` 1,000 条 + 四个 fuzz 目标。`fuzz_edit`：`arbitrary` 派生 `OpSketch`（操作种类 + 相对位置 + 短文本）映射到 `EditOp`，种子文档内嵌 5 份小语料；断言：不 panic、`Err` 时 `ModelFingerprint` 不变（`EDIT-05`）、保存良构。hostile 6 份接 `TEST-09` 三条（7.0 生成）。`docs/06` 第 2 件：toggle 歧义频率探针固化成 `tests/resolve.rs` 的常驻测量（遍历 synthetic + real，非 0 时打印分布，不 fail）。`xml:space` 复核（`docs/04` §8）：`save_blocks.rs` 的 `ignore_attr` 只对我们新建的 `w:t` 放行，抽样确认没有用例靠它掩盖真实差异。性能记录（**非门**）：`benches/edit.rs`（`cargo bench`，不加新依赖）在语料最大的三份文档上量 `open / InsertText / AcceptAll / save_with`，数字写进 `docs/05`，建议观察值 `apply` < 5 ms、`save_with` < 50 ms / MB。`docs/04` §16 逐条进度、§8 偏差、`docs/05` 数字与「明确未实现」清单同步 | TEST-06, TEST-07, TEST-09, TEST-10, EDIT-05, MOD-13, SAVE-02 | 门 5 / 6 全部通过并接进 CI；`random_ops` 的最小化路径可用（至少一次人为注入的 bug 被它抓到并最小化成 ≤ 5 步）；`fuzz_edit` 进 `fuzz.yml` 矩阵；性能数字与探针结果在 `docs/05` |
| 7.10 | **（建议）JS 绑定 `crates/rsword-js`**（形态待拍板，见「待决」）：M8「编辑器切换到 Rust 引擎」需要一个能从 `apps/docs` 调用的入口，今天没有任何里程碑包含它。最小面：`parse(bytes) -> string /* ParsedDoc JSON */`、`save(bytes, blocksJson, optionsJson) -> Uint8Array`、`blank() -> Uint8Array`、`version()`；错误映射 `Error { code, message }` → JS 异常；无线程；`wasm32-unknown-unknown` 构建进 CI；体积与 `parse` 耗时记 `docs/05`。wasm-bindgen（渲染进程内，与 TS 引擎同位置）或 napi-rs（Electron 主进程，`window.desktop.saveDocx` 旁边）二选一，接口一样 | COMPAT-02, COMPAT-08, TEST-03 | `diff-parse --via js` 走绑定在全语料上为 0 差异（与原生 `compat_ts` 逐字节相同的 JSON）；`save` 对全部保存用例结果与原生 `save_blocks` 相同；`apps/docs` 里用一行替换 `parseDocx` 能打开语料文档（不进本里程碑门，是 M8 第一步的预演） |

建议顺序：7.0 与 7.1 第一周并行（7.0 的 fixture 文档与操作单要**尽早**交给项目负责人，观察值回来才有门 3）；7.2 → 7.3 → 7.4
串行（同一套 `Tracker` 与索引）；7.5 / 7.6 / 7.7 / 7.8 互不依赖，可并行；7.9 收尾；7.10 若进 M7 可与 7.5–7.8 并行。

### 修订生成规则（`EDIT-03` 每个操作的「修订」行，M7 定稿）

`track_changes = Some(A)` 时的行为。实现为 `tracking_rules!` 一张表，同时展开 `Tracker::rule_for(&EditOp)` 与门 1 的用例列表。

| 操作 | 行为 | 出处 / 备注 |
| --- | --- | --- |
| `InsertText` | 落在 `Text` 段内先 `split_run`，新 run 进 `New w:ins{A}`；落在 A 自己的 `w:ins` 内 → 直接插（不套第二层）；落在他人 `w:ins` 内 → 拆开外层 `w:ins`（属性克隆）把新 `w:ins` 夹在中间；落在 `w:del` 内 → `Err(EDIT_IN_DELETED)` | `spec/08`；Word 不允许在删除文字中打字 |
| `DeleteRange` | 覆盖的 run（先 `split_run`）进 `New w:del{A}`，`w:t → w:delText`、`w:instrText → w:delInstrText`；A 自己 `w:ins` 里的 → 真删；他人 `w:ins` 里的 → `w:ins` 内嵌 `w:del`；已在 `w:del` 里的 → 不动；原子字段整段 begin..end 进 `w:del`；覆盖 `FootnoteRef` → 引用 run 进 `w:del`，条目**保留**（Accept 时才删）；范围标记不动、`offset_delta = 0` | `spec/08`；追踪下内容还在坐标流里 |
| `SetRunProps` / `SetFieldResultProps` | 拆 run；每个无 `rPrChange` 的 run 追加 `New w:rPrChange{A}`（`w:rPr` = 现有子元素克隆，排除 `rPrChange`）再打 patch；已有 `rPrChange` 保留旧快照 | `spec/08`；`PROP-05`：`rPrChange` 末位 |
| `SetParaProps` / `ReplaceParaProps` | `pPrChange{A}`（快照排除 `rPr / sectPr / pPrChange`）；已有则保留 | `spec/08`、`SAVE-04` |
| `SplitParagraph` | 原段 `pPr/rPr/w:ins{A}`（段落标记插入），新段照常 | `spec/08` |
| `MergeWithNext` | **不合并**；本段 `pPr/rPr/w:del{A}` | `spec/08`；Accept 后合并 |
| `InsertField` / `InsertAtom` | 全部新 run 进 `w:ins` | `spec/08` |
| `SetLinkTarget`（字段） | 旧 instr run `w:del` + `delInstrText`，新 instr run `w:ins`；`w:hyperlink` 的 `r:id / w:anchor` 改动不追踪 | Word 形态 |
| `SetFormText` | 结果 run 按 `DeleteRange` + `InsertText` 规则 | — |
| `UpdateBlockField` / `RegenerateBlockField` | 旧结果块按 `DeleteBlock` 规则、新结果块按 `InsertBlock` 规则 | — |
| `ReplaceInlines` / `SetSdtContent` / `SetNoteContent` | 坐标流 diff（Myers，UTF-16 单元，聚到 run 边界）：相等段保留原节点、删除段 `w:del`、插入段 `w:ins`；非文本内联按 canon 字节比较；标记按新位置重发 | compat 路径永远不追踪 |
| `InsertBlock` 段落 / 表格 / `Xml` | 内容 run `w:ins` + 段落标记 `w:ins` / 每行 `trPr/w:ins` / 块级 `w:ins` 包裹 | 7.3 |
| `DeleteBlock` | 段落：内容 `w:del` + 标记 `w:del`（段落保留）；表格：每行 `trPr/w:del`；其他：块级 `w:del` | 7.3 |
| `InsertRow` / `DeleteRow` | `trPr/w:ins` / `trPr/w:del`（行保留） | `spec/08` |
| `InsertColumn` / `DeleteColumn` | `tcPr/w:cellIns` / `tcPr/w:cellDel`（格保留）+ `tblGridChange` | 7.3 |
| `MergeCells` / `MoveBlock` | `Err(EDIT_UNSUPPORTED_TRACKED_MERGE)` / `Err(EDIT_UNSUPPORTED_TRACKED_MOVE)` | `docs/03` §8.2 第一阶段；「待决」3 |
| `Set{Table,Row,Cell,Section}Props` | `tblPrChange / trPrChange / tcPrChange / sectPrChange` | `spec/08` |
| `SetHeaderFooter` 内容 | 页眉 part 内按段落规则 del + ins | — |
| `InsertBlock{Image / Chart}` / `ReplaceImageMedia` | 段落规则 / 旧 run `w:del` + 克隆 run（换 blip）`w:ins` | 7.3 |
| **不追踪**（记 `REV_NOT_TRACKED` 诊断、照常执行）：书签 / 批注 / `ToggleCheckbox` / `LinkHeaderFooter` / 水印 / 页面底色 / 设置 / 图表数据 / part 替换 / 墨迹 / 绘图几何与样式（7.7）/ 分节符（7.6）/ `RemoveSdtShell` | — | Word 不记这些为修订，或另有机制 |

## 分层决策（实现前定死）

1. **修订是计划的一部分，不是事后包装**。`MutationPlan` 不加 `revisions` 字段（`docs/03` §8.3 写了 `revisions: Vec<NewRevision>`；
   登记 §8）：包裹 / 改名 / 快照都是普通 `NodeEdit`（`Insert / Move / Rename / SetAttr`），由 `Tracker` 在 plan 阶段生成；`w:id`
   在 plan 阶段从会话缓存的全包最大值预留（`apply_all` 一批共用一个分配器快照）；`w:date` 是 `RevisionAuthor.date` 原串——引擎里
   没有时钟（不变式 1 与可复现性）。
2. **同作者合并按 Word**：自己插的可以直接改、直接删；别人插的删了是 `w:ins` 里套 `w:del`；删除区里不能再打字。作者相等 =
   `w:author` 字符串相等（不看 `w:initials`）。
3. **`ModelFingerprint` 是 M7 的等价定义**：每个内容流的块序列（种类 / 表格几何 / 节边界）+ 每段坐标流文本 + 逐 UTF-16 单元的
   `rPr` 规范化字节（相邻同 `rPr` 的 run 合并后比较，所以拆 run 不算差异）+ 段落 `pPr` 规范化字节 + Span 索引（名字 / 种类 /
   偏移）+ 字段索引（指令 / 策略）；忽略 `NodeId / RevisionId / w:rsid* / w14:paraId`；`DelText` 段按「接受视图」（不含删除、含
   插入）与「拒绝视图」（含删除、不含插入）各算一份。放 `tests/common/fingerprint.rs`，`TEST-07` 与门 1 / 3 共用。
4. **Accept / Reject 是普通 `EditOp`**：走 plan / validate / commit；`AcceptAll` 是**一个**事务（一条计划），失败整体回滚；Reject
   `*PrChange` 用快照子元素的整体克隆而不是 typed `old` 做 patch（未建模子元素也要还原；typed `old` 只服务模型与 compat）。
5. **追踪只覆盖内容操作**（表见上）；不追踪的操作在 `track_changes = Some` 下照常执行并记 `REV_NOT_TRACKED`，不报错——编辑器
   开着修订时改页面颜色不该失败。
6. **`ReplaceInlines` 的 diff 只服务原生 API**：compat 路径的 `ins / del` 来自编辑器（`NewInline::Ins / Del`），`apply_save_blocks`
   永远 `track_changes = None`；diff 粒度到 run 边界，不做词级启发式。
7. **分节符 = 段落级 `sectPr` 克隆 + 原 `sectPr` 改 `w:type`**：不改原 `sectPr` 的页眉页脚引用（两节因此都「声明」同一 part，Word
   的「链接到前一节」会显示为关，但六槽有效值一致）；登记 §8。
8. **块字段生成器是纯函数**：输入 `Document` + 调用方给的页码，不在保存时自动运行、不猜页码（不做布局，`docs/03` §1.2）；`\h` /
   `PAGEREF` 是 Word 形态，`ts_shape` 只为夹具对照。
9. **绘图编辑只改属性或只换外壳**：`wp:extent` / `a:ext` / `posOffset` / `relativeHeight` / `a:xfrm` 属性改动让元素 `SelfDirty`，
   `a:graphic` 子树永远 `Clean` 搬家；z-order 归一是显式选项、缺省关（它会改未编辑图片的字节）。
10. **Fallback 孪生是派生物**：`mc:Fallback` 里的 VML 文本框内容永远由 Choice 内容生成，不可直接编辑（`Err(EDIT_TARGET_FALLBACK)`）；
    读侧（M4 `pict_kind` 读 Fallback VML）不变。
11. **空白模板是常量，不是随包文件**：逐字移植 TS `blank.ts`，parity 用 `blank-template__001` 的期望 JSON；将来改模板要同时改 TS
    并重导。
12. **`EditOp` 的新变体登记而不改冻结文档**：`AcceptAll { author }`、`RemoveSdtShell`、`RegenerateBlockField`、`InsertSectionBreak` /
    `DeleteSectionBreak`、`SetDrawing*` / `SetShapeStyle` / `SetTextboxContent`、`SetMathTokens`、`RemoveNote` 是 `docs/03` §8.2 之外的
    操作，按 M5 惯例（`LinkHeaderFooter / SetWatermark`）记进 `docs/04` §8；`SetParaStyle / SetList` 继续由 `SetParaProps` 的 patch
    覆盖，不另设。

## 实现约定：多用声明宏（用户要求，2026-09-05；与 `spec/14` / `spec/16` / `spec/17` 同一条）

修订域是历次里程碑里「表最多」的：21 种修订 × 两个方向、二十几个操作 × 追踪行为、五种原子、四个同形的属性型绘图操作。判断标准
仍是**同一形状重复三次以上就收成 `macro_rules!`**：

- `tracking_rules!`：上面那张表（操作 → `Tracked::{WrapRuns, MarkDeleted, Snapshot(kind), ParaMark, Diff, Reject(code), Never}`），
  同时展开 `Tracker::rule_for(&EditOp)` 与 `oracle_tests!`（对 `WrapRuns / MarkDeleted / Snapshot / ParaMark / Diff` 五类每操作一个
  `#[test]`，跑门 1 的三条 oracle）。
- `accept_reject!`：21 行（`RevKind` → Accept 动作 / Reject 动作），展开 `plan_accept / plan_reject` 的 `match` 与 `tests/revisions.rs`
  每种一个用例。
- `attr_op!`：7.7 四个「定位元素 → `SetAttr` 若干 → `SelfDirty` → 刷新所在块 → 同步 Fallback 孪生」同形操作。
- `atom_kinds!`：`NewAtom` 五种 → 生成器 + 「坐标流长度 1」的断言。
- 沿用：`set_some!` / `set_if!` / `display_json!`（compat 若有新字段）、`named_enum!`（`RevKind / RevOwner / SectType / BreakKind /
  PresetGeom / LineKind / Collation`）、`xpath_asserts!`、`fixture_tests!`（`fixtures/revisions` / `fixtures/fieldgen`）、`hf_slots!`、
  `boxed_reader!`。
- 不上宏：`InsertSectionBreak / DeleteSectionBreak` 两个；三个块字段生成器（形状不同：TOC 读结构、SEQ 数字段、INDEX 读 XE）。
- 宏带文档注释与 ```ignore 用例；跨模块用 `macro_rules!` + `pub(super) use`，展开里写 `$crate::…` 全路径；会把函数定义藏起来、
  让人跳不到声明处的，用共享模块而不是宏。

其余约定照旧：树遍历写成**迭代**（`rev-nested-wrappers` 500 层；`latexToOmml` 是用户输入的解析器，允许递归 + 深度上限）；属性
容器只走 `plan_apply_*`（段落标记 `rPr` 里的 `w:ins / w:del` 与 `*PrChange` 的位置由 `order` 表保证）；一个任务一个提交
`m7.<n>: 英文摘要 (SPEC-ID…)`；提交前同步 `docs/04` §16 勾选、§8 偏差表、`docs/05` 数字。

## 从 M0–M6 带过来的债（M7 内解决）

| 债 | 位置 | 解决任务 |
| --- | --- | --- |
| `EditContext.track_changes` 存在但被忽略 | `edit/mod.rs:46`、`edit/ops.rs` | 7.2 / 7.3 |
| `apply_save_blocks` 对 `run.rPrChange` 报 `EditUnsupported`（「在 M7」） | `bind/compat_ts/save_blocks.rs:1117` | 7.2 |
| `SPAN-07` MoveFrom / MoveTo / CustomXml 整体删除当作 `Remove` | `span/transform.rs`、`docs/04` §8 | 7.4 |
| `SPAN-10` 端点落在指令区未处理 | `span/transform.rs`、`docs/05`「明确未实现」 | 7.5 |
| `DeleteRange` 只支持同段 | `edit/ops.rs`、`docs/05` | 7.5 |
| `SetSectionProps` 不新建 `sectPr`（`Err(EDIT_BAD_POSITION)`） | `edit/section_ops.rs:273`、`docs/04` §8 | 7.6 |
| `MoveBlock` 跨 part `EditUnsupported`；`rehome_subtree`（`XML-12` E′）只剩规则文字 | `edit/ops.rs`、`xml/edit.rs` | 7.6 |
| `docs/03` §8.2 里缺的 `InsertAtom / SetNoteContent / SetSdtContent / Accept / Reject` | `edit/mod.rs` | 7.4 / 7.5 |
| `docs/05`「明确未实现」：compat 的 `instrField / fldBeginXml` 重发 | `bind/compat_ts/save_blocks.rs` | 7.8 复核后关掉 |
| `docs/06` 第 2 件（歧义探针）与第 3 件（`resolve` 与投影的对照） | `tests/resolve.rs` | 7.9 / 7.8 |
| `docs/04` §8「等价比较忽略 `xml:space`，M7 与 TS 全面差分时复核」 | `tests/save_blocks.rs` | 7.9 |
| `spec/17`「依赖与被阻塞」推给 M7 的四条（`latexToOmml` / `InsertAtom Math` / token 编辑；嵌入对象操作的修订；既有图片编辑与 z-order 回写） | — | 7.5 / 7.3 / 7.7 |
| `MutationPlan` 没有 `revisions` 字段（`docs/03` §8.3） | `edit/plan.rs` | 决策 1，登记 §8 |
| `RevisionMeta` 没有 `RevisionId`（`docs/03` §6.6） | `span/mod.rs`、`model/inline.rs` | 7.1（索引里给，登记 §8） |
| 空白文档模板没有归属里程碑 | — | 7.8 |

## 基线与复用

| 来自 | 复用什么 | 在哪个任务 |
| --- | --- | --- |
| M2 `span/transform.rs` | `SPAN-06` 四条变换、`split_run`、`SPAN-10` 插入侧规则 | 7.2 / 7.5 |
| M2 `edit/ops.rs` 的字段操作 | `InsertField` 的 run 生成、`UpdateBlockField` 的 separate..end 替换 | 7.5 / 7.8 |
| M3 `edit/table_ops.rs` | `DeleteColumn` 的 `gridSpan / tcW` 收缩算法（`cellDel` Accept 复用）、格尾 `w:p` 通则 | 7.3 / 7.4 |
| M3 `model/sdt.rs` | 四态锁 / 数据绑定判定 | 7.5 |
| M5 `edit/section_ops.rs`、`resolve/section.rs` | `SetSectionProps`、`RES-10` 六槽继承、`HfSlot::Declared / Inherited` | 7.6 |
| M5 `save/options/*` | 选项 → 操作的翻译骨架（`normalize_z_order` 照样接） | 7.7 |
| M5 `tests/hf_ops.rs` | `xpath_asserts!`、随机序列骨架（10 × 100 步） | 7.9 |
| M5 `fixtures/resolve` + `tools/gen-fixtures` | fixture 目录约定、`verified` 门、Word 操作单格式 | 7.0 / 门 3 |
| M6 6.7 `model/drawing.rs::to_anchor_xml`、`MediaStore::add`、`NewImage` | 环绕外壳生成、行内图片 | 7.7 / 7.5 |
| M6 6.5 `model/omml/` | OMML 结构与 token 读法（`SetMathTokens`）、夹具机制 | 7.5 |
| M6 6.9 `tests/embedded_ops.rs` | 多操作混合随机序列与包级检查（无悬空关系、无孤儿 part） | 7.9 |
| 生成的属性表 | `read_*_change`（typed `old`）、`plan_apply_*`、`order` 表（`w:ins` 在 `rPr` 首位、`*Change` 末位） | 7.2–7.4 |
| M1 `resolve/mod.rs::heading_level` | TOC 条目级别 | 7.8 |

## 依赖与被阻塞

| 事项 | 状态 |
| --- | --- |
| M6 并入 `main` | 7.0 的重导要和 M6 的语料一起做**一次**（`changedParts` 改导出格式，避免两次漂移）；M6 未完时 7.1–7.4 可以先在 `main` 上开（不碰 M6 的文件） |
| **真实 Word**：门 3 的三份 fixture；7.6 分节符 / 7.7 绘图编辑的人工核对（打开、可见、Word 能接受 / 拒绝我们生成的修订）；tracked `MergeCells` 与 `cellMerge` Reject 的形态 | 项目负责人操作；7.0 第一周把 `source.docx` 与操作单交出去，观察值回来前门 3 的测试只记不断言（5.8 同款） |
| `fixtures/fieldgen` 与 `latexToOmml` 夹具的 TS 输出 | 一次性脚本，依赖 genoffice 工作树（`GENOFFICE_DIR`） |
| 绑定形态（7.10） | 「待决」1 |
| `docs/07` 真实 Word 语料里的 `revisions/revisions-comments.docx` | 已在清单；进 `corpus/real` 后 7.1 的索引与 7.4 的 Accept / Reject 拿它做往返 |
| 语料里 `rPrChange` / 块级修订 / `cellMerge` / `numberingChange` / `delInstrText` 解析侧为 0 | 用构造文档补；**可选**：往 genoffice 的 `tests/revisions.test.ts` 加解析用例后重导（`TEST-02`），不作为门 |

## 不在 M7

- **分页 / 排版**：TOC 页码、`PAGE` 结果、题注编号之外的任何「重算」；页码来自调用方（`docs/03` §1.2）。
- **`moveFrom / moveTo` 的生成**（tracked `MoveBlock` 第一阶段拒绝，用 Delete + Insert）；**tracked `MergeCells`** 与 `cellMerge`
  的 Reject（等 fixture）。
- **`w:rsid*` 的生成**：我们的新节点不带 rsid（Word 接受；TS 也不写）。
- **比较文档**（两份文档生成修订）、修订上的批注回复、按时间筛选修订。
- **VML 形状 / WordArt 的新建与编辑**；SmartArt / 图表的**结构**编辑（M6 只做数据）。
- **`compat_ts` 的删除、原生 JSON `EditOp` 协议、媒体句柄**——M9。
- **编辑器接入与 e2e**——M8；`hashProtectionPassword`（编辑器继续调 TS 工具函数，`compat_ts` 存在期间不搬）。
- **`RES-04` 桌面版复核**——`docs/06` 第 1 件，项目负责人择机做，不占 M7 任务。

## 风险提示（实现前确认）

1. **Word 的修订形态比 ECMA 多**：`w:ins` 套 `w:del`、`w:del` 里的 `w:t`、只有一半的 `moveFrom`、`pPrChange` 里多个 `pPr`……门 1
   的 oracle 都是结构性的，只有门 3 的 Word fixture 能说「Word 认不认」。7.0 第一周就把 fixture 文档交出去。
2. **段落标记 `rPr` 是属性容器**：`pPr/rPr/w:ins` 必须按 `run.toml` 的 `order`（`w:ins / w:del / w:moveFrom / w:moveTo` 在第 9 行、
   首位）插入，否则调试构建的 `PROP-05` 自检直接报错；`w:rPrChange` 已在末位。
3. **`w:id` 全局唯一跨 part**：页眉 / 批注 / 注释 part 里的修订也占 id；会话缓存的最大值要在**任何** part 变脏时失效；`dup-ids`
   hostile 文档本身就重号，`SAVE-02` 只记诊断不改。
4. **追踪下 `DeleteRange` 不移动锚点、不改坐标流长度**——与不追踪的语义相反；`SPAN-06` 的删除规则在追踪路径上**不能**调用。
   `TEST-07` 会最先撞上这个。
5. **`ReplaceInlines` 的 diff 在坐标流上做，但输出要落到 DOM 节点**：一个 run 里部分相等部分不同要拆 run；标记按新坐标重发；
   字段原子只做整体等 / 不等。别做词级合并启发式。
6. **`AcceptAll` 的顺序**：先内层后外层、文档序——`w:ins` 里套 `w:del` 时先接受 `del`（内容消失）再接受 `ins`（解包空壳）；
   反过来会把 `w:del` 提到父级再处理，结果一样但计划里节点被搬两次。
7. **分节符改变 `Document.sections` 与 `block_range`**：5.2 的 `refresh_blocks` 已能重算，但 `section_of` 的查找与 compat 的
   `hidden` 块（尾部 `sectPr`）要重跑 `--scope hf` 与 `sections__*` 13 份确认无回归。
8. **Fallback 同步会让「改一段」变成「重写整个孪生」**：孪生内容是 `New` 深克隆，字节全变；这是 TS 也在做的事
   （`patchTextboxParas` 同步 fallback），而且孪生本来就是派生物。别试图在孪生上做局部补丁。
9. **z-order 归一缺省关**：开了会改所有锚定图片的 `relativeHeight`，不变式 2 的「其他块原字节」在这些 `wp:anchor` 上不成立；
   compat 只在 TS 会归一时（`imageZOrderNormalized`）开。
10. **`TEST-07` 的时间**：1,000 × 100 步 × `rebuild` oracle 在 CI 上可能要十几分钟——PR 跑 100 条，nightly 跑全量；`rebuild`
    只在每步 `affected_blocks` 非空时做全量对照，其余步骤只比 fingerprint 增量。
11. **`latexToOmml` 是递归下降**：用户输入短，深度上限 256 + `Err` 即可；不要为它写显式栈——那是文档遍历的规则，不是输入解析的。
12. **重导语料的漂移**：7.0 的 `changedParts` 要改导出器输出格式，按 `tools/export-golden/README`「重导的稳定性与噪音」核对 stem
    不漂、`outputSha256` 噪音还原；与 M6 的语料一起重导一次，不要两次。
13. **`blank()` 的字节要和 TS 一样才能拿 `blank-template__001` 当对照**：`internal.documentXml` 与 `extras.elements` 的偏移都在
    期望 JSON 里，模板差一个空格差分就不为 0；逐字移植，不「顺手整理」。

## 待决（需要项目负责人拍板）

| # | 事项 | 建议 |
| --- | --- | --- |
| 1 | 7.10 绑定进不进 M7；wasm-bindgen 还是 napi-rs | 建议进 M7（M8 的第一步就是它，不做 M8 无法开工）；建议 wasm-bindgen（与 TS 引擎同进程、无 native 构建矩阵），napi 留给性能不够时；接口一样，换实现不改调用方 |
| 2 | INDEX 排序的 collation | 建议 `CodePoint` 缺省 + 调用方可传排好的顺序；不引入 ICU（登记与 TS `localeCompare('zh-CN')` 的差异） |
| 3 | tracked `MoveBlock` 与 `MergeCells` 第一阶段拒绝 | 建议按 `docs/03` §8.2 拒绝；门 3 fixture 之后若 Word 形态清楚（`moveFrom / moveTo` 范围、`cellMerge` + `vMergeOrig`）再做 |
| 4 | `normalize_z_order` 缺省值 | 建议 false（不动未编辑字节）；compat 跟 TS 的 `imageZOrderNormalized` |
| 5 | `docs/03` §8.2 之外的新操作（决策 12 列出的十来个） | 建议按 M5 惯例登记 `docs/04` §8，不改冻结文档；M9 定原生协议时一并收进 `docs/03` 的下一版 |
| 6 | TOC 的 `\h` 与 `PAGEREF` 走 Word 形态（比 TS 多书签与超链接） | 建议做（编辑器点目录能跳转），`ts_shape` 只留给夹具；登记「超过 TS」 |
