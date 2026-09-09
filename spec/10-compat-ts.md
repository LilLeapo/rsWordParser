# SPEC 10 · 兼容适配器（bind/compat_ts）

对应 `docs/03` 第 8.4、第 L5 节。职责：把新模型投影为 TS `ParsedDoc` 的 JSON，并把 `SaveBlock[]` 翻译为 `EditOp`。

> **生命周期（2026-09-08 改判，`docs/03` v3.3 §14 / `spec/19` 决策 2）**：M1 建立。原定「M9 删除」——那个判断的前提是它要作为
> **对外契约**长期维护、genoffice 编辑器是使用者。范围改定之后 genoffice 退为测试基准，本模块的价值反过来了：它是
> 1,065 份文档差分的对接点，是目前最强的正确性证据。因此**保留、不删**，在 M8′ 8.7 降级为 `#[cfg(feature = "compat-ts")]`
> 的**测试专用件**——默认构建不含、不进公共 API、不承诺稳定。本文件的条目全部继续有效（**不标 `[已撤销]`**），
> 只是读者要知道：这里描述的是「与 TS 差分时的投影形态」，不是 rsword 的对外接口。对外接口见 `spec/21-bind.md`。
> 原「使编辑器零改动接入」的目的已撤销。

## COMPAT-01 原则

- 适配器**只读**规范状态与投影，不反向影响模型设计。
- 复现 TS 行为的所有"半解析"规则（字体五字段、控制字符、passthrough 类型判定、sdt 拆分、dataURL 内联）都封装在此模块，代码注释标注对应 `docs/01` 小节。
- 无法或不值得复现的差异登记在 `compat_ts/KNOWN_DIFFS.md`，差分工具据此过滤（`TEST-03`）。

## COMPAT-02 `ParsedDoc` 字段映射

**顶层**

| TS 字段 | 来源 |
| --- | --- |
| `blocks[]` | body 内容流的 `Block` 序列经 `COMPAT-03` 映射；`docxIndex` 见 `COMPAT-04` |
| `comments[]` | `Document.comments`：`{id, author, initials, date, text(段落 textOf 以 \n 连接), parentId, done, paraId}` |
| `footnotes/endnotes[]` | `Note{kind: Normal}` → `{id, text, richParas?}`；`richParas` 按 TS `noteRichParas` 规则（首段去前导空格、跳过 `footnoteRef` run、任一 run 有格式才给） |
| `sources[]`、`inks[]` | 同名子模型；ink 由 `DrawingFacts.is_ink` 的段收集 |
| `themeFonts/themeColors` | `Theme` |
| `protection/writeProtection/removePersonalInfo` | `Settings` |
| `headerText/footerText/headerParas/footerParas/headerImages/footerImages/watermarkText/footerHasPageNumber/headerHasPageNumber` | default 变体 hf part 经 `COMPAT-05` |
| `titlePg/evenAndOddHeaders/compatibilityMode/autoHyphenation/defaultTabStopTwips` | `Settings`、任一 `sectPr.titlePg` |
| `headerFirst/footerFirst/headerEven/footerEven`、`hfParts{rId}` | 各 hf part 经 `COMPAT-05` |
| `styles` (Map) | `Styles` → TS `StyleInfo`（`display` 用 `resolve` 的样式链结果按 TS 的字段子集与"半解析"规则；`tableDisplay` 同理） |
| `docDefaults` | `doc_defaults` 经 `RES-05` 的 EA 回填规则 |
| `headingStyleIds`、`listParagraphStyleId` | 由 `Styles` 推导（TS 规则） |
| `numbering` (Map) | `Numbering` → TS `NumberingDef`（`numStyleLink` 已解析、`lvlOverride` 已应用、`formats` 由 level 0 推导） |
| `internal.documentXml` | 主 part 字节转 UTF-16 字符串 |
| `internal.bodyInnerStart/End` | body 第一个顶层子节点 `lex.range.start` 与最后一个的 `.end`，**转为 UTF-16 索引**（`COMPAT-06`） |
| `extras.elements[]` | 与 `blocks` 对齐的 `{name, start, end}`（UTF-16 索引） |
| `extras.chartParts{path}` | 图表 part 原字节字符串 |

**Block**（通用）

