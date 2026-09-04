# SPEC 06 · 事实、分类与文档模型（semantic/、model/）

对应 `docs/03` 第 6 节。职责：从 DOM + Span 投影出编辑器消费的 `Document`。模型是投影：任何时刻 `Document::rebuild(&dom, &spans)` 与增量刷新结果相等（`MOD-13`）。

## MOD-01 Document

```
Document {
  main: Vec<Block>,                       // body 内容流
  sections: Vec<SectionInfo>,
  hf_parts: Map<PartId, HfPart { kind: Header|Footer, blocks: Vec<Block>, has_page_number, has_num_pages }>,
  footnotes: Vec<Note>, endnotes: Vec<Note>,   // Note { id, node, blocks: Vec<Block>, kind: Normal|Separator|ContinuationSeparator|ContinuationNotice }
  comments: Vec<Comment>,                 // { id, node, author, initials, date, blocks, para_id, parent_id, done }
  styles: Styles, numbering: Numbering, theme: Option<Theme>, font_table: Vec<FontEntry>,
  settings: Settings,                     // 含 CompatFacts、保护、themeFontLang、defaultTabStop、evenAndOddHeaders、trackRevisions…
  sources: Vec<Source>,
  media: Vec<MediaEntry>,
  fields: FieldIndex, spans: SpanIndex,   // 只读引用
  warnings: Vec<Diagnostic>,
}
```

页眉页脚、脚注、批注的内容都是 `Vec<Block>`，与正文同一构建器（`docs/03` 6.7）。

## MOD-02 Block

```
Block = Text(TextBlock) | Table(TableBlock) | Image(ImageBlock) | Protected(ProtectedBlock)
TextBlock { node, kind: Paragraph | Heading{level: u8} | ListItem{list: ListRef}, style_id, props: ParaProps, inlines: Vec<Inline>, sdt: Option<SdtInfo>, revisions: Vec<Revision>, facts: ParagraphFacts }
ProtectedBlock { node, kind: ProtectedKind, preview: String, display: Option<Display>, sdt: Option<SdtInfo>, revisions }
ProtectedKind = FieldBlockResult(FieldId) | Equation(FormulaDisplay) | Chart(ChartDisplay) | SmartArt(DiagramDisplay)
              | Ole(OleDisplay) | Rule(RuleDisplay) | Invisible | SectionBreak | SectionProps | BodyBreak{page: bool}
              | Unknown(QName) | TooDeep | Unparseable
```

相对 `docs/03` 6.3 的补充：`BodyBreak`（body 顶层 `w:br`）、`Unknown`（非 `w:p/w:tbl/w:sdt/w:sectPr` 的 body 子节点）、`TooDeep`（`MOD-07`）。`TextBox` 不再是保护种类：文本框是 `Inline::Run` 内的 `Drawing` 段（`MOD-06`），段落可编辑。

## MOD-03 TextKind 判定

- `ListRef`：直接 `w:numPr` 的 `numId/ilvl`；`numId == "0"` → 无编号；无直接 numPr 时用段落样式链上的 `numPr`（样式 `numId 0` 为显式取消）；`ilvl` 缺省用样式的，再缺省 0。
- `Heading{level}`：直接 `w:outlineLvl` 0–8 → level+1（9 → 非标题，**不再看样式**）；否则样式链的 `heading_level`（`RES-02`）；否则 styleId 匹配 `^Heading([1-9])$`（忽略大小写）。
- 优先级：ListRef 存在 → `ListItem`；否则 Heading；否则 `Paragraph`。

## MOD-04 ParagraphFacts

对 `w:p` 的语义子节点一次遍历得到（`docs/03` 6.2），字段说明：

