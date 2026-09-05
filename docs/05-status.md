# 05 · 现状快照（2026-09-05）

这份文档回答"现在能做什么、不能做什么、数字是多少"。任务清单在 `docs/04-dev-plan.md`，规范在 `spec/`。
数字都可以用文末的命令复现；改动代码后请一并更新这里。

## 结论

**M0 完成，M1 完成**（1.1–1.15 全部落地，M1 门三条都有测试覆盖），**已全部并入 `main`**（2026-09-04）。
**M2 完成并已并入 `main`**（2026-09-05，fast-forward 21 条提交；2.1–2.10 全部落地，M2 门两条都跑过）：
Span 索引与 Anchor 变换 / 物化（2.1–2.3）、字段子系统与它的模型 / compat（2.4–2.5）、
批注与注释（2.6：读侧 + 三个编辑操作 + `SAVE-05` 新建 part + compat 权威条目列表）、
符号字体（2.7）、`rId` 分配（2.8）、段落 / 书签 / 字段操作（2.9）、`fuzz_instr` 与 M2 门（2.10）。
任务分解见 `spec/13-m2-plan.md`，逐条进度见 `docs/04` §11。
**M4（绘图）另有一个并行会话在做**（工作树 `../rsWordParser-m4`，分支 `m4-drawing`），所以下一个
没人认领的里程碑是 **M3（表格）**——任务分解见 `spec/14-m3-plan.md`，分支 `m3-tables`（工作树 `../rsWordParser-m3`），
逐条进度在 `docs/04` §12。

现在这套代码能：打开任意语料文档、输出与 TS 兼容的 `ParsedDoc` JSON、以字节级局部补丁写回并保证
未编辑内容零改动；在文本段落上插入 / 删除 / 改 run 与段落属性 / 整段替换 / 拆分 / 合并；维护范围
（书签 / 批注 / 权限 / 移动）的 `Anchor` 并在保存时物化标记；解析字段（复杂 / 简单 / 嵌套 / 跨段）、
定策略、进模型并按 TS 形态输出，且能编辑它们（插入字段、改链接目标、切换复选框、改表单文字、
改结果格式、换块字段的结果）；建批注与注释条目并按需**新建 part**（`SAVE-05`）；把 TS 的
`SaveBlock[]` 与 `comments` / `footnotes` / `endnotes` 保存选项翻成编辑操作。
**不能**：表格与绘图的模型与操作（M3 / M4）、页眉页脚 / 节 / 水印 / 图表等保存选项（M5 / M6）、
块字段生成器与修订生成（M7）。

## 能力矩阵

