# SPEC 14 · M3 任务分解

对应 `docs/03` 第 12 节 M3 行。格式同 `spec/12` / `spec/13` / `spec/15`：每个任务给出产出、依赖的规范条目与
完成定义（DoD）。顺序即建议的实现顺序；同一编号内的子任务可并行。排期依据（实测差距）见下文与
`docs/04` §10；现状数字见 `docs/05-status.md`。

M3 与 M4（`spec/15`，工作树 `../rsWordParser-m4`）并行开发：表格不依赖绘图，绘图不依赖表格；两者在
单元格内相遇的地方（单元格里的锚定形状与图片）归 M4，见「不在 M3」。M3 从 `main`（含 M2）开分支
`m3-tables`，工作树 `../rsWordParser-m3`。

## M3 · L3 表格：属性表、模型、resolve 视图、compat 投影、容器级编辑

目标：`w:tbl` 从占位块变成真实模型（`MOD-07`），`tblPr / trPr / tcPr` 进属性表并可按 `PROP-06` 合并写回，
表格样式条件格式与列宽视图走 `resolve`（`RES-08`），`compat_ts` 输出 TS 的 `TableModel` 与
`tableDisplay`；单元格内的段落可用全部既有编辑操作，且行 / 列 / 合并操作可用。

**M3 门**（`spec/11` TEST-10 「M3–M6 对应域 diff 为 0」的具体化）：

1. `cargo run -p diff-parse -- --scope tables` 0 未知差异（`--scope fields` 的文档集 ∪ 含表格块的文档，
   剔除单元格内含锚定形状 / 图片 / 公式 / ruby 的文档——那些是 M4 的域；`TEST-03`）；
   `--scope text` 与 `--scope fields` 继续为 0。
2. **改单元格文本往返**（`docs/03` §12 M3 验收）：对每份含表格的语料，在一个单元格段落里 `InsertText`
   后保存——其他 zip 条目 CRC 与压缩字节不变、`document.xml` 里其他块原字节原样、重解析后只有该
   单元格的 `paras` 变（`TEST-04` 的单节点编辑扩展到单元格段落）。
3. `corpus/hostile/xml-deep-table.docx`（5000 层）解析成功、深于 64 层为 `Protected(TooDeep)`、
   无编辑保存字节相同（`TEST-09`）。
4. 表格结构操作的随机序列 200 步 × 10 份语料无失败（`TEST-07` 的表格子集，3.9）。

### 实测差距（2026-09-05，`main` = 624d4f0，`cargo run -p diff-parse -- --scope all --json`）

全域 1,666 处未知差异 / 295 份文档。表格域直接命中 **67 处 / 67 份**——路径全部是 `blocks[*].table`
**整个对象缺失**（`compat_ts` 今天只给 `type / label / previewText`），所以这 67 处是 67 张表的**入口**，
不是工作量：把 `table` 发出去之后，差异会按子字段展开。工作面按语料实测：

| 量 | 值 | 备注 |
| --- | --- | --- |
| 含表格块的文档 | 69 份（2 份 `char-unit-indents__*` 整份已登记） | 前缀：`table-display` 17、`table-edit` 12、`bugfix-regressions` 6、`table-grid-reconcile` 6、`cell-anchored-boxes` 5、`table-style` 3、`deep-nested-table` / `nested-table-edit` / `table-revisions` / `field-display` / `vml-textbox` 各 2、其余单份 |
| 表格 / 嵌套表 / 单元格 | 85 / 15 / 199 | TS 折叠 `hMerge` 与补 `gridGap` 之后的单元格数 |
| `richParas[].runs` | 4,205 个 run | 单元格段落复用段落投影，这部分不是新代码 |
| `styles.*.tableDisplay` | 22 个叶子（`band1Fill` 8、`firstRow` 8、`paraSpacing` 2、`firstCol` / `wholeTable` / `borders` / `cellMarTwips` 各 1） | 今天整条放行在 `KNOWN_DIFFS`，M3 删掉 |
| 与 M4 相遇 | `cell-anchored-boxes__*` 5 份（`anchoredBoxes` / `anchoredBoxAnchors`）、单元格 run 图片 4 份 | 不进 M3 门 |
| 保存语料 | 7 份 `*table*.save.*.json`（`deep-nested-table__001`、`nested-table-edit__001/002`、`table-display__009`、`table-revisions__001`、`table-style__001`） | 全是 `original` / `xml` 块：TS 在**客户端**用 `generateTableModelXml` / `patchTableCellTexts` 生成整表 XML 再以 `kind:'xml'` 提交，`EDIT-04` 现有路径已覆盖，M3 保存侧**没有** compat 工作 |

