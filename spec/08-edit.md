# SPEC 08 · L4 编辑引擎（edit/）

对应 `docs/03` 第 8 节。职责：把 `EditOp` 变成对 DOM + Span 的一次原子事务，维护 Anchor，按需生成修订，并给出投影刷新范围。

## EDIT-01 会话 API

```
EditSession::open(bytes) -> Result<EditSession>
session.document() -> &Document                       // 投影
session.apply(op: EditOp, ctx: &EditContext) -> Result<MutationResult>
session.apply_all(ops, ctx) -> Result<Vec<MutationResult>>   // 任一失败则整体不应用
session.add_media(bytes, mime) -> Result<MediaId>
session.save(opts) -> Result<Vec<u8>>
```

`EditContext { track_changes: Option<RevisionAuthor{author, date}>, default_run_props: Option<RunProps>, keep_orphan_comments: bool, mark_updated_fields_dirty: bool }`。

## EDIT-02 位置

- `InlinePos { para: NodeId, offset: Utf16Offset }`：`para` 是 `w:p` 节点（任何内容流），`offset` 是该段坐标流（`MOD-06`）中的 UTF-16 code unit 偏移，`0 ≤ offset ≤ len`。
- 定位算法 `locate(pos) -> Loc`：顺序累加 inlines 的 utf16 长度；落在 `Text/DelText` 段内部 → `Loc::InText{run, segment, byte_offset}`（**禁止**落在代理对中间：偏移指向低代理位时向前调整并返回 `Err(EDIT_SPLIT_SURROGATE)`）；落在两个 inline 之间 → `Loc::Boundary{index}`；原子（长度 1 的 `U+FFFC`）只能在其前或后，`offset` 指向其内部不可能（长度 1）。
- `BlockPos = Start(container) | After(block_node) | End(container)`。`container` 可为 `w:body`、`w:tc`、`w:sdtContent`、`w:txbxContent`、注释 / 批注条目——任何 `SPAN-01` 列出的块容器；`End(body)` 落在尾部 `sectPr` 之前，`End(tc)` 之后仍须以 `w:p` 结尾（`EDIT-03` 表格通则）。
- 位置在 `apply` 前解析；`MutationResult` 之后旧位置失效，调用方按 `MutationResult.offset_delta` 或重新查询。
- 实现错误码：`para` 不是会话中的可编辑文本段落、offset 越界或指向非文本原子内部 → `Err(EDIT_INVALID_POSITION)`；偏移落在代理对中间 → `Err(EDIT_SPLIT_SURROGATE)`。

## EDIT-03 操作语义

每个操作给出：前置条件、DOM 变更、脏标记、Anchor 变换（`SPAN-06`）、修订生成（`track_changes` 开启时）、结果。以下用 "→" 表示 DOM 变更。

**InsertText { at, text, props }**
- 前置：`text` 非空且不含 XML 非法字符（非法字符剔除并记诊断）；`at` 合法且不在原子形态字段内部。
- `props == None` 且 `at` 落在 `Text` 段内或紧邻一个 `Text` 段：把文本插入该 `w:t` 的文本节点（`Owned`）→ `w:t` `SelfDirty`（重生成以加 `xml:space="preserve"`），`w:r`、`w:p` `DescendantDirty`。
- 否则：在边界处插入 `New` `w:r`，其 `w:rPr` 为"继承格式"（左侧 run 的 rPr 字节克隆，或 `default_run_props`）合并 `props`；`w:t` 带 preserve。
- Anchor：按插入规则。
- 修订：新 run 包在 `New` `w:ins` 中（author/date/id）；若插入点位于同作者的 `w:ins` 内则直接插入该 `w:ins`。
- 若 `at` 在 `Link` 透明字段的结果内 → 插入结果 run（保持 `field`）。

