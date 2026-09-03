# SPEC 10 · 兼容适配器（bind/compat_ts）

对应 `docs/03` 第 8.4、第 L5 节。职责：把新模型投影为今天 TS `ParsedDoc` 的 JSON，并把今天的 `SaveBlock[]` 翻译为 `EditOp`，使编辑器零改动接入并支持差分测试。生命周期：M1 建立，M9 删除。

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
| `table` | `TableBlock` → TS `TableModel`（`paras/richParas/rawTcPr/rawTrPrs/colWidthsPct/colWidthsTwips/widthPct/autoLayout/…`；样式条件填充按 TS `applyTableStyleDisplay` 烘进 `cell.fill/bold/color`；`hMerge` 折叠；`tcW` 校正按 `RES-08`） |
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

## 验收清单

| ID | 用例 |
| --- | --- |
| COMPAT-02/03 | 全部 `corpus/synthetic` 的期望 JSON 与适配器输出 diff 为空（除 KNOWN_DIFFS） |
| COMPAT-04 | 多段 sdt 的 `docxIndex` 与 `elements` 与 TS 一致 |
| COMPAT-06 | 含 emoji 的 `document.xml` 的 `bodyInnerStart` 与 TS 相同 |
| COMPAT-08 | 用 TS 测试中的 `SaveBlock[]` 驱动 → 输出 `document.xml` 与 TS `saveDocx` 的输出经 XPath 等价（`TEST-05`） |
