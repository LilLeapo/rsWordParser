# SPEC 16 · M5 任务分解

对应 `docs/03` 第 12 节 M5 行。格式同 `spec/12` / `spec/13` / `spec/14` / `spec/15`：每个任务给出产出、依赖的
规范条目与完成定义（DoD）。顺序即建议的实现顺序；同一编号内的子任务可并行。排期依据（实测差距）见下文；
现状数字见 `docs/05-status.md`。

M5 是第一个**不再并行**的里程碑：M0–M4 已全部并入 `main`（9181eae = M3 与 M4 的合并提交，2026-09-05），
分支 `m5-hf` 从它开出，工作树 `../rsWordParser-m4`（M4 的工作树复用）。页眉页脚里的表格直接用 M3 的
`TableBlock`，页眉里的图片直接用 M4 的显示模型，不需要任何临时读法。

## M5 · 页眉页脚、节、声明 part：跨 part 内容流、`SectionProps`、保存选项全集、resolve 校准

目标：页眉页脚 part **复用正文管线**成为独立内容流（`docs/03` §6.7，`MOD-01` 的 `hf_parts`），节有完整模型
（`SectionInfo` + `RES-10` 继承），`w:sectPr` 进属性表并可按 `PROP-06` 合并写回，参考文献进模型；
`compat_ts` 输出 TS 的整个页眉页脚域（`COMPAT-05`）；TS `SaveOptions` 里剩下的 M5 选项（节 / 页眉页脚
六变体 / 水印 / 页面颜色 / 页码 / 保护 / 奇偶页眉 / 编号追加 / 主题 / 样式 upsert / 参考文献）全部翻成
`EditOp`（`SAVE-07`，没有旁路）；`resolve/` 的 toggle 规则与节继承用 Word 实测 fixture 校准（`RES-04` /
`RES-12` / `TEST-08`）。

**M5 门**（`spec/11` TEST-10 「M3–M6 对应域 diff 为 0」的具体化）：

1. `cargo run -p diff-parse -- --scope hf` 0 未知差异。按**路径**筛（同 `--scope drawing`，
   `compat_ts::is_hf_path`）：顶层 `headerText / footerText / headerParas / footerParas / headerImages /
   footerImages / watermarkText / headerHasPageNumber / footerHasPageNumber / headerFirst / footerFirst /
   headerEven / footerEven / hfParts.*  / titlePg / evenAndOddHeaders / sources[*]`。`--scope text / fields /
   drawing` 继续为 0。
2. **保存差分**：`tests/save_blocks.rs` 里被 M5 选项阻塞的 **48 份**用例全部转为「等价」或登记进 `INTENTIONAL`；
   跳过清单只剩 chart 6 + image 4 + inks 8 + `partXml` 1 + `replaceImage` 1 = 20 份（M6 / M7）。
3. **页眉页脚编辑往返**（`docs/03` §12 M5 验收 + `TEST-04` 扩到辅助 part）：对 43 份带页眉页脚的语料，在一个
   页眉段落里 `InsertText` 后保存——`document.xml` 与其他 zip 条目 CRC 与压缩字节**不变**，只有那个
   `header*.xml` 重写且其中其他块原字节原样；重解析后只有该 part 的 `paras` 变。
4. **resolve 校准**：`fixtures/resolve/toggle/*` 至少 5 个、`fixtures/resolve/sections/*` 至少 1 个通过
   `TEST-08`（`RES-04` 的占位规则被真实规则替换）。**依赖真实 Word**，见 5.8 与「依赖与被阻塞」。
5. `corpus/hostile` 新增的 4 份页眉页脚 / 节病态输入解析成功、局部降级、无编辑保存字节相同（`TEST-09`）。

### 实测差距（2026-09-05，`main` = 9181eae，`cargo run -p diff-parse -- --scope all --json`）

全域 244 处未知差异 / 81 份文档（另 187 处已登记）。页眉页脚域直接命中 **161 处 / 43 份**（语料里带 `header*.xml` /
`footer*.xml` 的文档恰好 43 份，一份不落）：

| 路径 | 差异点 / 文档 | 归属任务 |
| --- | --- | --- |
| `hfParts.<rId>` | 46 / 42（另 `hfParts.rIdHdr` 1 / 1：自定义 rId） | 5.4 |
| `headerText` / `headerParas` | 34 / 34 各 | 5.4 |
| `headerImages` | 15 / 15 | 5.4（读 M4 的 `ImageDisplay` / `VmlDisplay`） |
| `footerText` / `footerParas` | 11 / 11 各 | 5.4 |
| `footerHasPageNumber` / `headerHasPageNumber` | 6 / 1 | 5.3（`FLD-11` 由字段索引推导）+ 5.4 |
| `headerEven` | 2 / 2 | 5.4（`hf-variants__*`） |
| `sources[]` | 1 / 1（`watermark-theme-sources__008`） | 5.7 |
| `extra__mixed-flavor` 的 `internal.*` / `extras.elements[*]` | 7 / 1 | TS 装载时把 Strict 主 part 改写为 Transitional，与已登记的 `extra__strict-minimal` 同因；5.4 时按**路径**登记（该文档的页眉路径必须归零，不能整份放行） |

（`docs/05` 里 M3 收尾时写的「页眉页脚 157」漏算了 `headerEven` 2、`headerHasPageNumber` 1、`hfParts.rIdHdr` 1，
同一批数据，实为 161。）不属于 M5 的剩余：单元格里的锚定形状 15（M3 × M4 交叉地带，见「不在 M5」）、
公式 4 与图表 2（M6）、块分类连带项与 run 约 30（多半是页眉页脚段落的连带，随 5.4 归零）。

今天这些字段全是 `compat_ts/mod.rs` 里的占位（`null` / `false` / `{}`），所以 161 处是 43 份 part 的**入口**，
不是工作量：把 `hfParts` 发出去之后差异会按 `HfPartInfo` 的子字段展开。工作面按语料实测：