| 字段 | 计算 |
| --- | --- |
| `has_sect_pr` | `pPr/sectPr` 存在 |
| `visible_text` | 任一 `w:t`/`w:delText` 文本 trim 后非空（不含 `w:txbxContent` 内、不含字段指令区） |
| `visible_text_outside_boxes` | 同上但排除所有绘图/VML 内容 |
| `fields` / `inside_field_result` | 来自 `FieldIndex` |
| `drawings: Vec<DrawingFacts>` | 每个顶层 `w:drawing`（语义遍历已剥 Fallback）：`kind ∈ {Picture, Chart, ChartEx, Diagram, LockedCanvas, Shape, Group, Line}`、`anchored`、`has_txbx_text`、`has_blip`、`is_ink`（`wp:docPr/@name` 以 `aidocs-ink` 开头） |
| `picts: Vec<PictFacts>` | 每个 `w:pict`：`ImageData`、`TextBox`、`WordArt`（`v:textpath[@string]`）、`Hr`（`v:rect[@o:hr]`）、`ShapeTypeOnly`、`Hidden`（`visibility:hidden` 或 `stroked=f` 白矩形）、`Other` |
| `objects` | `w:object` 数量 |
| `math` | `oMath` 数量、是否有 `oMathPara` |
| `revision` | 含 `w:ins/w:del/w:moveFrom/w:moveTo`、`delInstrText`、段落标记 ins/del、`pPrChange` |
| `style_id`, `style_vanish` | `pStyle`；样式链 `vanish == true` 且段落无 `w:vanish w:val=0` 且不含绘图/书签/批注/sectPr/numPr |
| `toc_style_level` | styleId 匹配 `^TOC ?([1-9])$`；`TableofFigures` / `TableofAuthorities`（忽略空白与大小写）算 1 级——Word 的图表目录 / 引文目录也是目录行 |
| `numbering_ref`, `outline_level` | 见 MOD-03 |
| `sdt` | 最近的 `w:sdt` 祖先信息（`MOD-08`） |

## MOD-05 分类规则表

按优先级从上到下，首条命中即结束。与 TS 不同之处标 △。

| # | 条件 | 结果 |
| --- | --- | --- |
| R01 | body 子节点为 `w:sectPr` | `Protected(SectionProps)` |
| R02 | `w:tbl` | `Table`（`MOD-07`） |
| R03 | `w:sdt` | 对 `sdtContent` 的每个 `w:p`/`w:tbl` 子节点递归分类，各成一个 Block 并附 `SdtInfo`；无可分类子节点 → `Protected(Invisible)`（△ 不再拆 `openXml/closeXml`） |
| R04 | 范围标记元素 | 不产生 Block（已进入 Span 层）；`compat_ts` 映射为 invisibleMarker 块 |
| R05 | body 顶层 `w:br` | `Protected(BodyBreak{page: type==page})` |
| R06 | 顶层 `w:ins`/`w:del` 包裹 | 递归分类其子块，附 `Revision::Insert/Delete` |
| R07 | 其他非 `w:p` | `Protected(Unknown(qname))` |
| R08 | `w:p`，`style_vanish` | `Protected(Invisible)` |
| R09 | `w:p`，`inside_field_result` 或含 `Block` 策略字段的 begin | `Protected(FieldBlockResult(id))` |
| R10 | `w:p`，`has_sect_pr` 且 `!visible_text` | `Protected(SectionBreak)` |
| R11 | `w:p`，`math.omath_para` 或（`math.count > 0` 且 `!visible_text`） | `Protected(Equation)` |
| R12 | `w:p`，任一 drawing 为 `Chart`/`ChartEx` | `ChartEx` 且有 Fallback 图 → `Image`；否则 `Protected(Chart)` |
| R13 | `w:p`，任一 drawing 为 `Diagram` | `Protected(SmartArt)`（其他兄弟绘图进入 display 的 `siblings`） |
| R14 | `w:p`，任一 drawing 为 `LockedCanvas` | `Protected(SmartArt{canvas})` |
| R15 | `w:p`，`!visible_text` 且内容恰为一个 `Picture` drawing 或一个 `ImageData` pict（非 OLE） | `Image` |
| R16 | `w:p`，`!visible_text`，只含 `Hidden`/`ShapeTypeOnly` pict 或全部 noFill 无文字的 `wps:wsp` | `Protected(Invisible)` |
| R17 | `w:p`，`!visible_text`，只含 `Hr` pict 或 `extent.cy ≤ 130000 EMU` 的无文字 `Shape`/`Line` | `Protected(Rule)` |
| R18 | `w:p`，`!visible_text`，只含 `w:object` | `Protected(Ole)` |
| R19 | 其他 `w:p` | `Text`（△ 含字段、文本框、锚定形状、OLE、修订的段落全部可编辑；这些对象是 inlines 中的段或原子） |