TS `TableModel` 字段在语料里的出现次数（85 张表 / 199 格；决定 3.5 的字段清单与先后）：

| 表级 | 次数 | 格级 | 次数 |
| --- | --- | --- | --- |
| `rows` | 85 | `paras` / `richParas` | 199 / 197 |
| `autoFit` / `repeatHeaderRows` / `tableLook` | 84 | `rawTcPr` | 99 |
| `colWidthsPct` / `colWidthsTwips` | 76 | `fill` / `color` / `bold` | 21 / 16 / 13 |
| `autoLayout` | 70 | `nestedTables` / `nestedTableAnchors` | 14 |
| `tblStyleId` | 10 | `colSpan` | 10 |
| `borders` | 7 | `align` | 9 |
| `rawTrPrs` | 6 | `anchoredBoxes` / `anchoredBoxAnchors`（M4） | 5 |
| `cellMarTwips` / `indentTwips` / `fixedLayout` / `floatPos` / `floatSide` | 4 | `vMerge` | 4 |
| `rowHeightRules` / `rowHeightsTwips` | 3 | `textDirection` / `borders` / `gridGap` / `cellRevision` | 2 |
| `bidiVisual` / `align` / `rowRevisions` | 2 | `hMerge` / `cellMarTwips` / `vAlign` | 1 |
| `widthPct` / `cellSpacingTwips` / `fill` | 1 | | |

`docs/01` §3.5 的 `TableModel` 摘要少了 `autoFit`、`fixedLayout`、`cellSpacingTwips`、`fill`、`tableLook`、
`repeatHeaderRows`、`gridGap`、`anchoredBoxAnchors`（基线之后 TS 加的）；权威定义是 genoffice
`src/types.ts` 的 `TableModel` / `TableCell`，规则在 `COMPAT-10`。

### 任务