| 层 | 状态 | 已实现 | 缺口（里程碑） |
| --- | --- | --- | --- |
| L0 包层 `package/` | 完成 | zip（0x7075 中和、限额、raw copy）、`[Content_Types].xml`、`.rels` 双族、flavor 判定（Strict / Transitional / Mixed）、`NamespaceContext`、新建 part（`SAVE-05`：内容类型 Override + 关系 + `.rels` 自建） | — |
| L1 无损 DOM `xml/` | 完成 | tokenizer（区间精确、属性顺序 / 引号 / 重复容忍）、`Dirty` 五态与传播、MCE（含 `ProcessContent`）、命名空间作用域、`NodeEdit` 计划、片段解析、规范化比较、XPath 子集 | — |
| L2 范围 `span/` | 完成（范围部分） | `FlowId` / `FlowMap`、内容序列（`SPAN-01`）、`Anchor` / `Affinity`、九种 `RangeKind`、按流构建与配对诊断、文档序 `compare`、按容器倒排、编辑期变换与整体删除策略（`SPAN-06/07`）、物化与保存前校验（`SPAN-08/09`）、拆分 / 合并容器时的锚点重定位 | `SPAN-10` 的另一半：范围端点**落在字段指令区内**时移到原子边界（插入侧已按同一规则处理，端点侧还没做） |
| L2 字段 `span/field/` | 完成 | `FieldSpan` 配对（复杂 / 简单 / 嵌套 / 跨段 / 未闭合诊断）、指令 tokenizer 与 76 个关键字的策略表、`w:ffData` 读写、`FLD-13` 基线校验、`fuzz_instr` | 块字段生成器（`FLD-09` 的内容重算，M7） |
| L3 属性表 `semantic/props/` | 完成 | 27 张表由 TOML 生成（读 / 写 / diff / patch / merge / `plan_apply_*`）、按 flavor 编解码、`Val::Raw` 降级、`PROP-05` 顺序；表格三组表 `TableProps`（含 `tblPrEx`）/ `RowProps` / `CellProps` 与边框 / 边距子表、`MeasureOrPercent` codec（3.1） | 节的属性表（M5） |
| L3 模型 `model/` | 文本 + 表格 + 字段 + 批注 / 注释 | `Document::rebuild`、块分类 R01–R19（含 R09 字段块）、段落坐标流（`Run`/`Segment`，UTF-16）、`Inline::Field` 与透明字段、`ParagraphFacts`、**表格模型**（`TableBlock / Row / Cell`，穿透 sdt 与修订包裹，声明网格，表格修订，> 64 层 TooDeep，`MOD_TABLE_SHAPE` 诊断；3.2）、跨表格的 `blocks()` / `paragraphs()` / `block_path()`、**内容控件**（`SdtInfo`：16 种控件 / 四态锁 / 数据绑定 / docPart / 占位符；3.3）、声明模型（styles / numbering / theme / settings / fontTable / comments / footnotes / endnotes） | 绘图显示模型（M4） |
| resolve `resolve/` | 首版 + 表格 | 样式链（basedOn / link）、docDefaults 层叠、每字段 `Provenance`、主题字体与颜色、符号字体解码、heading 级别；**表格视图**（`tblLook`、表格样式链的条件格式、边框 / 边距回退、行高截断、`ColumnView` 的四条列宽启发式与 `hMerge` 折叠、`RES-03` 第 4 层；3.4） | toggle 属性真实规则 + Word 实测 fixture（M5）、补全 Wingdings 2/3 与 Webdings 映射表 |
| L4 编辑 `edit/` | 段落 / 范围 / 字段操作齐了，单元格内可编辑 | `EditSession`（含范围索引）、`InlinePos` 定位、`MutationPlan` plan/validate/commit、按 part 回滚的事务（DOM + 索引）、`InsertText`、`DeleteRange`（同段）、`SetRunProps`、`SetParaProps`、`ReplaceInlines`、`ReplaceParaProps`、`InsertBlock`/`DeleteBlock`/`MoveBlock`、`SPAN-06/07` 锚点维护、`AddComment`/`RemoveComment`/`SetCommentText`（含 `SAVE-05` 新建 part）、`SplitParagraph`/`MergeWithNext`、`AddBookmark`/`RemoveBookmark`、`InsertField`/`SetLinkTarget`/`ToggleCheckbox`/`SetFormText`/`SetFieldResultProps`/`UpdateBlockField`；**单元格内编辑**（`InlinePos.para` 可为任意深度的 `w:p`，容器级刷新，格尾自动保持 `w:p`；3.6）、`SetTableProps`/`SetRowProps`/`SetCellProps`（3.7）、**行列结构操作**（`InsertRow`/`DeleteRow`/`InsertColumn`/`DeleteColumn`/`MergeCells`/`NewBlock::Table`，声明网格几何 + 书签列区间维护；3.8） | 块字段生成器与修订生成（M7） |
| 保存 `save/` | 六步齐了 | `SAVE-01` 六步编排（含第 3 步 Span 物化）、`SAVE-02` 子集校验（未绑定前缀、`PROP-05` 顺序、`SPAN-09` 范围检查）、`SAVE-05` 新建 part（追加在 zip 末尾）、扩展命名空间声明、`w:t` preserve、`raw_copy_file` 写回、`SaveOptions`（`saved_at`、`remove_personal_info`、`remove_date_and_time`、批注与注释的权威列表） | 节 / 页眉页脚 / 水印 / 图表 / 墨迹等选项（M5 / M6） |
| 兼容 `bind/compat_ts/` | 文本 + 表格 + 字段 + 批注 / 注释 | `parsed_doc` 整份 `ParsedDoc`（含 `extras`、UTF-16 索引、sdt 拆分）、字段折叠 run 与 `fieldDisplay` / `fieldLabel`、`comments` / `footnotes` / `endnotes` / `commentIds` / `noteRef`、`apply_save_blocks`（original / generated / xml 块）、容忍差分；**表格模型**（`blocks[*].table` 全部字段与 `styles.*.tableDisplay`，含 TS 的 `attachRawTablePr` / 深度 8 扁平化 / `tableSummary` 三处半解析；3.5） | 绘图 / 页眉页脚字段（随对应里程碑） |

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
| 源码行数 / 文件数 | 34,197 行 / 92 个（另有生成代码 16,793 行） | `find crates tools -name '*.rs' \| xargs wc -l` |
| 测试数 | 314（单元 + 集成，23 个集成测试文件） | `cargo test --workspace` |
| 语料 | 573 份 synthetic（每份带 `expected.json`）+ 162 份 `save.<k>.json` + 16 份 hostile | `ls corpus/*` |
| 往返字节保真 | 589 份文档、3,093 个 XML part 全部字节相同 | `tests/xml_roundtrip.rs` |
| 声明模型对照 | 2,897 个样式、6,732 项主题颜色等，1 处已知差异 | `tests/decl.rs` |
| 模型对照 | 445 段类型 / styleId、387 段坐标流文本、22 项列表、9 项级别 | `tests/model.rs` |
| resolve 对照 | 86,465 项 `StyleDisplay`、2,326 项 heading 级别、2,897 项 linked shell | `tests/resolve.rs` |
| 解析差分（文本域） | 226 份用例（2.6 起含带批注 / 注释的文档），156 处已登记差异，**0 处未知差异** | `cargo run -p diff-parse -- --scope text` |
| 解析差分（字段与 Span 域，M2 门） | 253 份用例（文本域 + 字段 / 标记 / 批注 / 注释），**0 处未知差异** | `cargo run -p diff-parse -- --scope fields` |
| 解析差分（表格域，M3 门） | 311 份用例（字段域 + 表格；单元格里的绘图归 M4 剔除），**0 处未知差异** | `cargo run -p diff-parse -- --scope tables` |
| 解析差分（全域） | 573 份里 238 份有未知差异、1,615 个差异点（M4–M6 的工作面） | `cargo run -p diff-parse -- --scope all` |
| 保存差分 | 162 份 TS 保存用例：90 份与 `saveDocx` 等价（其中 41 份逐字节相同）、4 份有意不同、68 份跳过 | `tests/save_blocks.rs` |
| 范围索引 | 573 份 / 3012 个 part 的 31 个标记全部成对认领 → 19 个范围（书签 7、批注 12）；1 处孤儿终点 | `tests/span.rs` |
| Span 编辑与物化 | 29 个用例覆盖 `SPAN-01`–`SPAN-09`（含 4 条变换规则、整体删除策略、物化与原字节保真） | `cargo test -p rsword --test span` |
| 字段索引 | 43 份文档 / 57 个字段（`Atom` 33、`Block` 6、`Picture` 6、`Form` 4、`Link` 3、`Object` 3、`Marker` 1、`Unknown` 1）；3 份 TS 截断夹具本来就缺 `end` | `cargo test -p rsword --test field -- --nocapture` |
| 字段模型与 compat | 19 个用例（配对 / 指令 / 策略 / 坐标流 / R09 / 折叠 run / `fieldDisplay`） | `cargo test -p rsword --test field` |
| 段落 / 书签 / 字段操作 | 17 个用例（`SPAN-06` 拆分与合并、`EDIT-06` 书签分配、`FLD-09`/`10`/`12` 各自的验收行） | `cargo test -p rsword --test para_ops` |
| 批注与注释 | 语料 11 份带批注（17 条）、5 条注释条目；13 个用例（三部件关联、结构条目、`commentIds` 三形态、`noteRef` 编号、`SAVE-05` 新建 part、三个编辑操作、compat 权威列表） | `cargo test -p rsword --test notes` |