| TS 字段 | 来源 |
| --- | --- |
| `id` | `b{docxIndex}` |
| `type/label/previewText/fieldDisplay/hidden/invisibleMarker/decorative/brokenImage` | `COMPAT-03` |
| `docxIndex/originalXml` | `COMPAT-04` |
| `styleId/level/list/format` | `TextBlock` 的 `style_id/kind/props` → TS `ParaFormat`（`RES-07` 的视觉 `align`；`autoSpace` 按 TS 规则从样式补） |
| `rawPPr` | `pPr` 节点 `lex.range` 字节 |
| `runs[]` | `COMPAT-07` |
| `bookmarks/hiddenBookmarks/commentStarts/commentEnds` | 由 `SpanIndex` 按 TS 规则：起点在本段的书签名（`_` 前缀分流）；只有一端在本段的批注 id |
| `table` | `TableBlock` + `RES-08` 的 `TableView` → TS `TableModel`，逐字段规则见 `COMPAT-10` |
| `sdtShell` | `SdtInfo` → `{alias, tag, controlType, openXml, closeXml, group}`：`openXml` = sdt 开标签到 `sdtContent` 开标签结束的字节，`closeXml` = `</w:sdtContent></w:sdt>`；多子块 sdt 按 TS `splitSdtParts` 规则分配 `group` 与首末块的 open/close |
| `moveRevision/pPrChangeInfo/blockRevision/paraMarkDel` | `revisions` |
| `textboxes/strayRuns/strayStyleId/formulaDisplay/chartDisplay/diagramDisplay/image*` | `COMPAT-03` |

## COMPAT-03 块类型复现

TS 的 `buildBlock` 决策树（`docs/01` 6.2）在新模型中已不存在；适配器**用 facts 复现它**，把新模型的 `Text`（含 Drawing/Object/Field 段）映射回 TS 的 passthrough 变体：

| 新模型 | TS 输出 |
| --- | --- |
| `Protected(SectionProps)` | `passthrough`, `hidden`, label `Section properties` |
| `Protected(Invisible)` | `passthrough`, `invisibleMarker`, label 按来源 |
| `Protected(BodyBreak{page})` | `passthrough` + `fieldDisplay{pageBreak}` / `invisibleMarker` |
| `Protected(SectionBreak)` | `passthrough`, label `Section break paragraph` |
| `Protected(FieldBlockResult)` | `passthrough`，`label = fieldLabel(instr)`，`fieldDisplay = fieldDisplayOf`（TOC 行拆 left/right/level/anchor/num） |
| `Protected(Equation)` | `passthrough` label `Equation` + `formulaDisplay` |
| `Protected(Chart/SmartArt)` | `passthrough` label `Chart`/`SmartArt` + display |
| `Protected(Ole)` | `passthrough` label `Embedded object` + `oleDisplay` 字段 |
| `Protected(Rule)` | `passthrough` label `Drawing object` + `decorative` + rule 字段 |
| `Image` | `type image` + `imageDataUrl` + `imageMeta` 字段（含 TS 的 wrap 分类、zOrder、posH/V 规则） |
| `Table` | `type table` |
| `Text` 且 `facts` 命中 TS 保护条件（含非 XE/REF/简单/可转换 HYPERLINK/FORMCHECKBOX 的字段；含 `delInstrText`；含带文字的锚定文本框且无段落文字等） | 对应 TS passthrough（`Field (…)`、`Revised paragraph`、`Text box` + `textboxes/strayRuns`、`Drawing object`、`Image` brokenImage…），规则逐条移植 `docs/01` 6.2 并在代码中标注小节号 |
| `Text` 其他 | `paragraph/heading/listItem` |

复现的判定必须基于 facts 与显示模型，**不得**重新解析 XML。

### COMPAT-03a `fieldLabel` / `fieldDisplayOf`

TS 这两个函数的规则（`docs/01` 只给了函数名）从语料 33 个 `fieldDisplay` 实例反推，实现里逐条标注：

- **走 passthrough 的条件**（TS `buildBlock` 规则 2 / 3，逐段判定）：段落自身有 `w:fldChar` /
  `w:instrText` / `w:fldSimple`，且不是"全部可折叠"（`onlyXeFields`：无 `w:fldSimple`，每个字段是
  XE / REF / 简单内联字段（DATE TIME CREATEDATE SAVEDATE NUMPAGES FILENAME AUTHOR PAGE）/ 可转换
  HYPERLINK（`HYPERLINK "url"`，最多再带 `\o "tip"`）/ 有 `w:checkBox` 定义的 FORMCHECKBOX）；
  配不上对的 `fldChar`（未闭合、孤立 end）一律不可折叠。否则若段落样式是目录系列 → `TOC entry`。
