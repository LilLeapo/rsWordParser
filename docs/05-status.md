# 05 · 现状快照（2026-09-04）

这份文档回答"现在能做什么、不能做什么、数字是多少"。任务清单在 `docs/04-dev-plan.md`，规范在 `spec/`。
数字都可以用文末的命令复现；改动代码后请一并更新这里。

## 结论

**M0 完成，M1 完成**（1.1–1.15 全部落地，M1 门三条都有测试覆盖），**已全部并入 `main`**（2026-09-04）。
**M2 进行中**（分支 `m2-span-fields`）：2.1 Span 索引、2.2 Anchor 变换、2.3 物化与保存校验、
2.4 字段子系统、2.5 字段进模型与 compat、2.7 符号字体解码、2.8 的 `rId` 分配已落地；任务分解见
`spec/13-m2-plan.md`，进度清单见 `docs/04` §11。

现在这套代码能：打开任意语料文档、输出与 TS 兼容的 `ParsedDoc` JSON、在文本段落上做插入 / 删除 / 改
run 属性 / 改段落属性 / 整段替换、把 TS 的 `SaveBlock[]` 保存请求翻成编辑操作、以字节级局部补丁写回，
并保证未编辑内容零改动；范围标记（书签 / 批注 / 权限 / 移动）在编辑时会被正确变换与物化；字段
（复杂 / 简单 / 嵌套 / 跨段）解析出来、定策略、进模型，并按 TS 形态输出（折叠 run 与 `fieldDisplay`）。
**不能**：字段的编辑操作（插入 / 改链接目标 / 切换复选框 / 更新目录）、表格与绘图的模型、页眉页脚 /
节 / 图表等保存选项、修订生成。

## 能力矩阵

| 层 | 状态 | 已实现 | 缺口（里程碑） |
| --- | --- | --- | --- |
| L0 包层 `package/` | 完成 | zip（0x7075 中和、限额、raw copy）、`[Content_Types].xml`、`.rels` 双族、flavor 判定（Strict / Transitional / Mixed）、`NamespaceContext` | 新建 part（`SAVE-05`，M2） |
| L1 无损 DOM `xml/` | 完成 | tokenizer（区间精确、属性顺序 / 引号 / 重复容忍）、`Dirty` 五态与传播、MCE（含 `ProcessContent`）、命名空间作用域、`NodeEdit` 计划、片段解析、规范化比较、XPath 子集 | — |
| L2 范围 `span/` | 完成（范围部分） | `FlowId` / `FlowMap`、内容序列（`SPAN-01`）、`Anchor` / `Affinity`、九种 `RangeKind`、按流构建与配对诊断、文档序 `compare`、按容器倒排、编辑期变换与整体删除策略（`SPAN-06/07`）、物化与保存前校验（`SPAN-08/09`） | 与字段的交界（`SPAN-10`：端点落进指令区时移到原子边界，随 2.9 的字段操作） |
| L2 字段 `span/field/` | 解析完成 | `FieldSpan` 配对（复杂 / 简单 / 嵌套 / 跨段 / 未闭合诊断）、指令 tokenizer 与 76 个关键字的策略表、`w:ffData` 读侧、`FLD-13` 基线校验 | 字段编辑操作（2.9）、块字段生成器（M7） |
| L3 属性表 `semantic/props/` | 完成 | 20 张表由 TOML 生成（读 / 写 / diff / patch / merge / `plan_apply_*`）、按 flavor 编解码、`Val::Raw` 降级、`PROP-05` 顺序 | 表格与节的属性表（M3 / M5） |
| L3 模型 `model/` | 文本 + 字段完成 | `Document::rebuild`、块分类 R01–R19（含 R09 字段块）、段落坐标流（`Run`/`Segment`，UTF-16）、`Inline::Field` 与透明字段、`ParagraphFacts`、声明模型（styles / numbering / theme / settings / fontTable） | 表格模型（M3）、绘图显示模型（M4） |
| resolve `resolve/` | 首版 | 样式链（basedOn / link）、docDefaults 层叠、每字段 `Provenance`、主题字体与颜色、符号字体解码、heading 级别 | toggle 属性真实规则 + Word 实测 fixture（M5）、补全 Wingdings 2/3 与 Webdings 映射表 |
| L4 编辑 `edit/` | M1 子集 + Anchor | `EditSession`（含范围索引）、`InlinePos` 定位、`MutationPlan` plan/validate/commit、按 part 回滚的事务（DOM + 索引）、`InsertText`、`DeleteRange`（同段）、`SetRunProps`、`SetParaProps`、`ReplaceInlines`、`ReplaceParaProps`、`InsertBlock`/`DeleteBlock`/`MoveBlock`、`SPAN-06/07` 锚点维护 | 字段操作、拆分 / 合并段落（M2）、表格操作（M3）、修订生成（M7） |
| 保存 `save/` | M1 子集 + Span | `SAVE-01` 六步编排（含第 3 步 Span 物化）、`SAVE-02` 子集校验（未绑定前缀、`PROP-05` 顺序、`SPAN-09` 范围检查）、扩展命名空间声明、`w:t` preserve、`raw_copy_file` 写回、`SaveOptions`（`saved_at`、`remove_personal_info`、`remove_date_and_time`） | 节 / 页眉页脚 / 水印 / 图表 / 墨迹等选项（M3–M6） |
| 兼容 `bind/compat_ts/` | 文本 + 字段完成 | `parsed_doc` 整份 `ParsedDoc`（含 `extras`、UTF-16 索引、sdt 拆分）、字段折叠 run 与 `fieldDisplay` / `fieldLabel`、`apply_save_blocks`（original / generated / xml 块）、容忍差分 | 表格 / 绘图 / 页眉页脚字段（随对应里程碑） |

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