| 量 | 值 | 备注 |
| --- | --- | --- |
| 带页眉页脚 part 的文档 | 43 份 | 前缀：`hf-images` 17、`header-footer-rich` 12、`hf-variants` 5、`header-footer-textbox` 3、`sections` 2、`extra__mixed-flavor` / `layout-fidelity` / `resource-cleanup` / `write-protection` 各 1 |
| 其中 part 里带图片 / 带表格 / 带文本框 | 15 / 9 / 3 份 | 图片走 M4 的 `MediaStore`（按 part 自己的 rels）；表格是 M3 的 `TableBlock`，TS 把它折成 `HfParagraph.cells`；文本框段落被「提出」（`textboxParagraphs`） |
| `first` / `even` 变体、`titlePg`、`evenAndOddHeaders` | 2 / 2 / 1 份 | 变体很薄，行为正确性主要靠 `tests/hf.rs` 的构造用例 |
| `w:sectPr` 总数 / 多节文档 | 588 / 14 份（13 份两节、1 份三节） | 4.6c 的 `SectionGeom` 已经扫过它们，5.2 换成 `SectionInfo` |
| 水印（`v:textpath`）解析侧正例 | **0 份** | 只有保存侧 7 份要**生成**水印；`watermarkText` 的读取必须用构造文档测 |
| 参考文献 `b:Sources` | 1 份 | 保存侧 3 份 |
| 脚注 / 尾注 part | 4 / 1 份 | 读写已在 M2（2.6）完成；M5 只补 `Note.blocks`（见 5.3） |

保存侧（`tests/save_blocks.rs` 实测，按选项分组，去重后 48 份）：

| 组 | 份 | 选项键 |
| --- | --- | --- |
| 页眉页脚六变体与每节页眉 | 22 | `header` 15、`footer` 4、`headerFirst` 3、`headerEven` 2、`footerEven` 1、`titlePg` 3、`evenAndOddHeaders` 2、`sectionHf` 3、`hfAllSections` 1 |
| 水印 | 7（2 份与 `header` 同用例） | `watermark`（字符串 / `null`） |
| 节 | 7 | `section` 4、`sectionStartType` 2、`pgNumType` 1 |
| 保护 | 4 | `protection` 3、`writeProtection` 2 |
| 编号追加 / 主题 / 参考文献 / 样式 upsert | 3 / 2 / 3 / 1 | `numbering`、`themeFonts` / `themeColors`、`sources`、`styleUpserts` |
| 页面颜色 | 1 | `pageColor` |

`*.save.<k>.json` 只记录了输出的 `document.xml`：页眉页脚 / settings / numbering / theme / customXml 这些
part 的 TS 输出**没有留档**，所以 5.6 / 5.7 的等价只能证明「主 part 一致」，其余 part 要靠 XPath 断言
（`TEST-05`）与**重解析后的投影**做 oracle（例如保存 `themeFonts` 后我们自己的 `parsed_doc` 必须报出新字体）。

### 任务