**DeleteRange { from, to }**（同段）
- 覆盖的 `Text` 段部分 → 文本 `Owned` 截断；整段/整 run 被覆盖 → `Deleted`；覆盖原子形态字段 → 该字段 begin..end 全部 `Deleted` 并注销 `FieldSpan`；覆盖 `Drawing/Pict/Object` 段 → 所在 run 中该子节点 `Deleted`（run 若空则 `Deleted`）；覆盖 `FootnoteRef` → 同时删除 notes part 中的条目（`SetNoteContent` 语义）。
- Anchor：按删除规则；整体删除策略 `SPAN-07`。
- 修订：被删 run 包在 `w:del` 中，`w:t` 改名 `w:delText`（元素 `SelfDirty`）；位于同作者 `w:ins` 内的内容直接删除；已在 `w:del` 内的内容不重复包裹。
- 跨段：拆成 `DeleteRange`（首段尾部）+ `DeleteBlock`（中间段）+ `DeleteRange`（末段头部）+ `MergeWithNext`。

**SetRunProps { from, to, patch }**
- 在 `from/to` 处拆分 run（`split_run`：原 run 文本截断，右侧为 `New` run，rPr 为字节克隆）；对范围内每个 run 用 `PROP-06` 计划 rPr 变更。
- 修订：每个被改 run 的 rPr 追加 `New` `w:rPrChange{author,date,id}` 含旧 rPr 快照（旧 rPr 子元素字节克隆）；已有 rPrChange 时保留其旧值不重记。
- 原子形态字段内的 run 不受影响；对字段用 `SetFieldResultProps`。

**InsertAtom { at, atom }**：`Break`（`w:r/w:br`）、`Image`（`w:r/w:drawing` 由生成器产出，`wp:docPr/@id` 分配）、`Math`（`m:oMath` 由 LaTeX→OMML 产出）、`NoteRef`（创建 notes 条目 + `w:r/w:footnoteReference`）、`Symbol`。修订同 InsertText。

**InsertField { at, field }**：见 `FLD-12`。

**ReplaceInlines { para, inlines }**（compat 路径）：段落所有内容子节点 `Deleted`，新内容按 `NewInline` 生成 `New`；范围标记与字段结构由 compat 侧提供的 `NewInline::Marker/Field` 重发；`pPr` 不动。修订：`track_changes` 时按 diff 生成 ins/del（M7 后；M1 允许直接替换）。

**SplitParagraph { at }**
- `New` `w:p` 插在原段之后；`pPr` 为原 `pPr` 的 `Clean` 克隆（`XML-12` 规则 F）；`at` 之后的 inlines 移入新段（`move_within_part`，同 part 必兼容）；落在 `Text` 段内部 → 先 `split_run`。
- 原子形态字段不可拆：`at` 在其内部 → `Err`。透明字段跨段后成为 `Block` 策略（cross_paragraph）→ 拒绝：`Err(EDIT_SPLIT_FIELD)`。
- Anchor：拆分规则。
- 修订：原段 `pPr/rPr` 追加 `w:ins`（段落标记插入）。

**MergeWithNext { para }**
- 无 `track_changes`：下一段的 inlines 移入本段末尾，下一段 `Deleted`；范围标记随内容移动；下一段 `pPr` 丢弃（Word：合并后保留**前**段属性）。
- `track_changes`：**不合并**，在本段 `pPr/rPr` 追加 `w:del`（段落标记删除，Word 语义）；接受修订时才真正合并（`AcceptRevision`）。

**SetParaProps / SetParaStyle / SetList**：`PROP-06` 计划 `pPr` 变更（无 `pPr` → `New` 插为第一子）。修订：`pPrChange{author,date,id}` 含旧 `pPr` 子元素克隆（不含 `rPr`/`sectPr`/已有 `pPrChange`）。

**InsertBlock { at, block }**：`New` 子树；`NewBlock::Paragraph{props, inlines}` / `Table{...}` / `Xml(fragment)`（解析片段为 `New` 子树，命名空间按 `XML-14`）。修订：段落 → 内容 run 包 `w:ins` + 段落标记 `w:ins`；表格 → 每行 `trPr/w:ins`。