// TS 保存路径
let outcome = bind::compat_ts::apply_save_blocks(&mut s, &final_blocks_json, &options_json)?;
let bytes = s.save_with(&outcome.save_options)?;
```

错误契约：只有"根本不是 docx / 缺主 part / 超限 / 主 part 畸形 / 编辑位置或计划非法 / 引擎不变式被破坏"
返回 `Err`；其余一律局部降级并记 `Diagnostic`。不支持的输入返回 `Error::Edit { code: EditUnsupported }`，
**不改动任何状态**，调用方可以据此判断"这个能力还没到"。

## 实测数字

| 指标 | 值 | 来源 |
| --- | --- | --- |
| 源码行数 / 文件数 | 24,189 行 / 61 个（另有生成代码 16,793 行） | `find crates tools -name '*.rs' \| xargs wc -l` |
| 测试数 | 226（单元 + 集成，15 个集成测试文件） | `cargo test --workspace` |
| 语料 | 573 份 synthetic（每份带 `expected.json`）+ 162 份 `save.<k>.json` + 16 份 hostile | `ls corpus/*` |
| 往返字节保真 | 589 份文档、3,093 个 XML part 全部字节相同 | `tests/xml_roundtrip.rs` |
| 声明模型对照 | 2,897 个样式、6,732 项主题颜色等，1 处已知差异 | `tests/decl.rs` |
| 模型对照 | 445 段类型 / styleId、387 段坐标流文本、22 项列表、9 项级别 | `tests/model.rs` |
| resolve 对照 | 86,465 项 `StyleDisplay`、2,326 项 heading 级别、2,897 项 linked shell | `tests/resolve.rs` |
| 解析差分（文本域） | 223 份用例，160 处已登记差异，**0 处未知差异** | `cargo run -p diff-parse -- --scope text` |
| 解析差分（全域） | 573 份里 311 份有未知差异、1,709 个差异点（M3–M6 的工作面） | `cargo run -p diff-parse -- --scope all` |
| 保存差分 | 162 份 TS 保存用例：78 份与 `saveDocx` 等价（其中 41 份逐字节相同）、3 份有意不同、81 份跳过 | `tests/save_blocks.rs` |
| 范围索引 | 573 份 / 3012 个 part 的 31 个标记全部成对认领 → 19 个范围（书签 7、批注 12）；1 处孤儿终点 | `tests/span.rs` |
| Span 编辑与物化 | 29 个用例覆盖 `SPAN-01`–`SPAN-09`（含 4 条变换规则、整体删除策略、物化与原字节保真） | `cargo test -p rsword --test span` |
| 字段索引 | 43 份文档 / 57 个字段（`Atom` 33、`Block` 6、`Picture` 6、`Form` 4、`Link` 3、`Object` 3、`Marker` 1、`Unknown` 1）；3 份 TS 截断夹具本来就缺 `end` | `cargo test -p rsword --test field -- --nocapture` |
| 字段模型与 compat | 19 个用例（配对 / 指令 / 策略 / 坐标流 / R09 / 折叠 run / `fieldDisplay`） | `cargo test -p rsword --test field` |

全域差异按域聚合（差异点，2.5 之后）：绘图与图片约 830、块分类连带项 394、页眉页脚 157、表格 67、
run 相关（批注 / 符号字体）约 90；字段显示只剩 1 处（文本框分支，M5）。保存侧 82 份跳过按里程碑：
M5 约 48、M4/M6 约 20、M2 约 16、M7 2。

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

解析侧的已登记差异（数字 / 符号字体 / 表格显示等）在 `crates/rsword/src/bind/compat_ts/KNOWN_DIFFS.md`。

## 明确未实现

- **字段**：`DeleteRange` 覆盖字段结构段（`fldChar` / `instrText` / `commentReference`）时整 run 保留、只删文本段，并记 `EDIT_ANCHOR_UNMOVED`（M2 2.4 的 `FieldSpan` 接管）。
- **字段**：解析、进模型与 compat 输出都在（`FLD-01`–`FLD-08`、`MOD-06`、`COMPAT-03/07`）；缺的是
  **编辑操作**——`InsertField` / `SetLinkTarget` / `ToggleCheckbox` / `SetFormText` / `UpdateBlockField`
  与 `refField` / `xeTerm` / 表单域的 `SaveBlock` 仍是 `EditUnsupported`（2.9），块字段生成器在 M7。
- **新建 part**：缺 `settings.xml` 时清洗标志写不进去（记诊断）；批注 / 脚注 part 不能创建；
  part 没有 `.rels` 时也建不出来（新外链会因此报 `EditUnsupported`）——都等 2.6 的 `SAVE-05`。
- **表格 / 绘图**：块层面是占位（`Table` / `Image` / `Protected`），单元格与图片属性不进模型。
- **保存选项**：节、页眉页脚、水印、页面颜色、编号、样式 upsert、保护、主题、墨迹、图表、`partXml` 全部 `EditUnsupported`。
- **修订生成**：`EditContext.track_changes` 字段存在但被忽略（M7）。

## 如何验证

```sh
cargo fmt --all --check && cargo clippy --workspace --all-targets   # 零告警
cargo test --workspace && cargo test --workspace --release          # 163 个测试，两种构建
cargo run -p diff-parse -- --scope text                             # M1 门第一条：0 未知差异
cargo test -p rsword --test edit                                    # M1 门第二条：其他条目 CRC 不变
cargo test -p rsword --test save_validate                           # M1 门第三条：Strict 改字仍 Strict
cargo test -p rsword --test save_blocks -- --nocapture              # 保存差分明细与跳过原因
```

## 债务与风险

- `DeleteRange` 对字段结构段"原地保留"是临时行为，等 M2 的 `FieldSpan`。
- 表格单元格内段落的投影刷新目前退化为整体重建，M3 做容器级刷新。
- 语料导出自 genoffice `f105f36` **加 32 个脏文件**（`manifest.jsonl` 首行有记录）。已复核并接受：脏文件里只有 `src/generate.ts`（改动集中在 `patchTableCellTexts`）与 `tests/nested-table-edit.test.ts` 属于 `docx-engine`，`parseDocx` 未被改动，所以 573 份 `.expected.json` 等价于干净基线；`nested-table-edit` 的两份保存用例走 `kind:'xml'` 原样拼接，对 `generate.ts` 不敏感。genoffice 侧再改 `docx-engine` 时需要重导。
- `TEST-04`「对每个语料做一次单节点编辑」目前分两处覆盖：全语料版是 M0 留下的 L1 `set_text`（`tests/save.rs`，400+ 份，断言到"能重开且改动生效"），L4 `InsertText` 版只跑一份文档（`tests/edit.rs`，但断言到其他条目 CRC 与其他块原字节）。规范里"重解析后其他段落模型相等"这条 oracle 两处都没断言。补一个全语料的 `corpus_edit_fidelity` 才算无争议。
- toggle 属性（bold / italic 等的层叠语义）用的是占位规则，需要 Word 实测 fixture 校准（M5）。
- 语料在 Span / 字段这两个域上很薄：573 份里只有 15 份带范围标记（31 个标记、19 个范围）。M2 的行为正确性主要靠 `tests/span.rs` 的单元用例，不能只看差分数字。
- `compat_ts` 是负担性代码，删除期限定在 M9。