| # | 任务 | 规范 | DoD |
| --- | --- | --- | --- |
| 3.1 | **表格属性表**：`schema/props/table.toml`（`TableProps` + 子表 `TblBorders`（6 边）/ `TblCellMar`；struct `TblWidth{w,type}`、`TblLook`（6 个属性 + `val`）、`TblpPr`（全部 11 个属性））、`row.toml`（`RowProps` + struct `TrHeight{val,hRule}`、`CnfStyle`）、`cell.toml`（`CellProps` + 子表 `TcBorders`（8 边）/ `TcMar`；`cellIns/cellDel/cellMerge` 的 `RevisionMeta` 属性）；`types.toml` 增 `TblWidthType`、`Merge`、`VerticalJc`、`TextDirection`、`TblLayoutType`、`TblOverlap`、`JcTable`、`HAnchor`、`VAnchor` 枚举；`w:tblPrEx` 复用 `TableProps` 表读取；`styles.toml` 的 `tbl_pr / tr_pr / tc_pr` 从 `Raw` 改为这三张表（`TableStyleDecl` 有类型） | PROP-01/02/04/05/07/08/09 | 三张表每行 `PROP-07` 往返（两种 flavor）；语料全部 `w:tblPr / w:trPr / w:tcPr / w:tblPrEx` read → emit → read 建模字段全等，`PROP_BAD_VALUE` 逐条列出并解释；`PROP-05` 三张表的顺序单调率写进 docs/05；`tests/props.rs` 增三张表 |
| 3.2 | **表格模型**（`model/table.rs`）：`TableBlock { node, props, grid: Vec<GridCol>, rows, style_id, sdt, revisions }`、`Row { node, props, tbl_pr_ex, cells, sdt, revisions }`、`Cell { node, props, blocks, sdt, revisions }`；行 / 格穿透 `w:sdt` 与 `w:ins/w:del/w:customXml` 包裹取得；`Cell.blocks` 复用 `build_container`（段落 / 嵌套表 / sdt / 修订包裹）；**迭代**构建，嵌套 > 64 层 → `Protected(TooDeep)` + `MOD_TOO_DEEP`；`Document::paragraphs()` 迭代到单元格内段落，`Document::block_path(node)` 给任意块 / 段落的祖先路径；修订：`TablePropsChange / TableGridChange / RowPropsChange / CellPropsChange / CellInsert / CellDelete / CellMerge` 与行级 `trPr/ins\|del` 各附到 `MOD-09` 指定的位置 | MOD-07, MOD-09, MOD-12, MOD-13 | `MOD-07` 验收行（sdt 包裹的 tr/tc 解析出行列；65 层第 65 层 TooDeep）；hostile `xml-deep-table` 通过 `TEST-09` 行；全语料每张表的行数与 TS `rows.length` 一致、每行物理 `w:tc` 数 = TS 格数 + 折叠的 `hMerge continue` − `gridGap` 占位；`table-revisions__*` 的行 / 格修订附着位置正确（`MOD-09` 验收行的表格部分）；`tests/table.rs` 新建 |
| 3.3 | **`SdtInfo` 完整模型**：`alias / tag / id / control`（16 种）`/ lock / data_binding / doc_part / placeholder / showing_placeholder`；块级与 run 级 sdt 同一读取器；`compat_ts` 的 `sdtShell.controlType` 四值映射（TS 顺序：`w:date` → `date`、`w:dropDownList|w:comboBox` → `dropdown`、`w:checkbox` → `checkbox`、其余 → `text`）；编辑策略：目标在 `ContentLocked / SdtContentLocked` 的 sdt 内 → `Err(EDIT_SDT_LOCKED)`，有 `data_binding` → `Err(EDIT_SDT_BOUND)`（第一阶段） | MOD-08, EDIT-03 | `MOD-08` 验收行（`w:dataBinding` + `w:lock sdtContentLocked`）；`sdt__*` 与含 sdt 的语料 diff 仍为 0；两条 `Err` 各有测试且状态不变（`EDIT-05`） |
| 3.4 | **`resolve` 表格视图**（`resolve/table.rs`）：`Resolver::table(&TableBlock) -> TableView`；`tbl_look`（属性 > `w:val` 位 > 缺省）；表格样式链（`tblStyle` 的 basedOn，`TableStyleDecl` 的条件块）；`cell(r, c) -> EffectiveCellProps`（底纹、8 边边框、边距、`vAlign`、`textDirection`、条件 rPr / pPr 叠加）带 `Provenance::TableStyle{style, cond}`；条件优先级 firstRow > lastRow > firstCol > lastCol > 条带 > 整表 > 样式链；边框 / 边距回退（文档 `tblPr` → 样式链 → 缺省 上下 0 / 左右 108）；`columns() -> ColumnView { widths_twips, source: Grid \| TcW \| Stretched \| Reconciled, spans, gaps }` 实现 TS 的四条列宽启发式并标来源；`hMerge continue` 折叠视图；`trHeight` 上限 31680；`RES-03` 第 4 层（表格样式 rPr）接进 `resolve::run` | RES-01, RES-03, RES-08, RES-12 | `RES-08` 验收行（`tblLook 04A0`）；`table-style__*` 的五个场景各一单测（firstRow + 条带按 tblLook、`noHBand` 与 `firstRow=0`、显式底纹胜样式、basedOn 链、`paraSpacing` 深合并）；`table-grid-reconcile__*` 六份的列数与 `colWidthsTwips` 与 TS 一致；`fixtures/resolve/table/` 至少 1 个 |
| 3.5 | **`compat_ts` 表格投影**（`bind/compat_ts/table.rs`）：`blocks[*].table` 全部字段与 `styles.*.tableDisplay`，规则见 `COMPAT-10`；深度 ≥ 8 的嵌套表 → TS `flattenedTableModel`（1×1、`autoLayout`、全部段落文本按文档序，直接读 DOM 文本，不依赖被 TooDeep 截掉的模型）；`richParas` 复用段落投影（`ParaFormat` + `runs` + `styleId` + `list` + `emptyRun*`）；`rawTcPr / rawTrPrs` 取原字节区间；`anchoredBoxes*` 与格内 run 图片不做（M4）；`diff-parse --scope tables`（`TEST-03`）并接 CI | COMPAT-02, COMPAT-10, TEST-03 | `blocks[*].table` 67 → 0（M4 路径所在文档被 scope 剔除，在 `--scope all` 里仍算未知，归 M4）；`KNOWN_DIFFS` 删掉 `styles.*.tableDisplay*`；`--scope tables` 0 未知差异；`--scope all` 的文档数 / 差异点数更新到 docs/05 |
| 3.6 | **容器级刷新与单元格内编辑**：`Document::refresh` 按 `affected_containers` 经 `block_path` 定位顶层块，原位重建单元格段落 / 嵌套表 / sdt 并重算聚合，不再退化为整体重建；`FieldIndex / SpanIndex` 只重建受影响的 part；`InlinePos.para` 可为任意深度的 `w:p`（定位表按 `paragraphs()` 建）；`BlockPos` 容器可为 `w:tc`；**单元格最后一个块必须是 `w:p`**——`DeleteBlock` 删掉末段或 `InsertBlock{Table}` 落在格尾时补 `New` 空 `w:p`（`EDIT-03`） | MOD-13, EDIT-01, EDIT-02, EDIT-03, TEST-04 | `MOD-13` 验收（单元格内随机编辑后 `refresh == rebuild`）；`tests/save.rs::test_04_corpus_edit_fidelity` 扩到单元格段落（M3 门第 2 条）；`edit/session.rs` 「不在顶层 → 整体重建」的退路删除；既有九个段落 / 块操作在单元格内各至少一个用例 |
| 3.7 | **表格属性操作**：`SetTableProps { table, patch }`、`SetRowProps { row, patch }`（新增）、`SetCellProps { cell, patch }` 走 `plan_apply_*`；新容器位置：`tblPr` 为 `w:tbl` 第一个子元素、`tcPr` 为 `w:tc` 第一个子元素、`trPr` 在 `w:tblPrEx` 之后 / 第一个 `w:tc` 之前（`PROP-06` 第 1 步的表格规则，`docs/04` §8 挂账）；`tblGrid` 不在此改（3.8）；`track_changes` 下的 `*PrChange` 快照 → M7 | EDIT-03, PROP-05, PROP-06 | 三张表各一条 `PROP-05` / `PROP-06` 验收行（新元素按序插入、未建模子元素原字节不动、容器开标签不变）；TS `table-edit` 三个场景各有等价单测：改 `vAlign` / `tcBorders` 时 `rawTcPr` 里未建模属性保留、写 `tblHeader` 不动 `trPr` 其他子元素、`tblStyle` 的替换 / 新增 / 删除（新增时缺 `tblLook` 补缺省） |
| 3.8 | **行列结构操作**：`InsertRow { table, at, template }`、`DeleteRow`、`InsertColumn { table, at, width }`、`DeleteColumn`、`MergeCells { table, from, to }`、`InsertBlock { NewBlock::Table { rows, cols, widths, style } }`（TS `generateTableXml` 等价生成器）；几何以**声明网格**为准：`tblGrid` 列数 × 每行 `gridBefore + Σ gridSpan + gridAfter`；不一致的表格上做列操作 → `Err(EDIT_TABLE_GRID_INCONSISTENT)`（**不**偷偷修 grid）；`MergeCells` 非矩形或与既有合并区交叠 → `Err(EDIT_TABLE_GEOMETRY)`；书签 / 权限的 `colFirst / colLast` 随列增删更新（`SPAN-03`）；`SAVE-02` 新增对 `New` / 脏表格的网格一致性检查（`SAVE_TABLE_GRID`） | EDIT-03, EDIT-05, EDIT-06, SPAN-06, SAVE-02 | `EDIT-03` 验收行（InsertRow 新行 `tcPr` 与模板字节相同）+ 每个操作一组 XPath 断言（`TEST-05`）；操作后 `refresh == rebuild`；`DeleteRow` 删掉 `vMerge restart` 行后下一行的 `continue` 改成 `restart`；`InsertColumn` 落在某格 `gridSpan` 中间 → 该格 `gridSpan + 1` 而不是插 `tc`；`DeleteColumn` 删到跨列格 → `gridSpan − 1`（为 1 时去掉元素）；`MergeCells` 被并入格的内容按文档序并入主格、范围标记跟随（`SPAN-06` 合并规则）；三个 `Err` 用例 DOM / Span / Model 与操作前一致（`EDIT-05`）；`tests/table_ops.rs` 新建 |
| 3.9 | **随机序列、恶意输入与 M3 门**：`tests/table_ops.rs` 加小型随机序列（格内 `InsertText / DeleteRange / SetCellProps / InsertRow / DeleteRow / InsertColumn / DeleteColumn / MergeCells`，200 步 × 10 份语料，每步 `refresh == rebuild` 且 `SAVE-02` 无 `EngineInvariantViolation`，每 20 步保存 + 重解析）；`corpus/hostile` 补 `table-grid-mismatch.docx`（行 gridSpan 总和 ≠ 列数）与 `table-cell-no-paragraph.docx`（`w:tc` 没有 `w:p`）；CI 加 `diff-parse --scope tables` | TEST-07, TEST-09, TEST-10 | 200 × 10 无失败，失败用例最小化后固化；两份 hostile 解析成功、记 `PreExistingDamage` 诊断、无编辑保存字节相同、列操作返回 `EDIT_TABLE_GRID_INCONSISTENT`；`.github/workflows/ci.yml` 多一步且绿 |