R12–R14 保护的原因是它们需要整 part 级编辑（图表数据、SmartArt 数据），不是保真问题。

## MOD-06 内联模型与坐标流

```
Inline = Run(Run) | Field { id: FieldId, result: Vec<Inline> } | Atom(InlineAtom)
Run { node, segments: SmallVec<[Segment; 2]>, text: String, props: RunProps, link: Option<Link>, field: Option<FieldId> /* 透明字段 */, rev: Option<RevisionCtx>, comments: SmallVec<SpanId> }
Segment { node, kind: SegmentKind, text: Range<u32> /* 在 Run.text 中的字节区间 */, utf16_len: u32, display: Option<Display> }
SegmentKind = Text | DelText | Tab | PTab{align} | Br{kind: TextWrapping|Page|Column, clear} | Cr | NoBreakHyphen | SoftHyphen | Sym{font, code}
            | Drawing{anchored} | Pict | Object | Ruby{rt} | FootnoteRef{id} | EndnoteRef{id} | FootnoteRefMark | EndnoteRefMark
            | Separator | ContinuationSeparator | CommentRef | LastRenderedPageBreak | FldChar | InstrText | DelInstrText | AnnotationRef | Other(QName)
InlineAtom { node, kind: Math | BareBreak{kind} | Other(QName), props: RunProps }   // 段落级非 w:r 子节点
Link = Hyperlink { node: NodeId /* w:hyperlink */, target: Internal{anchor} | External{rel_id, href}, tooltip } | Field(FieldId)
```

- `Run` 与物理 `w:r` 一一对应；`Run.text` 是坐标流中该 run 的文本；`segments` 覆盖全部子节点且区间不重叠、按顺序。
- **坐标流**（`docs/03` 8.1）每个段落由 inlines 拼接，规则固定：

| 项 | 贡献 |
| --- | --- |
| `Text`/`DelText` | 文本本身（`xml:space` 规则见下） |
| `Tab`/`PTab` | `U+0009` |
| `Br{TextWrapping}`/`Cr` | `U+000A` |
| `Br{Page|Column}` | `U+FFFC` |
| `NoBreakHyphen` | `U+2011`；`SoftHyphen` → `U+00AD` |
| `Sym` | 解码后的字符（符号字体 → Unicode 映射，`RES-05`；失败 → `U+F000 + (code & 0xFF)`） |
| `Drawing`/`Pict`/`Object`/`Ruby`/`FootnoteRef`/`EndnoteRef`/`Separator`/`ContinuationSeparator`/`Other` | `U+FFFC` |
| `FldChar`/`InstrText`/`DelInstrText`/`CommentRef`/`LastRenderedPageBreak`/`AnnotationRef`/`FootnoteRefMark`/`EndnoteRefMark` | 空（长度 0） |
| `Inline::Field`（原子形态） | 单个 `U+FFFC`，其 `result` 不参与 |
| `Inline::Atom` | 单个 `U+FFFC` |
| 范围标记 | 空 |