- **`fieldLabel`**：段落里**第一条**指令的关键字决定标签——TOC → `Auto TOC (updates when opened in
  Word)`、PAGEREF → `Page reference field`、INCLUDEPICTURE → `Linked picture field`、HYPERLINK →
  `Hyperlink field`、SEQ → `Caption number field`、PAGE → `Page number field`、其余 → `Field (关键字)`；
  一条指令都没有（只剩孤立 `fldChar`）→ `Field end marker`，段落里还有分页符再加 ` + page break`。
- **`fieldDisplayOf`**：段落没有可见文字而有分页符 → `{kind: pageBreak}`；否则目录样式 →
  `{kind: tocLine, left, right, level, anchor?, num?, szHalfPoints?}`（显示文字按制表符切开，最后一段是
  `right`，`num` 是形如 `1.1.` 的纯数字前缀，`anchor` 取段落里 `w:hyperlink/@w:anchor`，`szHalfPoints`
  取第一个有文字的 run）；否则有文字 → `{kind: text, left（trim 过的整段文字）, align?, fontFamily?,
  szHalfPoints?, lineRawTwips?, lineRule?, lineSpacing?, runs?}`（字号不统一时逐 run 给 `runs`）；
  都不成立 → 没有 `fieldDisplay`。

## COMPAT-04 docxIndex 与 originalXml

- TS 的 `extras.elements` 是 body 顶层元素序列，但 **sdt 被拆分**：`splitSdtParts` 把含 ≥2 个 `w:p/w:tbl` 子节点的 sdt 拆成多条，每条的区间划分为"首块从 sdt 开头、末块到 sdt 结尾、中间到下一子块开头"。适配器复现此划分：`elements[i]` 与 `blocks[i].docxIndex == i`。
- `originalXml` = 对应区间字节；顶层 `w:ins/w:del` 包裹块的 `originalXml` 为整个包裹。
- 范围标记等不产生 Block 的顶层节点在 TS 中是 `invisibleMarker` 块：适配器为它们补块以保持 `docxIndex` 对齐。

## COMPAT-05 页眉页脚

复现 TS `hfContentFromXml/hfParagraphs/hfImages`：`PAGE`/`NUMPAGES` 原子 → 文本 ``/``；其他字段 → 缓存结果 run；表格 → 每行一个 `HfParagraph{cells}`（浮动表延后）；文本框内段落提出；`ptabAligns`、`frameXAlign`、样式 `tabStops/align`；尾部空段落去除；图片列表（内联/锚定、VML）与 `hfTableMedia` 规则。

## COMPAT-06 索引转换

字节偏移 → UTF-16 索引：对主 part 字节做一次前缀表（每 4 KiB 记录一次 UTF-16 累计），查询 O(log n + 4 KiB)。`documentXml` 字符串与索引必须一致。

## COMPAT-07 Run 映射