| # | 任务 | 规范 | DoD |
| --- | --- | --- | --- |
| 5.1 | **节属性表**：`schema/props/section.toml`——`SectionProps`（`change = "w:sectPrChange"`），`order` 为 `PROP-05` 的 CT_SectPr 全序；字段按 `PROP-08`：`header_references` / `footer_references`（`multi`，struct `HfReference { type: HdrFtrType, id: r:id }`）、`footnote_pr` / `endnote_pr`（子表：`pos / numFmt / numStart / numRestart`）、`type`（`SectType`）、`pg_sz`（struct `w / h / orient / code`）、`pg_mar`（struct 7 个属性）、`pg_borders`（子表：表级 `attrs` `zOrder / display / offsetFrom` + 4 边 `Border`）、`ln_num_type`、`pg_num_type`（struct `fmt / start / chapStyle / chapSep`）、`cols`（子表：表级 `attrs` `num / space / equalWidth / sep` + `col` `multi` struct `{w, space}`）、`form_prot`、`v_align`（`VerticalJc`）、`no_endnote`、`title_pg`、`text_direction`（`TextDirection`）、`bidi`、`rtl_gutter`、`doc_grid`（struct `type / linePitch / charSpace`）；`paperSrc` / `printerSettings` `Raw`。`types.toml` 增 `SectType`、`HdrFtrType`、`PageOrient`、`ChapterSep`、`LineNumberRestart`、`PgBorderZOrder / Display / Offset`、`DocGridType`；`VerticalJc` / `TextDirection` / `Border` / `NumberFormat` 复用已有的。`para.toml` 的 `sect_pr` 从 `Raw` 改为这张表。`SectionProps` 会和 `TableProps` 一样有两三 KB，读取一律走 M3 的 `boxed_reader!`（`#[inline(never)]` + `Box`，`model/table.rs` 的栈教训）。`model/section.rs` 的 `SectionGeom` 改由 `SectionProps` 读出（字段语义一致，`box_json` 不动） | PROP-01/02/04/05/07/08/09 | 每行 `PROP-07` 往返（两种 flavor）；语料 588 个 `w:sectPr` read → emit → read 建模字段全等，`PROP_BAD_VALUE` 逐条列出并解释；`PROP-05` 顺序单调率写进 docs/05；`tests/props.rs` 增这张表；`--scope drawing` 仍为 0（锚定定位没被换表改坏） |
| 5.2 | **节模型与 resolve 节视图**（`model/section.rs` 重写）：`SectionInfo { node, props: SectionProps, owner: Body \| Paragraph(NodeId), block_range: Range<usize> /* main 的下标，含分节段落 */, start_type /* 缺省 nextPage */, hf_refs: [[Option<RelId>; 3]; 2] /* kind × variant，声明值 */, revisions }`；`Document.sections: Vec<SectionInfo>`（没有任何 `sectPr` 时给一个缺省节，US Letter / 1 in，与 TS `DEFAULT_SECTION` 一致）；`section_of(node) -> SectionIdx`（`RES-10`：第一个在其之后的 `sectPr`）；`Revision::SectPropsChange { old: NodeId }` 附到 `SectionInfo.revisions`（`MOD-09`）。`Resolver::section(idx) -> EffectiveSection { hf: [[Effective<Option<PartId>>; 3]; 2] /* Declared \| Inherited(from) \| Absent */, title_pg, even_and_odd /* 文档属性 */, geom }` 与 `hf_for_page(idx, first: bool, even: bool) -> (kind → Option<PartId>)` 实现 `RES-10` 的继承与有效变体选择 | MOD-10, MOD-09, RES-10, RES-12 | `RES-10` 验收行（第二节无 header 引用时继承第一节）构造用例；14 份多节语料的节数与 `block_range` 与 TS `readSections` 一致（测试里按 TS 规则从 `expected.json` 的 `originalXml` 含 `<w:sectPr` 的块重算）；`titlePg` 的 compat 输出不变；`fixtures/resolve/sections/01` 的 `[[section]]` 断言通过（5.8 一起）；`tests/section.rs` 新建 |
| 5.3 | **页眉页脚 part 与跨 part 内容流**（`model/hf.rs`）：`HfPart { part: PartId, kind: Header \| Footer, root: NodeId /* w:hdr / w:ftr */, blocks: Vec<Block>, flows: FlowMap, fields: FieldIndex, spans: SpanIndex, has_page_number, has_num_pages /* 由本 part 字段索引的 Keyword::Page / NumPages 推导，FLD-11 */, watermark: Option<String> /* header 的 v:textpath/@string，实体解码 */, revisions }`；`Document.hf_parts: BTreeMap<PartId, HfPart>` + `Document.hf_by_rel: Map<RelId, PartId>`（主 part rels 里 type 以 `/header` / `/footer` 结尾的全部关系，含没被任何 `sectPr` 引用的孤儿 part——TS `parseAllHfParts` 也输出它们）；内容用**同一个** `build_container`（段落 / 表格占位 / sdt / 修订包裹 / 文本框），`Block` 的 `NodeId` 相对该 part 自己的 DOM；part 解析失败（`Opaque`）→ `HfPart.blocks` 空 + `PKG_OPAQUE_PART` 诊断（`TEST-09` `xml-unbalanced-header` 已覆盖）。**同一机制的第二个客户**：外部文本框 part（`wps:txbx/@r:txbx` → `word/txbx1.xml`）——`ShapeDisplay.content` 改为 `Option<FlowContent { part: Option<PartId>, blocks }>`，`Document.aux_flows` 收纳；**第三个客户**：`Note` / `Comment` 增加 `blocks: Vec<Block>`（`MOD-01` 形态；现有 `text` / `rich` / `paragraphs` 保留，compat 不变） | MOD-01, MOD-02, SPAN-01, FLD-02, FLD-11, PKG-05 | 43 份语料每个 hdr / ftr part 都建出 `blocks`，穿透 sdt 的段落 + 表格数与 TS `paras`（去掉尾部空段、表格按行）对得上（精确对齐在 5.4 验收）；`has_page_number` 与 TS `hasPageNumber` 逐 part 一致（7 处）；每个 part 自己的 `FlowMap` / `FieldIndex` / `SpanIndex`（页眉里的书签 / 字段成对认领）；`KNOWN_DIFFS` 删掉 `themeless-shapes-external-txbx__003`；遍历**迭代**（hostile `hf-deep-txbx` 3000 层不爆栈）；`tests/hf.rs` 新建 |
| 5.4 | **compat 页眉页脚投影**（`bind/compat_ts/hf.rs`，规则见 `COMPAT-05` 与 `docs/01` §9）：`hfParts{rId}` 每个关系一条 `HfPartInfo { text, hasPageNumber, paras, images? }`；`headerText / footerText / headerParas / footerParas / headerImages / footerImages / watermarkText / *HasPageNumber` 取 **default 变体**，选法照 TS `readHeaderFooterPart`：全文第一个 `w:type="default"` 的引用 → 否则 `w:type="odd"` → 否则无 `w:type` 的（不是按节）；`headerFirst / footerFirst / headerEven / footerEven` 只认 typed。`text`：PAGE 原子 → ``、NUMPAGES → ``、其他字段 → 缓存结果 run 的文字，`mc:Fallback` 已剥。`paras` = `hfParagraphs`：每段 `HfParagraph`（`ParaFormat` + `runs`（复用段落投影）+ `ptabAligns` + `frameXAlign` + 样式的 `align`（非 justify）/ `tabStops`）；表格（`TableBlock`）→ 每行一条 `{ runs: [], cells: [{ paras, align, widthPct /* tcW 非 pct，缺省按 tblGrid 与 gridSpan 求和 */, fill }] }`，`widthPct` / `fill` 的算法与 `compat_ts/table.rs` 的格投影共用，整行无字无图无 fill 跳过，`w:tblpPr` 浮动表延后到下一段之后；段落 `runs` 为空但有 `w:r` / `w:pict` → 提出全部 `w:txbxContent` 段落；尾部空段（无 cells）去掉。`images` = `hfImages`：`w:drawing`（非锚定且在顶层 `w:tbl` 区间内的跳过；第一个能解析的 blip；`wp:extent` px；锚定 → `floating / behind / wrap / posH / posV / posXPx / posYPx / posHRel / posVRel`；内联 → 段落 `w:jc` → `align`；`a:srcRect` → `crop`）与 `w:pict`（跳过 `v:textpath`；`v:imagedata`；`style` 尺寸；`position:absolute` → floating；`z-index` 负 → behind；`mso-position-*` → posH / posV；`gain \| blacklevel` → `washout`）——全部读 M4 的 `ImageDisplay` / `VmlDisplay` / `AnchorGeom`，px 换算只在这里做。`diff-parse --scope hf`（`is_hf_path`）接 CI | COMPAT-02, COMPAT-05, COMPAT-07, TEST-03, TEST-10 | 页眉页脚域 161 → 0；`extra__mixed-flavor` 的 `internal.*` / `extras.elements[*]` 按路径登记进 `KNOWN_DIFFS`（原因同 `extra__strict-minimal`）；`--scope hf` 0 未知差异并在 `.github/workflows/ci.yml` 里多一步；`--scope all` 的文档数 / 差异点数更新到 docs/05 |
| 5.5 | **页眉页脚与节的编辑操作**：**位置带 part**——`InlinePos` / `BlockPos` 增加 `part: Option<PartId>`（`None` = 主 part，现有构造函数零改动），`EditSession::apply` 按 part 分派，`MutationPlan` 本来就是按 part 的（1.14），`MutationResult` 带 `part`，`Document::refresh` 对辅助 part 整 part 重建（part 很小，接受）；既有段落 / 块 / 范围 / 字段操作在页眉段落上原样可用。新操作：`SetSectionProps { sect: NodeId, patch: SectionPropsPatch }` 走 `plan_apply_section_props`，与 M3 的三个表格属性操作同形，由 `table_props_op!` 泛化成 `props_container_op!` 一起生成（新建容器的位置：body 级 → `w:body` 最后一个子元素；段落级 → `pPr` 内按 `para.toml` 的 `order` 在 `rPr` 之后、`pPrChange` 之前——`PROP-06` 第 1 步的 `sectPr` 规则，`docs/04` §8 挂账）；`SetHeaderFooter { sect, kind, variant, content: Vec<NewBlock> }`（该节**声明**了此变体 → 目标 part 内容整体替换（内容子节点全 `Deleted` + `New` 块）；没声明（含从上节继承）→ 按 `SAVE-05` 新建 `word/header{N}.xml`（第一个空闲 N）、关系、`[Content_Types]` Override、`w:headerReference w:type r:id` 插为 `sectPr` 第一个子元素（`PROP-05`），本节因此独立、前面的节不受影响——与 Word / TS `sectionHf` 语义一致）；`LinkHeaderFooter { sect, kind, variant, part }`（给没有引用的节挂一个**已有** part 的引用；`hfAllSections` 用）；`SetWatermark { sect, text: Option<String> }`（default header 的第一个段落放 TS `watermarkParagraphXml` 那棵 VML 子树（`v:shapetype` 136 + `v:shape` `PowerPlusWaterMarkObject1` / `o:spid _x0000_s2049`，`v / o / w10` 前缀按 `XML-14` 声明），`None` → 删掉所有含 `v:textpath` 的段落；part 不存在时先建；**Strict 包 → `Err(EDIT_UNSUPPORTED_STRICT_VML)`**，`docs/03` §14）；`SetPageColor { color: Option<[u8; 3]> }`（`w:background w:color` 为 `w:document` 第一个子元素；`None` 删除）；`SetDocumentSettings { patch: SettingsPatch }`（`evenAndOddHeaders` / `documentProtection` / `writeProtection`，走 `plan_apply_settings`，缺 `settings.xml` 按 `SAVE-05` 建；`settings.toml` 缺的字段补上）。`track_changes` 下的 `sectPrChange` 快照 → M7 | EDIT-01, EDIT-02, EDIT-03, EDIT-05, EDIT-06, PROP-05, PROP-06, SAVE-05, XML-14 | `EDIT-03` 验收行：无页眉的文档 `SetHeaderFooter` → `header1.xml` + `.rels` + `[Content_Types]` 正确、`headerReference` 是 `sectPr` 第一个子元素、其他条目原压缩数据不变（`SAVE-05` 的页眉版）；已有 part → 只重写该 part，`document.xml` 逐字节相同；`SetSectionProps` 新加 `w:pgNumType` 落在 `w:lnNumType` 之后 `w:cols` 之前、未碰的子元素原字节不动、`sectPr` 开标签不变（`PROP-05/06`）；Strict 文档水印 → `Err` 且 DOM / Span / Model 与操作前一致（`EDIT-05`）；M5 门第 3 条（43 份页眉段落 `InsertText` 往返）；每个操作一组 XPath 断言（`TEST-05`）；`tests/hf_ops.rs` 新建 |
| 5.6 | **保存选项：节 / 页眉页脚 / 水印 / 页面颜色 / 保护 / 奇偶页眉**（`save/options.rs` 拆成目录 `save/options/{mod,hf,section,settings}.rs`）：`SaveOptions` 增 `section / section_start_type / pg_num_type / page_color / header / footer / header_first / footer_first / header_even / footer_even / title_pg / section_hf / even_and_odd_headers / hf_all_sections / watermark / protection / write_protection`，每项翻成 5.5 的操作（`SAVE-07`：没有旁路）。`compat_ts::apply_save_blocks` 把 TS JSON 翻过来：`HeaderFooter { text, pageNumber, paras }` → `Vec<NewBlock>` 按 TS `headerFooterPartXml` 规则（`` → PAGE 复杂字段五 run 结果 `1`、`` → NUMPAGES；`pageNumber` 且无标记时第一个 `#` 顶替；无 `paras` → 单段 `w:jc center`，`text` 后补一个空格再接页码字段；`paras` → 每条一个 `w:p`，`pPr` 用 M1 的 `formatPPrChildren`（含 `bidi`，别把视觉对齐写回逻辑值），带 `cells` 的条目**跳过**（原 `w:tbl` 字节保留）；**外科合并**：原 part 里的非段落子节点与含 `w:drawing / w:pict / w:object`（非 textpath）的段落原字节保留，只有文本段落整体替换在第一个文本段落的位置——用 `DeleteBlock` / `InsertBlock`（带 part 的 `BlockPos`）组合出来，不另造操作）；`watermark` 单独出现 → 只动水印段落（`SetWatermark`），与 `header` 同出现 → 先内容后水印；`sectionHf[].lastBlockIndex` → 该块的 `sectPr` → `SetHeaderFooter`；`hfAllSections` → 每个没有引用的 body `sectPr` 各一条 `LinkHeaderFooter`；`section`（TS `SectionSettings`）→ `SectionPropsPatch`（`pageBorder: true` → 四边 `single sz=4 space=24 auto`、`offsetFrom=page`；`cols` 的重建条件照 TS；`headerDist / footerDist` 只在给出时写；新建 `pgMar` 缺省 `header/footer 708`、`gutter 0`）；`sectionStartType`（`nextPage` = 删 `w:type`）；`pgNumType`（两者都缺 = 删）；`titlePg`；`protection` / `writeProtection`（`null` 删；crypt 属性只在有 `hash` 时写，缺省 `sid 14` / `spinCount 100000`；`enforced` → `w:enforcement="1"`）；`evenAndOddHeaders`；`pageColor`（`null` 删）。新元素位置一律按 `PROP-05`，TS 正则式的落点与 schema 不一致处（如 `titlePg` 被塞在 `bidi` 之后）**不跟随**，登记 `INTENTIONAL` | SAVE-01, SAVE-05, SAVE-07, EDIT-04, COMPAT-08 | 48 份用例里除 5.7 的 9 份以外的 **39 份**全部等价（`documentXml` 规范化相同）或登记；`outputIdenticalToSource` 的短路不受影响（`is_empty` / `forces_save` 语义补齐）；`tests/save_options.rs` 每个选项至少一个用例，对生成的页眉 / settings part 做 XPath 断言并重解析比对 `parsed_doc` 的 `headerText / watermarkText / titlePg / evenAndOddHeaders / protection`；`SAVE-07` 验收行 |
| 5.7 | **声明 part 的读写：参考文献、编号追加、主题、样式 upsert**：读侧 `model/sources.rs`——在 `customXml/item*.xml` 里找根为 `b:Sources`（bibliography 命名空间）的 part，`Source { tag, kind, author /* Corporate 或 "Last, First" */, title, year, publisher /* Publisher \| JournalName \| InternetSiteTitle */, url, raw }`（`MOD-10`），`Document.sources`，compat `sources[]`；`local_names.txt` 补 `Tag / Title / Author / Person / Middle / Corporate / Publisher / InternetSiteTitle / URL / City / Guid / LCID / RefOrder`。写侧四个选项各翻成对应 part 的 `MutationPlan`：`sources`（权威列表：字段没变的条目原字节不动、变了的换 `New`、列表外的 `Deleted`；part 缺失 → 新建 `customXml/item{N}.xml` + `itemProps{N}.xml`（`ds:datastoreItem` + bibliography `ds:schemaRef`）+ `customXml/_rels/item{N}.xml.rels` + 主 part 的 `customXml` 关系 + `[Content_Types]`，`SAVE-05` 扩到 customXml）；`numbering`（`newDefs` → `abstractNum` + `num` 追加在末尾，无 `levels` 时用 TS `blank.ts` 模板的 9 级常量（复制并注明出处），有 `levels` 按 `CustomNumberingLevel` 生成；`restartNums` → 指向已有 `abstractNum` 的 `num` + `lvlOverride/startOverride`；part 缺失 → 从模板新建 + 关系 + Override；**只追加**，既有条目字节不动）；`themeFonts` / `themeColors`（`a:majorFont / a:minorFont` 的 `latin / ea / cs` `typeface`、`a:clrScheme` 各槽的 `a:srgbClr/@val`（`a:sysClr` 换成 `srgbClr`）；`theme1.xml` 缺失 → 模板新建）；`styleUpserts`（`w:style[@w:styleId]` 存在 → 整条替换，否则追加；`rPr / pPr` 用属性表的 `emit_run_props / emit_para_props` 生成，**不手写 XML**） | MOD-10, SAVE-05, SAVE-07, COMPAT-02, PROP-07 | `sources[]` 1 处归零；9 份用例（sources 3 + numbering 3 + theme 2 + styleUpserts 1）主 part 等价；每个选项一个单测：XPath 断言生成的 part + **重解析**后 `parsed_doc` 的 `sources / numbering[numId] / themeFonts / themeColors / styles[styleId]` 等于请求值（这是这些 part 唯一的 oracle，见上文说明）；首次新建每种 part 后其他条目原压缩数据不变 |
| 5.8 | **resolve 校准：toggle 与节继承 fixture**（`RES-04` / `RES-10` / `RES-12` / `TEST-08`）：`tests/resolve_fixtures.rs` 读 `fixtures/resolve/**/expected.toml`（`[[run]] para / run / bold / italic / … / source`、`[[section]] idx / header_default / inherited_from`），每个 fixture 目录一个 `#[test]`（`fixture_tests!` 宏展开）；`resolve_toggle(prop, direct, char_chain, para_chain, table, doc_default) -> Effective<bool>` 独立函数替换 `resolve/mod.rs` 的 `RES-04 placeholder`，规则参数化，按 fixture 校准。**fixture 的 docx 由我们生成**（`tools/fixtures/gen_toggle.rs` 或测试里的 `docx_with_parts` 构造最小文档：① 段落样式 b + 字符样式 b；② docDefaults b + 段落样式 b；③ basedOn 两层都 b；④ 表格样式 firstRow b + 段落样式 b（走 M3 的 `RES-08` 表格视图）；⑤ 直接 `w:b w:val="0"` 覆盖样式；⑥ 两节文档第二节无 header 引用），**观察值必须来自真实 Word**：由项目负责人在 Word 里打开、记录每个 run 是否加粗 / 第二节页眉显示什么，填进 `expected.toml` 与 `README.md`（Word 版本、观察方法） | RES-04, RES-10, RES-12, TEST-08 | 5 个 toggle fixture + 1 个 sections fixture 通过；`RES-04` 规范条目按 Word 实测改写并注明与 ECMA-376 §17.7.3 / [MS-OI29500] 的差异；`resolve/mod.rs` 与 `resolve/tests.rs` 里的 `RES-04 placeholder` 字样删除；`fixtures/resolve/README.md` 更新；`tests/resolve.rs` 的 86,465 项 `StyleDisplay` 对照仍全等（TS 的 `display` 是 child-overrides-parent，改规则后若不等，差异按路径登记并说明 Word 为准） |
| 5.9 | **恶意输入、随机序列与 M5 门**：`corpus/hostile` 补 `hf-dangling-reference.docx`（`headerReference r:id` 在 rels 里不存在）、`hf-part-binary.docx`（header part 是二进制垃圾）、`sectpr-bad-values.docx`（`pgSz w="abc" h="-1"`、`cols num="0"`、`pgNumType start="x"`、`type val="weird"`、`titlePg w:val="maybe"`）、`hf-deep-txbx.docx`（页眉里 3000 层 `w:txbxContent` 套娃），生成器进 `tools/export-golden/hostile.export.test.ts`；`tests/hf_ops.rs` 加小型随机序列（页眉段落 `InsertText / DeleteRange / SetRunProps`、`SetSectionProps`、`SetHeaderFooter`、`SetWatermark`，100 步 × 10 份带页眉的语料，每步 `refresh == rebuild`、`SAVE-02` 无 `EngineInvariantViolation`，每 20 步保存 + 重解析）；CI 加 `diff-parse --scope hf` | TEST-07, TEST-09, TEST-10 | 四份 hostile：解析成功、诊断分别为 `PKG_REL_MISSING` / `PKG_OPAQUE_PART` / `PROP_BAD_VALUE`×n / `MOD_TOO_DEEP`，页面几何回退缺省值，`hfParts` 不含悬空引用，对 `Opaque` part 的 `SetHeaderFooter` → `Err(EDIT_TARGET_OPAQUE)` 且状态不变，无编辑保存字节相同；100 × 10 无失败，失败用例最小化后固化；`.github/workflows/ci.yml` 多一步且绿；docs/05 全部数字更新 |