**顺序说明**：3.1 → 3.2 是主干（模型要属性表）。3.2 之后三路并行：3.4（resolve）、3.5（compat，依赖 3.4 的
`tableDisplay` 与列宽视图）、3.6（刷新与格内编辑）。3.7 只要 3.1 + 3.2；3.8 要 3.6（并入内容要搬块）与
3.7（写 `gridSpan / vMerge / tcW` 走属性表）。3.3 独立，可随时插。3.9 收尾。3.5 是**门**的第 1 条，
建议 3.2 一落地就先把 `rows / paras / richParas / rawTcPr / colSpan / vMerge` 这批「不需要 resolve」的
字段发出去，让差分数字尽早开始往下走。

## 从 M1 / M2 带过来的债（M3 内解决）

| 债 | 位置 | 解决任务 |
| --- | --- | --- |
| 投影刷新遇到不在正文顶层的段落（单元格内）退化为整体 `rebuild` | `edit/session.rs`、`model/build.rs::refresh_paragraphs` | 3.6 |
| 每次段落刷新都全 part 重建 `FieldIndex` / `SpanIndex`（`docs/04` §8 `MOD-01` 行） | `model/build.rs` | 3.6 |
| `PROP-06` 第 1 步缺 `trPr`（在 `tblPrEx` 之后）等表格容器的新建位置规则（`docs/04` §8 `PROP-06` 行） | `semantic/props` 生成器 / `edit/ops.rs` | 3.7 |
| `TableStyleDecl.tbl_pr / tr_pr / tc_pr` 是 `Raw`，`RES-08` 的边框 / 边距回退拿不到值 | `schema/props/styles.toml` | 3.1 |
| `KNOWN_DIFFS` 整条放行 `styles.*.tableDisplay*`；`TableBlock` 注释写着「行列模型在 M2」 | `bind/compat_ts/KNOWN_DIFFS.md`、`model/block.rs` | 3.5 / 3.2 |
| `Document::text_blocks()` 只遍历顶层，测试与 compat 看不到单元格段落 | `model/build.rs` | 3.2 |