**DeleteBlock { node }**：`Deleted`；Anchor 按容器删除规则；修订：段落 → 内容包 `w:del` + 段落标记 `w:del`（段落保留为已删除）；表格行 → `trPr/w:del`。

**MoveBlock { node, to }**：同 part → `move_within_part`（`XML-12` E）；跨 part → `rehome_subtree`（E′）后源 `Deleted`。修订：Word 用 `moveFrom/moveTo`；第一阶段 `track_changes` 下拒绝 `MoveBlock`（`Err(EDIT_UNSUPPORTED_TRACKED_MOVE)`），用 Delete + Insert 代替。

**表格**（M3；`track_changes` 下的修订生成 → M7，见 `spec/14`）

通则：几何以**声明网格**为准——列数 = `tblGrid/gridCol` 数，行宽 = `gridBefore + Σ gridSpan + gridAfter`；任一行行宽 ≠ 列数的表格上，列操作与 `MergeCells` 返回 `Err(EDIT_TABLE_GRID_INCONSISTENT)`（**不**修 grid，由调用方决定）。行 / 格穿透 `w:sdt` 与修订包裹定位（`MOD-07`）。单元格最后一个块**必须**是 `w:p`：任何操作让单元格没有段落或以 `w:tbl` 结尾时，补一个 `New` 空 `w:p`。`gridSpan / vMerge / tcW / gridBefore / gridAfter` 的写入走 `plan_apply_cell_props / plan_apply_row_props`；`tblGrid` 不是属性容器，用 `NodeEdit` 直接增删 `gridCol`。

- `SetTableProps { table, patch }` / `SetRowProps { row, patch }` / `SetCellProps { cell, patch }`：`PROP-06`。新容器位置：`tblPr` 为 `w:tbl` 第一个子元素；`trPr` 在 `w:tblPrEx` 之后、第一个 `w:tc` 之前；`tcPr` 为 `w:tc` 第一个子元素。修订：`tblPrChange / trPrChange / tcPrChange`（M7）。
- `InsertRow { table, at, template }`：`template`（缺省为 `at` 的前一行；`at == 0` 时为第 0 行）的 `trPr`、`tblPrEx` 与各 `tcPr` 字节克隆（`XML-12` F），每格内容为一个空 `w:p`（克隆模板格首段的 `pPr`，含段落标记 `rPr`）；模板格 `vMerge` 为 continue → 新行该格去掉 `vMerge`；模板格 `vMerge restart` 且新行插在它与其 continue 之间 → 新行该格为 continue。修订：`trPr/w:ins`（M7）。
- `DeleteRow { table, at }`：`w:tr` `Deleted`；被删行某格为 `vMerge restart` 且下一行同列为 continue → 下一行该格改为 `restart`（合并区收缩，不能留下无头的 continue）；Anchor 按容器删除规则（`SPAN-07`）。修订：`trPr/w:del`（M7）。
- `InsertColumn { table, at, width }`：`tblGrid` 在 `at` 处插入 `gridCol`（`width` 缺省取左邻列宽，`at == 0` 取右邻）；每行：`at` 落在两格之间 → 插入 `New` `w:tc`（`tcPr` 克隆左邻格并把 `tcW` 设为 `width`、去掉 `gridSpan / vMerge`，内容一个空 `w:p` 克隆左邻格首段 `pPr`）；落在某格 `gridSpan` 中间 → 该格 `gridSpan + 1`（`tcW` 加 `width`）；落在 `gridBefore / gridAfter` 区间内 → 对应值 + 1。书签 / 权限范围的 `colFirst / colLast`（`SPAN-03`）≥ `at` 的 + 1。
- `DeleteColumn { table, at }`：`gridCol` `Deleted`；每行：恰好覆盖该列的格 `Deleted`（Anchor 按容器删除规则）；跨列格 `gridSpan − 1`（减到 1 去掉元素；`tcW` 减去该列宽）；`gridBefore / gridAfter` 覆盖处 − 1；`colFirst / colLast` > `at` 的 − 1，恰等于 `at` 的范围收缩，`colFirst == colLast == at` → 范围整体删除。某行只剩这一格 → 拒绝：`Err(EDIT_TABLE_GEOMETRY)`（应改用 `DeleteBlock` 删表）。
- `MergeCells { table, from: (r, c), to: (r, c) }`：网格坐标闭区间，须为矩形且不与既有合并区（`gridSpan` / `vMerge`）部分交叠，否则 `Err(EDIT_TABLE_GEOMETRY)`。横向：每行区间内第一格 `gridSpan = 区间宽`，其余格的内容（末尾空段除外）按文档序 `move_within_part` 到第一格末尾后整格 `Deleted`；纵向：首行的格 `vMerge restart`，其余行的格 `vMerge`（continue）**保留元素**，内容搬到首行格后留一个空 `w:p`（Word 的 OOXML 形态）。范围标记随内容移动（`SPAN-06` 合并规则）。修订：`cellMerge`（M7）。
- `InsertBlock { at, block: NewBlock::Table { rows, cols, widths, style, header } }`：生成 `tblPr`（`tblStyle` 可选、`tblW type=auto`、`tblLook w:val="04A0"` 及等价属性）、等分或给定的 `tblGrid`、每格一个空 `w:p`；`header` 为 true 时首行 `trPr/tblHeader`。整棵 `New`；落在单元格内时遵守"格尾是 `w:p`"通则。