| TS 字段 | 来源 |
| --- | --- |
| `text` | 坐标流文本经控制字符折回：`Br{Page}` → `\f`，`Br{Column}` → `\v`，`Br{TextWrapping}`/`Cr` → `\n`，`Tab/PTab` → `\t`，`NoBreakHyphen` → `‑`，`Sym` → 解码字符或 PUA；`U+FFFC` 的对象段不出现在 `text`（TS 把图片 run 的文本置空） |
| `rawRPr` | `rPr` 节点字节（△ TS 是重序列化结果；`KNOWN_DIFFS` 登记引号/自闭合差异） |
| `styleId` | `rStyle`（`Hyperlink` 除外） |
| `bold/italic/underline/strike/color/sizeHalfPoints/font/eaSlotEmpty/fontAscii/fontCs/csFont/rtl/charSpacingTwips/caps/charScalePct/highlight/shading/cs/vertAlign/em` | `RES-05/06` 按 TS 的半解析规则（不做样式链，只做主题） |
| `link` | `Link::Hyperlink` → `{href, rId, tooltip}`；`Link::Field` → `{href, tooltip}` |
| `commentIds` | 覆盖该 run 且起止都在本段的批注（TS 规则）；reference-only 批注挂最近 run |
| `ins/del` | `RevisionCtx` |
| `noteRef` | `FootnoteRef/EndnoteRef` 段 → `{kind, id}`，`text` 为 part 内序号 |
| `xeTerm/refField/refInstr/instrField/fldBeginXml` | 原子字段按策略：XE → `xeTerm`；REF → `refField/refInstr`，`text` = 结果文本；简单字段 → `instrField`；FORMCHECKBOX → `instrField + fldBeginXml`（begin run 字节）；可转换 HYPERLINK → 结果 run 带 `link` |
| `rPrChange` | `RunPropsChange` |
| `math` | `Inline::Atom(Math)` → `{omml: 节点字节}`，`text` = token 串 |
| `ruby` | `Ruby` 段 → `{rt, xml}`，`text` = base |
| `image` | `Drawing/Pict/Object` 段 → `{dataUrl, widthPx, heightPx, xml, wrap, offsetXEmu, offsetYEmu, noOverlap, border, lineCenterV}`（按 TS `imageMeta` 规则从 `AnchorGeom` 推导） |
| 合并 | 相邻 run 按 TS `sameStyle` 合并（rawRPr 相等等条件），原子 run 不合并 |

TS 的碰撞位移（`allowOverlap=0`）在适配器中复现于 `image.wrap/offset`（属显示决定，登记 `KNOWN_DIFFS` 待渲染器接管后删除）。

## COMPAT-08 SaveBlock → EditOp

见 `EDIT-04`。`GeneratedBlock.runs` → `NewInline`：文本中的控制字符折回段种类；`math.omml/ruby.xml/image.xml/fldBeginXml` → `NewInline::Xml(fragment)`；`xeTerm/refField/instrField` → `NewField`；`bookmarks/hiddenBookmarks/commentStarts/commentEnds` → `NewInline::Marker`；`rawPPr` → `SetParaProps` 时的基底；`sdtShell` 忽略（DOM 保留）。

## COMPAT-09 差分容忍

差分工具（`TEST-03`）比较规则：键顺序无关；`undefined` 与缺失等价；浮点按 1e-6；`KNOWN_DIFFS.md` 中列出的路径模式跳过并计数。

保存比较（`COMPAT-08`，`tests/save_blocks.rs`）在规范化文本上容忍**分配细节**：`w:p` 的 `w14:*` / `w:rsid*`、新建 `w:t` 的 `xml:space`、`wp:docPr` / `pic:cNvPr` 的 `@id` 与由它派生的 `@name`（TS 从 8000 / 9000 起计，我们按 `EDIT-06` 最大值 + 1；M6 6.6 / 6.7），以及墨迹锚（`wp:docPr/@name` 以 `aidocs-ink` 开头的 `wp:anchor`）的 `@relativeHeight`（TS `251658240 + docPrId`，同样由 id 派生；6.8）。普通锚定图片的 `relativeHeight` 是输入的 z-order，照常比较。

## COMPAT-10 表格模型复现

TS `extractTable` 一族（`docs/01` §7；权威定义是 genoffice `src/types.ts` 的 `TableModel` / `TableCell`，`docs/01`
§3.5 少了基线之后加的 `autoFit / fixedLayout / cellSpacingTwips / fill / tableLook / repeatHeaderRows / gridGap /
anchoredBoxAnchors`）在新模型中的来源。输入是 `TableBlock` 与 `resolve` 的 `TableView`（`RES-08`），**不重新解析
XML**；只有深度 ≥ 8 的扁平化允许直接读 DOM 文本（模型在 64 层截断，TS 的扁平表要全部段落）。

**表级**