**顺序说明**：5.1 → 5.2 → 5.3 → 5.4 是读侧主链（投影要 part 模型，part 模型要节引用，节要属性表）。5.5 要 5.1 + 5.3
（位置带 part 是 5.5 的第一个提交，纯机械改动，先落地让 M3 的合并面尽早稳定）；5.6 要 5.5；5.7 独立，可随时插；
5.8 的 **fixture 文档生成**应在第一周就做，把观察工作尽早交给项目负责人，规则替换等观察值回来再做；5.9 收尾。
5.4 是门的第 1 条，建议 5.3 一落地就先把 `hfParts{rId}.text / hasPageNumber / paras[].runs` 这批「不需要图片与表格」
的字段发出去，让差分数字尽早开始往下走。

## 分层决策（实现前定死）

1. **`NodeId` 仍相对单个 DOM；`PartId` 进位置**。`docs/03` §8.1 的 `InlinePos { para, offset }` 与 `BlockPos` 各加
   `part: Option<PartId>`（`None` = 主 part），`MutationResult` 带 `part`。不做「全包统一 arena」：那会让 M0–M4 的
   全部 `NodeId` 语义变化。这是相对 `docs/03` 的偏差，登记 `docs/04` §8。
2. **页眉页脚内容是 `Vec<Block>`，每个 part 一套索引**（`FlowMap` / `FieldIndex` / `SpanIndex`），与主 part 结构相同；
   `HfPart` 只是「主 part 那一套」的小号复制。`has_page_number` 由 `FieldIndex` 的 `Keyword::Page` 推导（`docs/03`
   §5.4 末段），`` / `` 这两个占位符只存在于 `compat_ts`。