**字段**：`SetFieldResultProps`（对 `result` run 走 SetRunProps 逻辑）、`ToggleCheckbox`/`SetFormText`（`FLD-10`）、`SetLinkTarget`（重写 instrText 的 `Owned` 文本，或 `w:hyperlink` 的 `r:id` 目标关系/`w:anchor`）、`UpdateBlockField`（`FLD-09`）。

**Span**
- `AddBookmark { name, from, to }`：分配 `w:id`（`EDIT-06`）；名字重复 → `Err`；新 `RangeSpan` 与 `New` 标记。
- `RemoveBookmark { span }`：标记 `Deleted`，索引移除。
- `AddComment { from, to, comment }`：`comments.xml` 不存在则创建 part（关系、内容类型）；`New` 条目（`w:comment[@w:id,@w:author,@w:date,@w:initials]` 含段落，`w14:paraId`）；范围标记 + `w:commentReference` run（`CommentReference` 样式）；`commentsExtended.xml` 条目（回复/done）按需。
- `RemoveComment`、`SetCommentText { span, text, done }`。

**修订** `AcceptRevision/RejectRevision { rev }`：

| 修订 | Accept | Reject |
| --- | --- | --- |
| Insert（run 级） | `w:ins` 解包：子节点 `move_within_part` 到父，`w:ins` `Deleted` | 内容 `Deleted` |
| Delete（run 级） | 内容 `Deleted` | 解包，`w:delText` 改名 `w:t` |
| Insert/Delete（块级） | 同上作用于块 | 同上 |
| ParaMarkInsert | 删除 `pPr/rPr/w:ins` | 合并本段与下一段（无追踪的 Merge） |
| ParaMarkDelete | 合并本段与下一段 | 删除 `pPr/rPr/w:del` |
| RunPropsChange | 删除 `rPrChange` | rPr 子元素替换为 `rPrChange/rPr` 的克隆，删除 `rPrChange` |
| ParaPropsChange | 删除 `pPrChange` | `pPr` 子元素（除 rPr/sectPr）替换为旧值克隆 |
| MoveFrom/MoveTo（成对） | MoveFrom 内容 `Deleted`；MoveTo 解包；范围标记删除 | MoveTo 内容 `Deleted`；MoveFrom 解包 |
| SectPropsChange/TablePropsChange/TableGridChange/RowPropsChange/CellPropsChange | 删除 `*Change` | 旧值克隆替换 |
| CellInsert/CellDelete/CellMerge | 删除标记 / 删除单元格（需重算 grid） | 反之 |
| NumberingChange | 删除 | 恢复旧 numPr |
| FieldInstrDelete | `delInstrText` 节点 `Deleted` | 改名 `instrText` |