| TS 字段 | 来源 |
| --- | --- |
| `rows` | `TableView` 折叠 `hMerge continue` 后的行列（并入左格的 `colSpan`，`tcW` 相加）；没有格的行不计；`gridBefore / gridAfter > 0` 的行在首 / 尾补 `{ paras: [], gridGap: true, colSpan? }` 占位——在样式条件格式**之后**补，不参与条带计数 |
| `colWidthsPct` / `colWidthsTwips` | `ColumnView.widths_twips`：`Pct` 按总和归一；`Twips` 仅当 `source ≠ Grid` 或 grid 全部 > 0 |
| `widthPct` | `tblW type=pct`：`w / 50`，字面 `NN%` 直取；(0, 100] 有效 |
| `autoLayout` / `autoFit` / `fixedLayout` | `fixed = tblLayout type=fixed`；`autoWidth = 无 tblW ∨ type=auto ∨ (dxa 且 w ≤ 0)`；`autoLayout = !fixed ∧ (autoWidth ∨ widthPct)`（为 true 才给）；`autoFit = fixed ∨ (!autoWidth ∧ !widthPct) ? fixed : widthPct == 100 ? window : contents`（总是给）；`fixedLayout = fixed`（为 true 才给） |
| `cellMarTwips` | 文档 `tblCellMar` → 表格样式链的 `tblCellMar`（`RES-08` 回退）；`start / end` 归到 `left / right`，type 缺省或 dxa，每边首个有效值 |
| `cellSpacingTwips` | `tblPr/tblCellSpacing`（dxa，> 0）→ 第一行 `trPr/tblCellSpacing` |
| `fill` | `tblPr/shd` 的显示填充（`fill` 非 auto；图案底纹按 TS 混色近似） |
| `borders` | `tblBorders`（含 inside）重复容器按边合并后者胜、`w:val` 必须存在、`szEighths / color`；缺 → 样式链 `tblBorders` |
| `align` | `jc`：`center` → center；`right / end` → right；其他不给 |
| `indentTwips` | `tblInd`（type 缺省或 dxa，非 0） |
| `floatSide` / `floatPos` | 有 `tblpPr` 才给：`floatSide = tblpXSpec ∈ {right, outside} ∨ (无 xSpec ∧ tblpX > 4680) ? right : left`；`floatPos = { xTwips（缺省 right → 9360 / left → 0）, yTwips（缺省 0）, horzAnchor / vertAnchor ∈ {page, margin, text}, distanceTwips 四边 ≥ 0 }`（显示启发式，只在此处） |
| `rowHeightsTwips` / `rowHeightRules` | 每行 `trHeight`：`val > 0` → `min(val, 31680)`，`hRule = exact ? exact : atLeast`；否则 `null`；全 `null` 两项都不给 |
| `repeatHeaderRows` | 每行 `trPr/tblHeader`（无 `trPr` 为 false）；**总是**给 |
| `rawTrPrs` | 每行 `trPr` 节点原字节，无则 `null`；全 `null` 不给 |
| `rowRevisions` | 每行 `trPr/ins\|del` → `{ kind, author, date?, id? }`；全 `null` 不给 |
| `tblStyleId` / `bidiVisual` | `tblStyle/@val`；`bidiVisual` 为 true 才给 |
| `tableLook` | 六个布尔**总是**给：`firstRow / lastRow / firstColumn / lastColumn`，`bandedRows = !noHBand`，`bandedColumns = !noVBand`；属性 > `w:val` 十六进制位（`0x20 0x40 0x80 0x100 0x200 0x400`）> 缺省（firstRow / firstColumn / bandedColumns 开） |

**格级**