全域差异按域聚合（差异点，2.6 读侧之后实测 1,666）：绘图与图片 839、块分类连带项 394、
run 相关 153（批注已归零，剩的是绘图与页眉页脚里的 run）、页眉页脚 146、表格 67、其他 67。保存侧 68 份跳过按里程碑（实测）：M5 43 份（页眉页脚 13 + 节 9 + 水印 5 + 参考文献 3 + 编号 3 +
保护 3 + 主题 2 + 页码 1 + 奇偶页眉 2 + 页面颜色 1 + 写保护 1）、M4/M6 11 份（图表 6 + 图片 4 +
`partXml` 1）、墨迹 8 份、`replaceImage` 1 份、样式 upsert 1 份、其余 4 份。

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
| 权威列表删掉批注后 | 空掉的 `commentReference` run 整个删掉 | TS 留一个 `<w:r></w:r>`（`comments__001.save.2` 因此不等价，见 `INTENTIONAL`；共 4 份） |

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
  `.rels` 都能建，内容类型 Override 与关系同步写，新 part 追加在 zip 末尾）。还没有的：页眉页脚与
  图表 part（随 M5 / M6）。
- **表格**：模型（3.2）与属性表（3.1）已在；`resolve` 视图、compat 的 `blocks[*].table`、单元格内编辑与行列操作还没有（3.4–3.8）。**绘图**：块层面是占位（`Image` / `Protected`），图片属性不进模型（M4）。
- **保存选项**：已支持 `savedAt` / `removePersonalInfo` / `removeDateAndTime` / `comments` / `footnotes` /
  `endnotes`；节、页眉页脚、水印、页面颜色、页码、编号、样式 upsert、保护、主题、墨迹、图表、
  `partXml` 仍是 `EditUnsupported`。
- **修订生成**：`EditContext.track_changes` 字段存在但被忽略（M7）。

## 如何验证

```sh
cargo fmt --all --check && cargo clippy --workspace --all-targets   # 零告警
cargo test --workspace && cargo test --workspace --release          # 314 个测试，两种构建
cargo run -p diff-parse -- --scope text                             # M1 门第一条：0 未知差异
cargo run -p diff-parse -- --scope fields                           # M2 门：字段与 Span 域 0 未知差异
cargo run -p diff-parse -- --scope tables                           # M3 门：表格域 0 未知差异
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
- toggle 属性（bold / italic 等的层叠语义）用的是占位规则，需要 Word 实测 fixture 校准（M5）。
- 语料在 Span / 字段这两个域上很薄：573 份里只有 15 份带范围标记（31 个标记、19 个范围）、43 份带字段
  （57 个）。M2 的行为正确性主要靠 `tests/{span,field,notes,para_ops}.rs` 的 78 个单元用例，不能只看
  差分数字。
- `compat_ts` 是负担性代码，删除期限定在 M9。