- `xml:space`：`w:t`/`w:delText` 无 `xml:space="preserve"` 时，去掉首尾 `[ \t\r\n]`（Word 行为）；有则原样。`w:instrText` 恒视为 preserve。
- 文本段的实体在读取时解码一次（`XML-06`）。
- 透明字段（`Link`/`FLD-07`）：结果 run 作为普通 `Inline::Run` 出现并带 `field: Some(id)`；结构 run（begin/instr/separate/end）作为 `Run` 出现但只含长度 0 的段，`compat_ts` 与编辑器可据 `field` 与 `SegmentKind` 隐藏。
- 修订上下文 `RevisionCtx { ins: Option<RevisionMeta>, del: Option<RevisionMeta>, move_from, move_to }` 由 run 的祖先 `w:ins/w:del/w:moveFrom/w:moveTo` 决定；`w:moveFrom` 计入 `del`，`w:moveTo` 计入 `ins`（TS 语义），同时保留 `move_*` 精确信息。
- `comments`：覆盖该 run 的批注 `SpanId` 列表（由 Span 索引反查）。
- 符号字体：`Sym` 与 `rFonts` 为符号字体（Symbol/Wingdings/Wingdings 2/Wingdings 3/Webdings）的 `Text` 段，其**显示**文本经 `RES-05` 解码；`Run.text` 保持原字符，解码结果放在 `Segment.display`。（△ TS 直接改写 run 文本并删除 rFonts；此处不改写规范状态。）

## MOD-07 表格

```
TableBlock { node, props: TableProps, grid: Vec<GridCol { node, w: Option<Val<i32>> /* 声明值，允许 0 与缺失 */ }>, rows: Vec<Row>, style_id, sdt, revisions }
Row { node, props: RowProps, tbl_pr_ex: Option<TableProps> /* w:tblPrEx，行级表格属性例外 */, cells: Vec<Cell>, sdt, revisions }
Cell { node, props: CellProps, blocks: Vec<Block>, sdt, revisions }
```

- `Cell.blocks` 由与正文同一构建器产生（段落 / 嵌套表 / sdt / 修订包裹递归分类）。单元格最后一个块**必须**是
  `w:p`（Word 约束）；编辑操作负责维持它（`EDIT-03` 表格通则）。
- `Document::paragraphs()` 迭代全部段落（含单元格内任意深度、sdt 内）；`Document::block_path(node)` 给出从顶层块到
  该节点的路径，供 `MOD-13` 的容器级刷新与 `EDIT-02` 的定位使用。`text_blocks()` 仍只给顶层文本块。
- 构建**迭代**实现：语料有 2000 层嵌套、hostile 有 5000 层。

- 行与单元格通过 `semantic_children` 加"穿透 `w:sdt`"取得（研究报告模板把 tr/tc 包在 sdt 里）；被包裹的 tr/tc 附 `SdtInfo`。
- `w:hMerge` 不在模型层折叠（保持声明值）；`resolve` 提供折叠后的网格视图。
- 嵌套表格是 `Cell.blocks` 中的 `Block::Table`。递归深度 > 64 的子表 → `Protected(TooDeep)`，其 DOM 原样保留。
- 不做 tcW 与 tblGrid 的"校正"（TS 的 `tcwColumnWidths`）：两者都以声明值给出，校正属于 `resolve`（`RES-08`）。

## MOD-08 SdtInfo

```
SdtInfo { node, alias, tag, id, control: RichText|PlainText|Picture|ComboBox|DropDownList|Date|Checkbox|Group|Citation|Bibliography|DocPartObj|DocPartList|Equation|RepeatingSection|RepeatingSectionItem|Unknown,
          lock: Unlocked|SdtLocked|ContentLocked|SdtContentLocked, data_binding: Option<{prefix_mappings, xpath, store_item_id}>,
          doc_part: Option<{gallery, category, unique}>, placeholder: Option<doc_part_id>, showing_placeholder: bool }
```

编辑策略在 `EDIT-03`（`ContentLocked/SdtContentLocked` 只读；`data_binding` 第一阶段只读）。

## MOD-09 修订

`Revision` 枚举与 `RevisionMeta` 见 `docs/03` 6.6。附着位置：