`AcceptAll/RejectAll` 按文档序逐个应用，先内层后外层。

**节与页眉页脚**
- `SetSectionProps`：`PROP-06` 于 `sectPr`；修订 `sectPrChange`。
- `SetHeaderFooter { sect, kind, variant, content }`：part 不存在 → 新建 part（`word/headerN.xml`，关系、内容类型、`sectPr` 的 `headerReference`）；存在 → 其内容替换（`ReplaceBlocks`）。跨 part 的内容用 `rehome`。

**其他 part**：`SetNoteContent`、`SetSdtContent`（`ContentLocked` → `Err(EDIT_SDT_LOCKED)`；有 `data_binding` → `Err(EDIT_SDT_BOUND)`，第一阶段）、`SetChartData`、`SetDocumentSettings`、`ReplacePartXml` / `ReplacePartBytes`。

- `SetChartData { part, patch: ChartPatch { title, categories, series } }`（`chart.ts` 补丁语义，M6 6.6）：**只改缓存文本**——标题取 `c:title` 里第一个 `a:t`（其余 `a:t` 清空），没有 `a:t` 则 `c:strCache/c:v`，两者都没有（自动标题）→ 在 `c:tx/c:rich/a:p` 的 `a:endParaRPr` 之前注入 `a:r/a:t`，`c:tx` 是无缓存 `strRef` → 整个换成 rich body，没有 `c:tx` → rich body 插为 `c:title` 第一个子元素；系列名 → `c:ser/c:tx` 下第一个 `c:v`；值 → `c:val` 缓存点按 `idx` 改，**缺的点不补**；类别 → 每个系列的 `c:cat` 都改。数据引用 `c:f`、样式、布局一个字节不动；chartex part → `Err(EDIT_UNSUPPORTED)`。验收：`tests/chart_ops.rs`。
- `ReplacePartXml { part, xml }` / `ReplacePartBytes { part, bytes }`（TS `partXml` / `partBinary`）：整 part 替换，只接受已存在的 part（不存在 → `Err(EDIT_TARGET_MISSING)`），新 XML 经解析成为该 part 的新 DOM，主 part 不能按二进制换；事务回滚覆盖它们。

## EDIT-04 SaveBlock 兼容映射（第一阶段）

| SaveBlock | EditOp |
| --- | --- |
| `{kind:'original', docxIndex}` | 无操作；顺序变化 → `MoveBlock` |
| `{kind:'generated', block}` | `ReplaceInlines` + `SetParaProps`（由 `format`/`rawPPr` 决定）+ 类型变化（`SetParaStyle`/`SetList`）；`sdtShell` 由 DOM 天然保留 |
| `{kind:'xml', xml, docxIndex?}` | `InsertBlock{Xml}`（替换 `docxIndex` 对应块时先 `DeleteBlock`） |
| 缺失的 original 块 | `DeleteBlock` |
| `SaveOptions.section/header/footer/notes/comments…` | 对应操作 |

## EDIT-05 事务

```
plan(op, &session) -> Result<MutationPlan>      // 只读
plan.validate(&session) -> Result<()>            // 只读：id 分配、命名空间、relationship、Anchor 预测、schema 顺序
session.commit(plan) -> MutationResult           // 机械写入，不可失败
document.refresh(&result)
```