| TS 字段 | 来源 |
| --- | --- |
| `paras` | 每个直接段落（穿透 sdt）的坐标流文本按 `COMPAT-07` 折回控制字符 |
| `richParas` | 每段 `ParaFormat`（与 `COMPAT-02` 的 `format` 同一函数）+ `runs`（`COMPAT-07`）+ `styleId` + `list` + `emptyRunSizeHalfPoints / emptyRunFontFamily` |
| `rawTcPr` | `tcPr` 节点原字节（TS `attachRawTablePr` 的"数不对就放弃"在新模型不需要：节点一一对应） |
| `colSpan` | `gridSpan > 1`，再加折叠进来的 `hMerge continue` 格的跨度 |
| `vMerge` / `hMerge` | `vMerge`：`restart` → restart，其他 → continue；`hMerge` 同（只在没被折叠掉的格上出现） |
| `fill` / `bold` / `color` | 自身：`tcPr/shd` 显示填充；`bold` = 有文字的 run 全部 `w:b`（且至少一个）；`color` = 有文字的 run 颜色恰一种（非 none）。自身未设时按 `TableView.cell(r, c)` 的条件格式补（TS `applyTableStyleDisplay`：firstRow > lastRow > firstCol > lastCol > 条带（行号从 firstRow 之后起算，偶 band1 奇 band2，`noHBand` 关）> 整表 `fill` / `wholeTable.bold / color`） |
| `align` | 有文字的段落的 `jc` 集合大小为 1 且 ∈ {center, right, left, justify}（视觉值，`RES-07`） |
| `vAlign` / `textDirection` / `cellMarTwips` / `borders` | `tcPr`：`vAlign ∈ {top, center, bottom}`；`tbRl / tbRlV → tbRl`，`btLr / btLrV → btLr`；`tcMar`；`tcBorders` 合并规则同表级（不含 inside） |
| `nestedTables` / `nestedTableAnchors` | `Cell.blocks` 中的 `Block::Table` 递归；锚点 = 该表之前的段落数；深度 ≥ 8 → TS `flattenedTableModel`：`{ rows: [[{ paras, richParas: 每段一个纯文本 run（空段无 run） }]], autoLayout: true }`，段落文本按文档序直读 DOM（迭代） |
| `cellRevision` | `tcPr/cellIns\|cellDel` → `{ kind, author, date?, id? }` |
| `anchoredBoxes` / `anchoredBoxAnchors` | 格内段落里的锚定形状（M4 的 `ShapeDisplay` / `VmlDisplay`）做成只读展示框挂在**格**上——Word 把它们画在格里并把行撑高，所以不像正文段落那样把整块降级成 `Text box`；`anchoredBoxAnchors` 是框所在段落的下标。取框之后要把「锚定且整棵没有 `pic:pic` 的绘图」与「带 `txbxContent` 的 `w:pict`」从段落里剥掉再取 `paras` / `richParas`，否则框里的文字与 `wp:posOffset` 的数字会漏进单元格文本 |
| 格内 run 的 `image` | 与正文同一条路（`COMPAT-07`）。只有一张图的格内段落被 `MOD-05` R15 分成图片块，TS 在格里一律当普通段落，所以按块上的显示模型补一个 `text: ""` 的图片 run |

**`styles.*.tableDisplay`**（TS `tableStyleDisplayOf` + `mergeTableDisplay`）：`fill`（样式级 `tcPr/shd`）、`wholeTable
{ color, bold, italic, sizeHalfPoints }`（样式级 `rPr`）、`firstRow / firstCol / lastCol / lastRow { fill, bold, color,
sizeHalfPoints }`（对应 `tblStylePr`）、`band1Fill / band2Fill`（`band1Horz / band2Horz` 的 `tcPr/shd`）、`borders`、
`cellMarTwips`（样式级 `tblPr`）、`paraSpacing { beforeTwips, afterTwips, lineRawTwips, lineRule, lineSpacing }` 与 `paraJc`
（样式级 `pPr`）；basedOn 链**深合并**（`wholeTable / firstRow / firstCol / lastCol / lastRow / paraSpacing` 逐字段，
子样式有值才覆盖）。空对象不输出。

`label` 仍是 `Table R×C`（R = 整段 XML 里 `w:tr` 个数——含嵌套表，C = 第一个 `w:tr` 里 `w:tc` 个数），
`previewText` 为纯文本前 120 字；两者 M1 已有。

## 验收清单

| ID | 用例 |
| --- | --- |
| COMPAT-02/03 | 全部 `corpus/synthetic` 的期望 JSON 与适配器输出 diff 为空（除 KNOWN_DIFFS） |
| COMPAT-04 | 多段 sdt 的 `docxIndex` 与 `elements` 与 TS 一致 |
| COMPAT-06 | 含 emoji 的 `document.xml` 的 `bodyInnerStart` 与 TS 相同 |
| COMPAT-08 | 用 TS 测试中的 `SaveBlock[]` 驱动 → 输出 `document.xml` 与 TS `saveDocx` 的输出经 XPath 等价（`TEST-05`） |
| COMPAT-10 | 全部含表格块的语料（M4 域除外）的 `blocks[*].table` 与 `styles.*.tableDisplay` diff 为空；`deep-nested-table__001` 第 8 层为 1×1 扁平表且含其下全部段落文本；`table-grid-reconcile__*` 的 `colWidthsTwips` 与 `colSpan` 与 TS 一致 |