3. **TS 的页眉页脚启发式全部留在适配器**：default 变体「全文第一个匹配」的选法、`hfParagraphs` 的表格折行 /
   文本框提出 / 尾空段删除 / `ptabAligns` / `frameXAlign`、`hfImages` 的 px 与位置、水印文字。模型按节记
   `hf_refs` 声明值，`resolve::section` 给继承后的有效值。
4. **保存选项 = 编辑操作的组合，没有旁路**（`SAVE-07`）。TS 用正则改 `sectPr` / settings / 页眉 part；我们的等价物是
   `SectionPropsPatch` / `SettingsPatch` / 带 part 的块操作。新元素位置按 `PROP-05`；`document.xml` 里 TS 落点与
   schema 不一致的地方（`titlePg` 在 `bidi` 之后、`pgNumType` 在 `lnNumType` 之前）**不跟随**，登记 `INTENTIONAL`。
   settings / 页眉 part 不在 `*.save.json` 的比较范围内，位置差异不会体现在差分里，但仍按 schema 写。
5. **页眉页脚里的表格就是 `TableBlock`**（M3 已并入）：同一个 `build_container` 在页眉 part 里建出来的表格与正文
   完全同型，`HfParagraph.cells` 只是 TS 的折行投影，放在 `compat_ts/hf.rs`，行 / 格 / 列宽 / 底纹的取法与
   `compat_ts/table.rs` 共用一套辅助函数。`--scope hf` 因此**不**剔除带表格页眉的文档。