## 不在 M3

- **单元格里的绘图**：`anchoredBoxes / anchoredBoxAnchors`、格内 run 的 `image`、TS 在 `extractCell` 里
  剥掉锚定 drawing 再重新解析段落的那套——M4（`spec/15` 4.6）。`--scope tables` 剔除这些文档。
- **页眉页脚里的表格**（`COMPAT-05` 的 `HfParagraph.cells`、浮动表延后输出）——M5。
- **修订生成**：`tblPrChange / trPrChange / tcPrChange`、`trPr/w:ins|w:del`、`cellIns / cellDel / cellMerge` 的
  生成与 `Accept / Reject`——M7。M3 只**解析并保留**它们（`MOD-09`）。`EditContext.track_changes` 继续忽略。
- **`TableModel → XML` 整表再生成**（TS `generateTableModelXml` / `patchTableCellTexts` 的原生等价物）——
  M7 的 `table-edit` 场景。M3 提供的是底层原语（格内段落操作 + 行列操作 + 属性操作），组合出来就是它。
- **`SplitCell`**、表格转文字、排序——不在 `docs/03` §8 的 `EditOp` 里。
- **列宽 / 浮动位置的排版语义**：`floatSide` 的「`tblpX > 4680` 算右侧」、`reconcileGridColumns` 的
  96 列上限、stale grid 拉伸，都是显示层启发式，只出现在 `resolve` 视图与 `compat_ts`，**不进模型**
  （`MOD-11` 的禁令同样适用于表格）。