| 修订 | 附着 |
| --- | --- |
| `Insert`/`Delete`/`MoveFrom`/`MoveTo`（run 级） | `Run.rev` |
| `Insert`/`Delete`（块级 `w:ins/w:del` 包裹 `w:p`/`w:tbl`） | `Block.revisions` |
| `ParaMarkInsert`/`ParaMarkDelete`（`pPr/rPr/ins|del`） | `TextBlock.revisions` |
| `RunPropsChange` | `Run.rev`（`old: RunProps` 由 `PROP-06` 读 `rPrChange/rPr`） |
| `ParaPropsChange` | `TextBlock.revisions`（`old: ParaProps + old_style + old_list`） |
| `SectPropsChange`/`TableGridChange` | 对应对象的 `revisions`，`old: NodeId`（快照里的 `sectPr` / `tblGrid` 节点） |
| `TablePropsChange`/`RowPropsChange`/`CellPropsChange` | `TableBlock` / `Row` / `Cell` 的 `revisions`，`old` 是类型化快照（`Box<TableProps>` 等，由属性表的 `read_*_change` 读出） |
| `NumberingChange` | `TextBlock.revisions` |
| `CellInsert`/`CellDelete`/`CellMerge` | `Cell.revisions`；`Row.revisions` 承接 `trPr/ins|del` |
| `FieldInstrDelete` | 字段所在 `Run.rev` 与 `FieldSpan` |

每个修订带 `RevisionId`（会话内稳定），供 `Accept/Reject` 引用。

## MOD-10 其他子模型（声明值）

- **Styles**：`StyleInfo { id, name, kind: Paragraph|Character|Table|Numbering, based_on, next, link, default, hidden, semi_hidden, unhide_when_used, q_format, ui_priority, ppr: ParaProps, rpr: RunProps, tbl: Option<TableStyleDecl{ tbl_pr, tr_pr, tc_pr, conditional: Vec<{type, ppr, rpr, tbl_pr, tr_pr, tc_pr}> }> }` 与 `doc_defaults { rpr, ppr }`。不做链解析（`RES-02`）。
- **Numbering**：`AbstractNum { id, nsid, multi_level_type, tmpl, name, style_link, num_style_link, levels: [Level; ≤9] }`，`Level { ilvl, start, num_fmt(含 w14 custom format), lvl_restart, p_style, is_lgl, suff, lvl_text, lvl_pic_bullet_id, legacy, lvl_jc, ppr, rpr }`，`Num { num_id, abstract_num_id, overrides: Vec<{ilvl, start_override, lvl}> }`。
- **Theme**：字体方案（major/minor 的 latin/ea/cs 及 `a:font script→typeface` 表）、颜色方案 12 槽（`sysClr` 取 `lastClr`）。
- **Settings**：`CompatFacts`（`docs/03` 6.5）、`document_protection`、`write_protection`、`remove_personal_information`（前缀无关解析）、`even_and_odd_headers`、`auto_hyphenation`、`default_tab_stop`、`theme_font_lang`、`track_revisions`、`update_fields`、`rsid` 原样。
- **Comments**：`comments.xml` + `commentsExtended.xml`（`paraIdParent`、`done`）+ `commentsIds.xml`（durableId）+ `people.xml`。
- **Notes**：条目带 `w:type`（separator 等）为结构条目，`kind` 非 `Normal`。
- **Sources**：`b:Sources` 的 `b:Source` 建模字段与 TS 一致，其余 `Raw`。
- **FontTable**：`name, altName, panose1, family, pitch, charset, sig, embed*`。
- **Sections**：`SectionInfo { node /* sectPr */, props: SectionProps, owner: Body|Paragraph(NodeId), block_range }`；继承在 `RES-10`。

## MOD-11 显示模型（只含文档事实）

挂在 `Segment.display` / `ProtectedBlock.display` 上：