6. **水印在 Strict 包里拒绝**（`Err(EDIT_UNSUPPORTED_STRICT_VML)` + 诊断），DrawingML 水印不在 M5（`docs/03` §14）。
   语料里 Strict 文档只有 `extra__strict-minimal`，没有水印保存用例。
7. **注释 / 批注条目的 `blocks`** 与页眉页脚同一构建器（`MOD-01`），但 M2 的 `text / rich / paragraphs` 与
   compat 的 `richParas` 不动——那是 TS 形态的投影，M9 随 `compat_ts` 一起删。

## 从 M0–M4 带过来的债（M5 内解决）

| 债 | 位置 | 解决任务 |
| --- | --- | --- |
| `SectionGeom` 只是锚定定位要的几个数（4.6c 注释：「M5 建 `SectionInfo` 时替换」） | `model/section.rs` | 5.1 / 5.2 |
| `RES-04 placeholder`：toggle 按「最具体声明胜出」 | `resolve/mod.rs`、`resolve/tests.rs` | 5.8 |
| `KNOWN_DIFFS` 放行 `themeless-shapes-external-txbx__003` 的外部文本框 part（`spec/15` 被阻塞表） | `bind/compat_ts/KNOWN_DIFFS.md` | 5.3 |
| `compat_ts/mod.rs` 的页眉页脚占位（`null` / `false` / `{}`）与 `sources: []` | `bind/compat_ts/mod.rs` | 5.4 / 5.7 |
| `save/options.rs` 模块头「节 / 页眉页脚 / 页面颜色属 M5」；`apply_save_blocks` 对这些键 `EDIT_UNSUPPORTED` | `save/options.rs`、`bind/compat_ts/save_blocks.rs` | 5.6 / 5.7 |
| `PROP-06` 第 1 步缺 `sectPr` 容器的新建位置规则（`docs/04` §8 `PROP-06` 行） | `semantic/props` / `edit/ops.rs` | 5.5 |
| `docs/05`「页眉页脚里的图片（M5）」「外部文本框 part」两条明确未实现 | `docs/05-status.md` | 5.3 / 5.4 |
| `para.toml` 的 `sect_pr` 是 `Raw` | `schema/props/para.toml` | 5.1 |
| `Settings` 缺 `documentProtection` 的写侧字段（读侧 `protection_json` 已有） | `schema/props/settings.toml` | 5.5 |