- `apply_all` 把多个操作的 plan 逐个 validate 到一个临时会话视图上（或顺序 plan/commit 并在失败时回滚快照）；任一失败 → 整批不生效。计划引用不存在的 part / 节点 / `Target::New`，或 `before` 不是目标父节点的子节点 → `Err(EDIT_INVALID_PLAN)`，且在任一写入前返回。
- 所有 id 分配在 plan 阶段完成（`EDIT-06`）。
- `MutationResult.offset_delta: Vec<(NodeId /*para*/, Utf16Offset /*from*/, i32 /*delta*/)>` 供调用方修正光标。

## EDIT-06 id 分配

| id | 唯一范围 | 规则 |
| --- | --- | --- |
| `rId` | part 的 `.rels` | `rId{max+1}`，跳过已用 |
| 书签 `w:id` | part | `max+1`；名字全文档唯一 |
| 批注 `w:id` | 文档（`comments.xml`） | `max+1` |
| 修订 `w:id` | 文档（所有 part 的所有 `w:id` 修订属性） | 全局 `max+1`，plan 阶段预留区段 |
| `wp:docPr/@id`、`pic:cNvPr/@id` | part | `max+1` |
| `w14:paraId` | 文档 | 随机 32 位，`< 0x80000000`，非 0 |
| 脚注/尾注 `w:id` | 各 part | `max+1`（分隔符占 -1/0） |
| 媒体文件名 | 包 | `media/image{N}.{ext}`，`N` 为现有最大 +1 |
| 新 part 名 | 包 | `header{N}.xml`、`footer{N}.xml`、`charts/chart{N}.xml` |

## 验收清单

| ID | 用例 |
| --- | --- |
| EDIT-02 | 偏移落在 😀 中间 → `Err`；PAGE 原子前后偏移差 1 |
| EDIT-03 InsertText | 在干净 run 中间插字 → 只有该 `w:t` SelfDirty；`w:p` 开标签字节不变；追踪时出现 `w:ins` |
| EDIT-03 DeleteRange | 删除覆盖书签起点 → 起点移到删除点；删除覆盖整个 REF 字段 → begin..end 全部消失 |
| EDIT-03 Merge | 追踪时合并 → 段落仍分开，`pPr/rPr/w:del` 出现；Accept 后真正合并 |
| EDIT-03 InsertRow | 新行的 `tcPr` 与模板行字节相同；模板格 `vMerge continue` 在新行中消失 |
| EDIT-03 DeleteRow | 删掉 `vMerge restart` 行 → 下一行同列出现 `w:vMerge w:val="restart"` |
| EDIT-03 InsertColumn | 落在 `gridSpan=2` 格中间 → 该格 `gridSpan=3`、该行 `w:tc` 数不变、`tblGrid` 多一列；覆盖该列的书签 `colLast` +1 |
| EDIT-03 DeleteColumn | 删掉跨列格覆盖的一列 → `gridSpan` 减 1，减到 1 时元素消失；其他行该列的 `w:tc` 消失 |
| EDIT-03 MergeCells | 2×2 合并 → 左上格 `gridSpan=2` + `vMerge restart`，第二行左格 `gridSpan=2` + `w:vMerge`，右列两格消失，四格文字按文档序出现在左上格，被并入格内的书签仍成对 |
| EDIT-03 表格通则 | 行 gridSpan 总和 ≠ 列数的表格做 `InsertColumn` → `Err(EDIT_TABLE_GRID_INCONSISTENT)` 且 DOM / Span / Model 与操作前一致；`DeleteBlock` 删掉格内唯一段落 → 格内出现 `New` 空 `w:p` |
| EDIT-03 Accept/Reject | 每种修订各一用例，结果 XPath 断言 |
| EDIT-05 | 构造在第 3 步失败的批操作 → DOM/Span/Model 与操作前完全一致（`rebuild` 相等） |
| EDIT-06 | 连续两次 AddComment 得到不同 `w:id`，`comments.xml` 有两条 |