## 实现约定（本里程碑特别强调）

1. **多用声明宏**（用户要求，2026-09-05）。表格域的样板比 M1 / M2 都多：6 边 / 8 边边框、4 边边距、
   `tblLook` 六个开关、TS 的五个「与行对齐」数组（`rowHeightsTwips / rowHeightRules / repeatHeaderRows /
   rawTrPrs / rowRevisions`）、格级 `fill / color / bold` 的「自身未设才补」。判断标准仍是**同一形状重复三次
   以上就收成 `macro_rules!`**：
   - 投影层新字段一律用 `bind/compat_ts/json.rs` 已有的 `set_some!`（有值才写）/ `set_if!`（为真才写），
     不再手写 `if let Some`；显示枚举用 `model/macros.rs` 的 `named_enum!`。
   - 预期新增：`sides!`（对一组边名同时展开读 / 写 / 合并，边框与边距共用）、`look_flags!`
     （`tblLook` 属性名 / 位 / 缺省三元组一张表）、`row_aligned!`（按行收集并「全 `None` 则不输出」）。
   - 宏带文档注释与 ```ignore 用例；跨模块用 `macro_rules!` + `pub(super) use`，展开里写全路径
     `$crate::…`；不要用宏藏起函数定义（让人跳不到声明处的不写宏，用共享模块）。
2. **表格代码放新文件**，尽量不碰 M4 正在改的 `model/block.rs`、`model/build.rs`、`bind/compat_ts/blocks.rs`
   的现有函数体：`model/table.rs`、`resolve/table.rs`、`bind/compat_ts/table.rs`、`edit/table_ops.rs`、
   `tests/{table,table_ops}.rs`。必须改的共享点（`Block::Table` 的字段、`build_container` 的 `w:tbl` 分支、
   `body_block` 的 `w:tbl` 调用、`diff-parse` 的 scope）改动越小越好，方便两边合并。
3. **树遍历一律迭代**（`deep-nested-table` 2000 层、hostile 5000 层；递归 = 测试 SIGABRT）。
4. **属性容器只经 `plan_apply_*` 改**；`tblGrid` 不是属性容器，用 `NodeEdit` 直接增删 `gridCol`，但
   `gridSpan / vMerge / tcW / gridBefore / gridAfter` 的写入必须走 `plan_apply_cell_props / _row_props`。
5. **模型只存声明值**：`grid` 是 `gridCol/@w:w` 原值（允许 0 与缺失）、`hMerge` 不折叠、`trHeight` 不截、
   `tcW` 不校正；折叠与校正在 `resolve` / `compat_ts`。
6. 一个任务一个提交：`m3.<n>: English subject (SPEC-IDs)`；提交前更新 `docs/04` §12 与 `docs/05` 的数字。

## 风险提示（实现前确认）

1. **与 M4 的合并冲突**：`m4-drawing` 从 M2 之前的 `main` 分出，已改 `model/block.rs`（`ImageBlock.display`）、
   `model/build.rs`、`bind/compat_ts/blocks.rs`、`classify.rs`、`docs/04`、`docs/05`、`CLAUDE.md`、
   `spec/00`。两个里程碑谁先并入 `main`，另一个就要 rebase。缓解：约定 2（新文件）；`spec/00` 的索引行
   只加自己那一行；`docs/05` 的能力矩阵只改「表格」一格；先并入的一方通知另一方。
2. **深嵌套的两个上限不一致**：模型 64 层 `TooDeep`，TS 8 层 `flattenedTableModel`。compat 复现 TS 的
   扁平化时要拿到被截掉那部分的**全部段落文本**（`deep-nested-table__001` 是 2000 层），所以 3.5 的
   扁平化直接读 DOM（`Ctx::plain_text` 一类，迭代），不能只看模型。这不是「重新解析 XML」，符合 `COMPAT-03`。
3. **网格不一致是真实输入**：`table-grid-reconcile__*` 六份语料的行 gridSpan 总和与 `tblGrid` 不一致
   （生成器留下过时 grid、掉了 gridSpan 的行）。读侧靠 `resolve` 的 `Reconciled` 视图给出可显示的列宽；
   **写侧**不能在这种表上做列操作（会把错误固化成新的 `w:tc`），所以 3.8 要求先检查一致性并返回 `Err`，
   由调用方决定是否先 `SetTableGrid` 修正——修正本身不在 M3。
4. **`MergeCells` 的内容搬运**：Word 把被并入格的段落原样追加到主格（保留 `pPr`），被并入格删除；
   `vMerge continue` 的格在 OOXML 里**仍然存在**（带空段落）。所以纵向合并不删 `tc`，只改 `tcPr`
   并搬走内容留一个空 `w:p`；横向合并才删 `tc`。搬运走 `move_within_part`（`XML-12` E），范围标记按
   `SPAN-06` 合并规则跟随。实现前先用 Word 造一份合并前后的文档对照。
5. **表格样式里的 toggle 属性**（`firstRow` 的 `w:b` 与段落样式 / 直接格式的叠加）：`RES-04` 的真实规则
   与 Word 校准 fixture 在 M5。M3 的 `EffectiveCellProps.rpr` 先按 `RES-03` 的普通覆盖顺序做，compat 的
   `cell.bold / color` 只复现 TS 的「自身未设才补」，并在代码里标注 `RES-04` 待校准。
6. **语料在结构操作上是空的**：69 份文档全是读侧；`InsertRow / InsertColumn / MergeCells` 没有任何 TS
   参照（TS 的行列编辑在编辑器里做，引擎只收整表 XML）。正确性完全靠 3.8 的单测与 3.9 的随机序列
   + `SAVE-02` 网格检查；写 DoD 里的 XPath 断言时以 ECMA-376 §17.4 与 Word 实际输出为准，不以 TS 为准。
7. **`w:tblPrEx`**（行级表格属性例外，Word 合并两张表时产生）：`SPAN-01` 已把它列为属性元素，模型以
   `Row.tbl_pr_ex: Option<TableProps>` 保留声明值；`resolve` 在该行优先于 `tblPr`。语料里没有实例，
   补一个 hostile / synthetic 用例再做。