`xml/dirty.rs` 的 `rehome_subtree`（规则 E′，M0 只留签名）**M5 仍不需要**：`SetHeaderFooter` 的内容是 `NewBlock`
（`New` 子树），compat 的外科合并也只在同一 part 内删插；跨 part 搬原节点是 M7 `MoveBlock` 的事。

## 不在 M5

- **页眉页脚里的图片编辑 / 插入**、`replaceImage`、墨迹（`inks`）、`partXml` / `partBinary`、图表——M6 / M7。
- **分节操作**：插入 / 删除分节符（新建或删除 `w:sectPr`）不在 `docs/03` §8.2 的 `EditOp` 里；TS 的 `sectionStartType`
  也只改尾部 `sectPr` 的 `w:type`。`SectPropsChange` 的接受 / 拒绝——M7。
- **节的排版语义**：`docGrid` 行网格、`lnNumType` 行号、`pgBorders` 绘制、`vAlign`——parser 只记录，`resolve::section`
  只做引用继承与页面几何。
- **脚注 / 尾注的读写**——M2 已完成；M5 只补 `Note.blocks`。
- **`compatibilityMode` 等 `CompatFacts` 的解释**——已在 M1 记录（`decl.rs::CompatFacts`），解释归布局层。
- **DrawingML 水印**（Strict 包）、图片水印的生成——TS 也没有。
- **单元格里的锚定形状**（`blocks[*].table.rows[][].anchoredBoxes / anchoredBoxAnchors / paras[]`，15 处 / 5 份
  `cell-anchored-boxes__*`）：M3 × M4 的交叉地带，不是页眉页脚域。两边模型都在，把 M4 的框提取接到
  `compat_ts/table.rs` 的格投影上就能归零；可以顺手做，但不进 M5 门。

## 实现约定（本里程碑特别强调）