- `ImageDisplay { media: Option<MediaId>, external: Option<String>, extent_emu, crop, fill_rect, rot_60k, flip_h, flip_v, border: Option<{color, w_emu}>, anchor: Option<AnchorGeom>, alt, name }`
- `AnchorGeom { rel_h, rel_v, align_h, align_v, offset_h_emu, offset_v_emu, pct_h, pct_v, wrap: None|Square{wrap_text}|Tight{..}|Through{..}|TopAndBottom, behind_doc, allow_overlap, relative_height_raw, layout_in_cell, dist_t/b/l/r, hidden }`
- `ShapeDisplay { prst, xfrm{off, ext, rot, flips}, fill: Solid(rgb)|Gradient(stops)|Pattern{fg,bg}|Blip(media)|None, line: Option<{color, w_emu, dash, head, tail}>, body_pr{insets, anchor, autofit, wrap, vert}, style_refs{fill_ref, ln_ref, effect_ref, font_ref}, content: Option<Vec<Block>> /* txbxContent，独立内容流 */, group: Option<GroupCtm> }`
- `VmlDisplay { kind: Shape|Rect|RoundRect|Oval|Line|Group|Image|TextPath|Hr, style: Map<String,String> /* 原始 style 键值 */, fill, stroke, imagedata: Option<MediaId>, textpath: Option<String>, content: Option<Vec<Block>> }`
- `ChartDisplay`、`DiagramDisplay`、`OleDisplay`、`RuleDisplay`、`FormulaDisplay { omml_node, tokens, mathml, latex }` 字段与 TS 对齐，但几何用 EMU 原值，颜色经 `RES-05` 解析为 sRGB 并保留原始定义。

**禁止**在这些结构中出现由排版决定的字段（碰撞位移后的偏移、band 高度、猜测的 floatSide 等）。

## MOD-12 诊断

`Document.warnings` 收集所有层的 `Diagnostic`；每次降级（`Protected(Unparseable|TooDeep|Unknown)`）**必须**对应一条诊断。

## MOD-13 重建与刷新

- `Document::rebuild(&dom_set, &spans) -> Document`：从规范状态完整构建。
- `Document::refresh(&mut self, result: &MutationResult)`：只重建 `affected_containers` 覆盖的段落/块及其祖先链上的聚合（表格、sdt），并重算受影响 part 的 `FieldIndex`/`SpanIndex` 引用。
- 不变式：任意操作序列后 `refresh` 结果与 `rebuild` 结果在忽略 `RevisionId`/内部缓存字段后相等（`TEST-07`）。

## 验收清单

| ID | 用例 |
| --- | --- |
| MOD-03 | `outlineLvl=9` 且样式为 Heading1 → Paragraph；样式 `numId 0` 取消继承编号 |
| MOD-05 | `docs/01` 第 6.2 节决策树中的每个分支各一个用例，标 △ 的用例断言新行为（含 REF 的段落为 Text；含锚定文本框的段落为 Text 且 Drawing 段带 ShapeDisplay） |
| MOD-06 | `"Hello" + <w:tab/> + "World"` 坐标流为 `Hello\tWorld`；含图片 run 的段落坐标流含 1 个 U+FFFC；PAGE 字段结果 `12` 只占 1 单位；无 preserve 的 `<w:t> x </w:t>` 文本为 `x` |
| MOD-07 | sdt 包裹的 tr/tc 解析出行列；65 层嵌套第 65 层为 TooDeep；hostile `xml-deep-table` 无编辑保存字节相同；`w:tblPrEx` 读入 `Row.tbl_pr_ex`；全语料每张表行数、每行物理 `w:tc` 数与 TS 对得上（折叠 `hMerge` 与 `gridGap` 占位换算后） |
| MOD-08 | 带 `w:dataBinding` 与 `w:lock w:val="sdtContentLocked"` 的 sdt 字段正确 |
| MOD-09 | 每种修订至少一个语料用例，附着位置正确 |
| MOD-13 | 随机编辑后 `refresh == rebuild` |