1. **多用声明宏**（用户要求，2026-09-05，与 `spec/14` 同一条）。页眉页脚域的样板是 M1–M4 里最「矩阵状」的：
   两种 kind × 三种 variant 出现在模型（`hf_refs`）、`Resolver::section`、compat 顶层键（`header / footer × "" / First /
   Even`）、`SaveOptions` 字段（`header_first …`）、`apply_save_blocks` 的键解析五处；settings 布尔开关有四个同形状
   （`removePersonalInfo` / `removeDateAndTime` 已有，加 `evenAndOddHeaders` / `displayBackgroundShape`）；
   `SectionSettings` → patch 有八个同形状的度量字段；toggle fixture 每个目录一个测试。判断标准仍是**同一形状重复
   三次以上就收成 `macro_rules!`**：
   - 投影层新字段一律用 `bind/compat_ts/json.rs` 已有的 `set_some!` / `set_if!`；显示枚举用 `model/macros.rs` 的
     `named_enum!`（`HfKind` / `HfVariant` / `SectType` 的 `as_str` 就是 TS 的字面值）；字段关键字表沿用 M2 的
     `FLD-06` 宏（`Keyword::Page / NumPages`）。
   - 预期新增：`hf_slots!`（kind × variant 六元组一张表，同时展开 `HfRefs` 的访问器、TS 键名 `headerFirst` 一类、
     `SaveOptions` 字段名与 JSON 键的对应）；`settings_flag!`（`Some(true)` 确保元素存在 / `Some(false)` 删除 /
     `None` 不动，三个已有开关一起收进去）；`patch_some!`（`Option<T>` → `Patch::Set`，`SectionSettings` 的八个
     度量共用）；`fixture_tests!`（每个 fixture 目录展开一个 `#[test]`，失败时能看出是哪个 fixture）；
     `xpath_asserts!`（`TEST-05` 风格：一组 `(表达式, 期望)` 对某个 part 逐条断言并在失败信息里带上表达式，
     5.5 / 5.6 / 5.7 的测试都用）。已有的 M3 宏照用：`boxed_reader!`、`table_props_op!`（泛化）、`sdt_enum!`。
     `pgBorders` 四边在 M5 只出现一处，**不**为它上宏（三次以上才收）。
   - 宏带文档注释与 ```ignore 用例；跨模块用 `macro_rules!` + `pub(super) use`，展开里写 `$crate::…` 全路径；
     会把函数定义藏起来、让人跳不到声明处的，用共享模块而不是宏。
2. **树遍历写成迭代**（`hf-deep-txbx` 3000 层）；页眉里的文本框 / 组 / 嵌套 drawing 沿用 M4 的显式栈。
3. **属性容器只走 `plan_apply_*`**：`sectPr` / `settings` / 样式 `rPr / pPr` 都是属性表，手写 `append_child` 会在调试
   构建下被 `PROP-05` 自检拒绝。水印那棵 VML 子树是唯一手写的片段（它不是属性容器），用 `xml::fragment` 解析
   TS 的字面量，不拼字符串。
4. **一个任务一个提交**：`m5.<n>: 英文摘要 (SPEC-ID…)`；提交前同步 `docs/04` §14 勾选、§8 偏差表、`docs/05` 数字。

## 基线与复用（M3 / M4 都已并入）

M5 直接站在 M0–M4 的成果上，没有并行分支要合，但要**复用而不是重造**这几样：

| 来自 | 复用什么 | 在哪个任务 |
| --- | --- | --- |
| M3 `model/table.rs` | `boxed_reader!` / `boxed_change_reader!`（大属性结构体的装箱读取；`SectionProps` 同样大） | 5.1 / 5.2 |
| M3 `edit/ops.rs` | `table_props_op!`（定位容器 → `plan_apply_*_at` → 标记刷新），泛化后给 `SetSectionProps` 用 | 5.5 |
| M3 `compat_ts/table.rs` | 格投影的列宽 / 底纹 / 段落取法，`HfParagraph.cells` 共用 | 5.4 |
| M3 `model/sdt.rs` | `sdt_enum!`；页眉 part 里的 sdt 走同一 `SdtInfo` | 5.3 |
| M3 `resolve/table.rs` | `RES-08` 条件格式，toggle fixture ④ 的表格层 | 5.8 |
| M4 `model/{drawing,vml}.rs`、`package/media.rs` | `ImageDisplay` / `VmlDisplay` / `AnchorGeom` / `MediaStore`（按 part 自己的 rels） | 5.3 / 5.4 |
| M4 `compat_ts/diff.rs` | `is_drawing_path` 的按路径筛法，`is_hf_path` 照抄形状 | 5.4 |
| M4 `model/section.rs` | 4.6c 的 `SectionGeom` 与 `Sections::at`，换成 `SectionInfo` 的派生视图 | 5.2 |
| M2 `span/field/instr.rs` | `keywords!` 表里的 `Page` / `NumPages`，`has_page_number` 由此推导 | 5.3 |

`--scope` 今天五档：`text` / `fields` / `tables` 按文档筛，`drawing` 按路径筛，`all` 全算；`hf` 是第六档，按路径。

## 依赖与被阻塞

| 事项 | 状态 |
| --- | --- |
| **toggle / 节继承 fixture 的观察值**（5.8） | **需要真实 Word**。我们生成 6 份最小 docx；项目负责人在 Word 里打开、记录显示结果与 Word 版本。观察值回来前 5.8 只能做到「文档生成 + 测试骨架 + `expected.toml` 空断言」，M5 门第 4 条挂起 |
| `pageColor` 与 `w:displayBackgroundShape` | Word 是否需要 settings 里的 `w:displayBackgroundShape` 才显示页面颜色，待 Word 实测（TS 不写）。实测需要就一起写并登记为「超过 TS」，否则不写 |
| 页眉页脚 part 的 TS 保存输出未留档 | `*.save.json` 只有 `documentXml`；5.6 / 5.7 的非主 part 用 XPath + 重解析投影验收。若要逐字节对照，得扩 `tools/export-golden/src-index.wrapper.ts` 记录全部改动 part 并重导语料（改期望值的唯一合法途径，`TEST-02`），作为可选项列出，不作为门 |

## 风险提示（实现前确认）

1. **位置带 part 是 API 变化**：`InlinePos::new(para, offset)` 保持主 part 语义，所有现有调用零改动；但
   `EditSession::apply` 内部所有「取主 part DOM」的地方都要改成按位置取——这是 5.5 的第一个提交，纯机械，先做。
2. **default 变体的选法**：TS 是「全文第一个 `headerReference`」，多节文档时来自第一节；Word 显示的是各节自己的
   （继承后）。模型按节、适配器照 TS，两者在多节语料（14 份）上会有不同的「default」——差分只看适配器，
   模型的正确性靠 `tests/section.rs` 的构造用例。
3. **水印没有解析正例**：`readWatermarkText` 的实体解码（`&quot;` 等）与「`v:textpath` 在 `mc:Fallback` 里」的
   情形只能构造文档测；生成侧 7 份用例的头 part 不在比较范围内，重解析 `watermarkText` 是唯一 oracle。
4. **外科合并的边界**：TS 判「受保护段落」用正则 `<w:drawing[\s>]|<w:pict[\s>]|<w:object[\s>]`，含 textpath 的
   段落例外；我们按 `ParagraphFacts.drawings / picts / objects` 判，语义一致但对 `mc:Fallback` 里的 VML
   （TS 正则会命中、语义遍历已剥）可能不同——出现时按路径登记，以「Word 能画出来的都算受保护」为准。
5. **`hfAllSections` 的正则**：TS 给**每个**没有 `headerReference / footerReference` 开头的 `sectPr` 注入全部新引用
   （含段落级），我们用 `LinkHeaderFooter` 逐节做；等价前提是「没有引用」的判定一致（TS 只看第一个子元素）。
6. **numbering / theme / styles 的模板**：TS `blank.ts` 的模板常量要原样复制并注明出处，否则编辑器渲染的列表缩进
   会和今天不同——这些 part 不在差分里，只有重解析投影这一层 oracle。
7. **`settings.toml` 的 `documentProtection`**：读侧今天是手写的 `protection_json`；写侧走属性表要先把字段补进
   表里（struct 8 个属性），并保证 `PROP-05` 顺序（`w:documentProtection` 在 `w:trackRevisions` 一带，不在根后）。
8. **性能**：`Document::rebuild` 现在会解析每个页眉页脚 part；语料最多几 KB，无感。真实文档几十个 part 也在
   毫秒级。`refresh` 对辅助 part 整 part 重建是有意的简化，记进 `docs/04` §8。
