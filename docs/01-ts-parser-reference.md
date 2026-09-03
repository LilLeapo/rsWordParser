# docx Parser 开发文档（面向 Rust 重写）

> 对象：`packages/docx-engine` 中的 Word 解析器（`parseDocx` 及其支撑模块）。
> 目的：完整描述现有 TypeScript 实现的架构、数据模型、算法、兼容性细节与保存契约，作为 Rust 重写的规格书。
> 基线：仓库 `main` 分支，commit `a4d58fc`（2026-09-03）。

---

## 0. 一页总览

| 项目 | 内容 |
| --- | --- |
| 入口 | `parseDocx(bytes: Uint8Array): Promise<ParsedDoc & { extras }>`（`src/parse.ts`） |
| 输出 | `ParsedDoc`：`Block[]` 树 + 样式/编号/主题/页眉页脚/批注/脚注/保护等辅助模型 + `internal`（原始字节与 `document.xml` 字符串）+ `extras`（body 顶层元素的字节区间、图表 part 原文） |
| 代码量 | parser 侧约 9,000 行 TS：`parse.ts` 6,440 行，`types.ts` 1,457 行，其余支撑模块（section/notes/list-markers/theme/symbol-fonts/chart/math/scan/xml-utils/zip-load/…）约 1,500 行 |
| 依赖 | `jszip`（zip）、`fast-xml-parser`（`preserveOrder` 树）、`utif2`（TIFF 解码）、vendored `emf-converter`（EMF/WMF → PNG，依赖 Canvas） |
| 测试 | `tests/*.test.ts` 77 个文件、约 15,000 行；夹具由 `tests/helpers/build-docx.ts` 在内存里合成 |
| 消费方 | `apps/docs`（编辑器，主要消费者）、`packages/file-parse`（转纯文本）、`packages/pdf2docx`、`apps/markdown` |

**最重要的一句话**：这个 parser 不是"读出内容"就完事，它是 **patch-save（段落级补丁保存）** 架构的前半段。每个 Block 都记录它在原始 `document.xml` 中的精确字节切片（`originalXml`）和位置（`docxIndex`），未被编辑的 Block 保存时原样拷回；被编辑的 Block 再由 `generate.ts` 重建，并借助 `rawPPr`/`rawRPr`/`rawTcPr`/`sdtShell` 等"原文碎片"保留未建模的属性。Rust 重写如果丢了这些锚点，`patch.ts`/`generate.ts` 就无法工作，"无编辑 → 输出与输入字节相同" 的回归测试也会失败。

---

## 1. 范围与非目标

### 1.1 本文覆盖（parser 侧）

- `src/parse.ts`：主流程、Block 决策树、段落/run/表格/绘图/页眉页脚/图表/SmartArt 提取
- `src/types.ts`：全部输出模型
- `src/scan.ts`：body 顶层元素字节扫描
- `src/xml-utils.ts`：XML 树辅助函数
- `src/zip-load.ts`：zip 装载修正
- `src/section.ts`（读取部分）、`src/notes.ts`（`parseNotesXml`）、`src/sources.ts`（`parseSourcesXml`）、`src/theme.ts`（`readThemeFonts`/`readThemeColors`/`resolveThemeColor`）、`src/symbol-fonts.ts`、`src/list-markers.ts`、`src/watermark.ts`（`readWatermarkText`）、`src/chart.ts`（`parseChartPartXml`）、`src/math.ts`（`ommlFragmentsOf`/`mathTokensOf`/`ommlToMathML`/`ommlToLatex`）、`src/ink.ts`（`findInkRuns`/`stripInkRuns`）、`src/metafile.ts`、`src/tiff.ts`
- parser 对保存路径承诺的契约（第 7 节）

### 1.2 不覆盖（写回侧，另立文档）

- `src/generate.ts`（OOXML 片段生成）、`src/patch.ts`（`saveDocx`）、`src/text-patch.ts`、`src/blank.ts`、`src/resource-cleanup.ts`、`src/protection.ts` 的哈希生成

但第 7 节会列出 parser 输出中哪些字段是写回侧的输入，这些字段的语义必须逐字保留。

---

## 2. 总体架构

### 2.1 数据流

```
bytes
 │
 ├─ loadDocxZip()                 zip 装载（修正 Info-ZIP Unicode Path 字段）
 ├─ assertZipWithinLimits()       zip 炸弹防护（part 数 / 单 part / 总解压大小）
 ├─ resolveMainDocumentPath()     word/document.xml，否则 _rels/.rels 的 officeDocument
 │
 ├─ 辅助 part（互相独立，可并行）
 │    parseTheme        → ThemeFonts / ThemeColors（+ settings.xml themeFontLang）
 │    parseStyles       → Map<styleId, StyleInfo>, DocDefaults（依赖 theme）
 │    parseRels         → Map<rId, RelInfo>（document.xml.rels）
 │    parseNumbering    → Map<numId, NumberingDef>, formats
 │    parseComments     → CommentInfo[]（comments.xml + commentsExtended.xml）
 │    parseProtection / parseWriteProtection / parseRemovePersonalInfo
 │    parseNotesPart×2  → footnotes / endnotes
 │    parseSources      → SourceInfo[]（customXml/item*.xml）
 │
 ├─ scanBody(documentXml)         BodyElement[]  {name,start,end}
 ├─ tableBlipMedia()              预取表格内图片 rId → dataURL（因 extractCell 是同步的）
 ├─ sectSlices / sectionAt()      按字节偏移查询"管辖该位置的 sectPr"（页面几何）
 │
 ├─ for el in scan.elements:
 │    w:sdt 多子块 → splitSdtParts → 每个子块一个 Block（共享 sdtShell.group）
 │    否则 buildBlock(el, i, xml, ctx) → Block
 │
 ├─ applyTocEntryNumbers()        TOC 行的编号标记
 ├─ normalizeImageZOrders()       异常 relativeHeight 重排
 │
 ├─ readHeaderFooterPart ×6       default/first/even × header/footer
 ├─ parseAllHfParts()             按 rId 全部页眉页脚 part（多节文档用）
 ├─ settings 杂项                  titlePg / evenAndOddHeaders / compatibilityMode / autoHyphenation / defaultTabStop
 ├─ ink                           aidocs-ink 锚定图片 → InkInfo[]
 │
 └─ ParsedDoc { blocks, ..., internal, extras }
```

### 2.2 两层 XML 处理策略（实现哲学）

现有实现刻意混用两层：

1. **字符串/正则层**（面向字节保真与快速探测）
   - `scanBody`、`rawPPrOf`、`splitXmlChildren`、`topLevelDrawings`、`sectSlices`、`hfTblRanges`、`ommlFragmentsOf`、`rubyFragmentsOf`：在原文上找 **精确切片**，切片会原样写回。
   - `detect` 字符串（去掉 `mc:Fallback` 与 ink run 后的段落 XML）上的一系列 `includes`/正则：决定 Block 类型。
   - `plainText(xml)`：正则抓 `<w:t>` 求可见文本（含 `</w:tc>` 处补空格）。
2. **树层**（面向语义提取）
   - `fast-xml-parser` `preserveOrder:true, ignoreAttributes:false, trimValues:false, parseTagValue:false, parseAttributeValue:false`，节点形态 `XNode = { 'w:p': XNode[], ':@': {attr: value} }`，文本节点 `{ '#text': string }`。
   - 辅助：`nameOf/childrenOf/attrsOf/textOf/findChild/findChildren/childrenThroughSdt/boolProp/underlineProp/serializeXNode`。
   - `deepXmlParser`（`maxNestedTags: 100_000`）仅用于表格（POI 压力文件嵌套 5000 层）。

**Rust 对应**：可以用一次解析同时满足两层需求——在建树时给每个元素记录 `[start, end)` 字节区间、保留属性顺序、不做 trim、不解析实体（或只解析 XML 五个命名实体并保留数字实体原文）。这样 `originalXml`/`rawPPr`/`rawRPr` 直接从区间切出来，探测逻辑用树查询代替正则。详见第 8 节。

### 2.3 字符串索引体系

TS 中所有偏移（`BodyElement.start/end`、`bodyInnerStart/End`、`sectionAt(docOffset)`）是 **UTF-16 code unit** 索引。Rust 自然用 **UTF-8 byte** 索引。这只在 parser/patch 边界要求一致：`extras.elements` 与 `internal.bodyInnerStart/End` 必须和 `internal.documentXml` 用同一体系。如果 patch 侧也迁 Rust，整体用 byte 索引即可；如果 patch 暂留 TS，边界处要做转换或让 Rust 输出 UTF-16 索引。

---

## 3. 数据模型（`types.ts` 摘要）

### 3.1 单位与控制字符约定

| 量 | 单位 | 备注 |
| --- | --- | --- |
| 长度（页面、缩进、间距、表格宽、单元格边距、tab 位） | twips (1/1440 in) | `w:ind`/`w:spacing`/`w:pgSz`/`w:tcW` 原值 |
| 字号 | half-points | `w:sz`/`w:szCs` 原值 |
| 边框粗细 | eighth-points（`szEighths`）或 pt（`szPt = sz/8`） | 段落边框用 pt，表格边框用 eighths |
| 绘图几何 | EMU（原值）与 CSS px（显示） | `EMU_PER_PX = 9525`，`EMU_PER_PT = 12700`，`EMU_PER_TWIP = 635`；twips→px 为 `/15`；pt→px 为 `*96/72` |
| 颜色 | 6 位大写/原样 hex，无 `#` | `stripHash` 容忍前导 `#`（tdf#57589） |
| 表格百分比宽 | `w:tblW type="pct"`：`w/50` 为百分点；也容忍字面 `"NN%"` | |
| 行高 | twips，上限 31680（22in，MS-OI29500 2.1.51） | |
| 字符缩放 `w:w` | 百分数 | 100 不记录 |
| 旋转 `rot` | 1/60000 度 | 归一到 0–359 |

Run 文本里的控制字符：

| 字符 | 含义 | 来源 |
| --- | --- | --- |
| `\t` | 制表 | `w:tab`、`w:ptab` |
| `\n` | 软换行 | `w:br`（无 type 或 textWrapping）、`w:cr` |
| `\f` | 分页 | `w:br w:type="page"` |
| `\v` | 分栏 | `w:br w:type="column"` |
| `‑` | 不间断连字符 | `w:noBreakHyphen` |
| `` (`PAGE_MARK`) | PAGE 字段位置 | 仅页眉页脚 |
| `` (`TOTAL_PAGES_MARK`) | NUMPAGES 字段位置 | 仅页眉页脚 |
| `☐` / `☒` | FORMCHECKBOX 合成字形 | 配合 `Run.fldBeginXml` |

### 3.2 `Block`

```
type BlockType = 'paragraph' | 'heading' | 'listItem' | 'table' | 'image' | 'passthrough'

Block {
  id: `b${index}`
  type
  docxIndex: number | null          // extras.elements 下标；编辑器新建块为 null
  originalXml: string | null        // 原始 document.xml 精确切片（补丁锚点）

  // 文本块（paragraph/heading/listItem）
  level?            // heading 1–9
  styleId?          // w:pStyle
  list?: { kind: 'bullet'|'ordered', numId, ilvl }
  format?: ParaFormat
  rawPPr?: string   // 精确 <w:pPr>…</w:pPr> 切片
  runs?: Run[]
  bookmarks? / hiddenBookmarks?     // 用户书签 / _ 前缀内部书签（_Toc/_Ref/_Hlk…）
  commentStarts? / commentEnds?     // 跨段批注范围端点

  // 保护块（passthrough）通用
  label?            // 'Table 3×4' / 'Image' / 'Chart' / 'Text box' / 'Equation' / …
  previewText?
  hidden?           // 尾部 w:sectPr：保存时移到 body 末尾
  invisibleMarker?  // bookmarkEnd/proofErr 等落到 body 顶层：不渲染但位置保留
  decorative? + ruleColorHex/ruleThicknessPx/ruleWidthPx   // 细横线装饰
  brokenImage?

  // 图片（type 'image' 或 passthrough 上的 OLE 预览）
  imageDataUrl?, oleProgId?, imageWidthPx?, imageHeightPx?, imageCrop?, imageFillRect?,
  imageAlign?, imageWrap?: ImageWrap, imageZOrder?, imageZOrderNormalized?,
  imageOffsetXEmu?, imageOffsetYEmu?, imagePosH?, imagePosV?,
  imageRotDeg?, imageFlipH?, imageFlipV?, imageBorder?

  // 结构化显示模型
  table?: TableModel
  fieldDisplay?: FieldDisplay       // tocLine / text / pageBreak
  textboxes?: TextboxDisplay[]      // 锚定文本框/形状（显示用）
  strayRuns?: Run[], strayStyleId?  // 锚定段落自身的文字
  formulaDisplay?: FormulaDisplay   // tokens / mathml / omml / latex
  chartDisplay?: ChartDisplay
  diagramDisplay?: DiagramDisplay   // SmartArt / lockedCanvas
  sdtShell?: SdtShell               // 内容控件外壳（openXml/closeXml/group）

  // 修订
  moveRevision?: 'from' | 'to'
  pPrChangeInfo?: { author, date?, id?, old?: ParaFormat & {type?, styleId?, level?, kind?, numId?, ilvl?} }
  blockRevision?: { kind: 'ins'|'del' } & RevisionInfo   // 顶层 w:ins/w:del 包裹
  paraMarkDel?: RevisionInfo                              // w:pPr/w:rPr/w:del
}
```

`ImageWrap = 'square-left' | 'square-right' | 'tight-left' | 'tight-right' | 'through-left' | 'through-right' | 'topBottom' | 'behind' | 'front'`

### 3.3 `Run`

```
Run {
  text
  rawRPr?           // serializeXNode(<w:rPr>)，写回时 mergeRPrModel 合并
  styleId?          // w:rStyle，'Hyperlink' 不存（由 link 隐含）
  bold?, italic?, underline?, strike?, color?, sizeHalfPoints?
  font?             // eastAsia ?? ascii ?? hAnsi（主题已解析）
  eaSlotEmpty?      // font 是空 EA 主题槽的回填，不是文档选择
  fontAscii?        // ascii ?? hAnsi
  fontCs?           // w:cs 字面值（主题引用不解析，留在 rawRPr）
  csFont?           // cs/cstheme 解析后（显示用）
  rtl?, cs?         // cs = 选择了 Cs 孪生属性（bCs/iCs/szCs）
  charSpacingTwips?, caps?: 'all'|'small', charScalePct?, highlight?, shading?,
  vertAlign?: 'superscript'|'subscript', em?: 'dot'|'comma'|'circle'|'underDot'
  link?: { href, rId?, tooltip? }
  commentIds?       // 仅当 range 起止都在本段
  ins? / del?: RevisionInfo
  noteRef?: { kind: 'footnote'|'endnote', id }   // text = 显示编号
  xeTerm?           // XE 索引项（附在本 run 之后，不可见）
  refField?, refInstr?                            // REF 交叉引用（text = 缓存结果）
  instrField?       // DATE/TIME/CREATEDATE/SAVEDATE/NUMPAGES/FILENAME/AUTHOR/PAGE/FORMCHECKBOX
  fldBeginXml?      // FORMCHECKBOX 的 begin run 原文（含 ffData）
  rPrChange?: RevisionInfo & { old?: {...modeled subset} }
  math?: { omml }   // 原子内联公式，text = m:t token 串
  ruby?: { rt, xml }
  image?: { dataUrl, widthPx?, heightPx?, xml, wrap?, offsetXEmu?, offsetYEmu?, noOverlap?, border?, lineCenterV? }
}
```

### 3.4 `ParaFormat`

字段：`align` (`left|center|right|justify|distribute`)、`lineSpacing`（auto 倍数 = line/240）、`lineRule` (`auto|atLeast|exact`)、`lineRawTwips`、`indentLeft/indentRight`、`indentFirstLine`（负 = hanging）、`spaceBefore/spaceAfter`、`pageBreakBefore`（三态）、`keepNext`、`keepLines`、`widowControl`（仅显式关闭时 false）、`snapToGrid`（仅显式关闭时 false）、`autoSpace`（DE 与 DN 都显式关闭才 false）、`contextualSpacing`、`shadingFill`、`borders`（"tblr" 子集）、`borderStyle`、`borderLines`（每边 color/szPt）、`tabStops: TabStop[]`、`dropCap`、`frame: ParaFrame`、`bidi`、`emptyRunSizeHalfPoints`、`emptyRunFontFamily`。

`TabStop { pos: twips, val: left|center|right|decimal|bar|clear, leader?: dot|hyphen|underscore|heavy|middleDot }`

### 3.5 表格

```
TableModel {
  rows: TableCell[][]
  colWidthsPct?, colWidthsTwips?, widthPct?, autoLayout?
  cellMarTwips?: CellMargins, borders?: TableBorders (top/left/bottom/right/insideH/insideV)
  align?: left|center|right, indentTwips?, floatPos?, floatSide?
  rowHeightsTwips?: (number|null)[], rowHeightRules?: ('atLeast'|'exact'|null)[]
  rawTrPrs?: (string|null)[]            // 每行 <w:trPr> 原文
  rowRevisions?: ({kind:'ins'|'del'} & RevisionInfo | null)[]
  tblStyleId?, bidiVisual?
}
TableCell {
  paras: string[]                       // 每段纯文本（textOf）
  richParas?: TableParagraph[]          // ParaFormat & { runs, styleId?, list? }
  cellMarTwips?, nestedTables?: TableModel[], nestedTableAnchors?: number[]
  anchoredBoxes?: TextboxDisplay[]
  colSpan?, vMerge?: 'restart'|'continue', hMerge?（中间态，解析时折叠）
  fill?, color?, bold?, align?, vAlign?, textDirection?: 'tbRl'|'btLr'
  borders?: CellBorders, rawTcPr?: string
  cellRevision?: {kind:'ins'|'del'} & RevisionInfo
}
CellBorder { style, szEighths?, color? }
```

### 3.6 文本框 / 形状（显示模型）

```
TextboxDisplay {
  fill?, borderColor?, borderWidthPx?, borderDash?: 'dashed'|'dotted'
  widthPx?, heightPx?（autofit 关时才有）, minHeightPx?
  insetTop/Right/Bottom/LeftPx?
  prst?（非 rect 的预设几何；连线用合成值 line/lineArrow/lineArrowDouble/lineBent/lineCurved）
  wordArtId?, textOutline?, nowrap?, vAlign?: 'center'|'bottom'
  readOnly?          // 内容含 w:tbl/w:sdt 等，段落与 txbxContent 不再 1:1 → 禁止编辑
  paras: TextboxParaDisplay[]   // ParaFormat & { styleId?, runs }
  offsetXEmu?, offsetYEmu?, floating?, bandTopPx?, bandBottomPx?
  rotDeg?, fillImageDataUrl?, fillTile?, lineDiag?, flipH?, flipV?
}
```

### 3.7 样式与编号

```
StyleInfo { styleId, name, type: paragraph|character|table, headingLevel?, semiHidden?, qFormat?,
            linkedCharShell?, display?: StyleDisplay, tableDisplay?: TableStyleDisplay,
            numPr?: {numId, ilvl} | 'none', isDefault? }
StyleDisplay { sizeHalfPoints?, color?, bold?, italic?, boldCs?, italicCs?, sizeCsHalfPoints?, rtl?,
               underline?, strike?, font?, fontAscii?, eaSlotEmpty?, csFont?, charSpacingTwips?,
               tabStops?, caps?, lineSpacing?, lineRule?, lineRawTwips?, spaceBefore/AfterTwips?,
               indentLeft/Right/FirstLineTwips?, keepNext?, keepLines?, pageBreakBefore?,
               contextualSpacing?, align?, shadingFill?, autoSpace?, vanish? }
TableStyleDisplay { fill?, wholeTable?, firstRow?, firstCol?, lastCol?, lastRow?, band1Fill?, band2Fill?,
                    borders?, cellMarTwips?, paraSpacing?, paraJc? }
DocDefaults { sizeHalfPoints?, asciiFont?, eastAsiaFont?, eaSlotEmpty?, bold?, italic?, color?,
              lineSpacing?, lineRule?, lineRawTwips?, spaceBeforeTwips?, spaceAfterTwips?, lang? }

NumberingDef { numId, abstractNumId, levels: Record<ilvl, NumberingLevel>, startOverrides: Record<ilvl, number> }
NumberingLevel { numFmt, customFormat?, lvlText, start, suff?, indentLeft?, hanging?, firstLine?, szHalfPoints?, font? }
```

### 3.8 节、页眉页脚、其他

- `SectionSettings { pageWidth, pageHeight, orientation, marginTop/Right/Bottom/Left, pageBorder, columns, colSpace?, colWidths?, bidi?, headerDist?, footerDist?, vAlign?, docGrid?, textDirection? }`（默认 US Letter，1 in 边距，header/footer 720）
- `SectionInfo { settings, startType, firstBlockIndex, lastBlockIndex, sectPrXml, titlePg, pageNumberStart?, pageNumberFmt?, headerRefs, footerRefs }`（`readSections(parsed)` 事后从 blocks 推导）
- `HfParagraph extends ParaFormat { frameXAlign?, ptabAligns?, runs, cells?: HfTableCell[] }`
- `HfTableCell { paras: Run[][], align?, widthPct?, fill? }`
- `HfImage { dataUrl, widthPx?, heightPx?, floating?, behind?, posH?, posV?, posXPx?, posYPx?, posHRel?, posVRel?, wrap?, washout?, align? }`
- `HfPartInfo { text, hasPageNumber, paras, images? }`
- `CommentInfo { id, author, initials?, date?, text, parentId?, done?, paraId? }`
- `NoteInfo { id, text, richParas?: NoteRun[][] }`
- `SourceInfo { tag, type, author, title, year, publisher?, url? }`
- `InkInfo { anchorIndex, offsetXPx, offsetYPx, widthPx, heightPx, dataUrl, payload }`
- `DocProtection { edit, enforced, hash?, salt?, spinCount?, algorithmSid? }`、`WriteProtection { recommended?, hash?, salt?, spinCount?, algorithmSid? }`
- `ThemeFonts { major, minor, eastAsia?, majorEastAsia?, majorCs?, minorCs?, eaLang? }`、`ThemeColors { name?, dk1?, lt1?, dk2?, lt2?, accent1..6?, hlink?, folHlink? }`
- `ChartDisplay { partPath, kind: bar|line|pie|area|other, horizontal?, title?, categories, series: {name?, values:(number|null)[]}[], widthPx?, heightPx? }`
- `DiagramDisplay { widthPx, heightPx, shapes: DiagramShape[], offsetXEmu?, offsetYEmu?, floating?, canvas? }`

### 3.9 `ParsedDoc`

```
ParsedDoc {
  blocks, comments, footnotes, endnotes, sources, inks
  themeFonts?, themeColors?, protection, writeProtection, removePersonalInfo
  headerText?, headerParas?, footerParas?, headerImages?, footerImages?, watermarkText?,
  footerText?, footerHasPageNumber?, headerHasPageNumber?
  titlePg?, evenAndOddHeaders?, compatibilityMode?, autoHyphenation?, defaultTabStopTwips?
  headerFirst?, footerFirst?, headerEven?, footerEven?: HfPartInfo | null
  hfParts?: Record<rId, HfPartInfo>
  styles: Map<styleId, StyleInfo>, docDefaults?, headingStyleIds: Map<level, styleId>, listParagraphStyleId?
  numbering: Map<numId, NumberingDef>
  internal: { originalBytes, documentXml, bodyInnerStart, bodyInnerEnd }
}
ParseExtras { elements: BodyElement[], chartParts: Record<partPath, xml> }
```

---

## 4. 装载与辅助 part 解析

### 4.1 zip 装载（`zip-load.ts`, `parse.ts`）

1. **Unicode Path 字段中和**：Word 按 zip 本地头文件名解析 part，忽略 Info-ZIP Unicode Path extra field（id `0x7075`）；JSZip 会尊重该字段，导致 crc 合法但冲突的字段把 `word/document.xml` 指向别的条目（POI unicode-path 语料）。做法：扫描 EOCD → central directory，把每条记录 extra 区中 id `0x7075` 改成 `0xFFFF`。zip64（count/offset 为 0xFFFF/0xFFFFFFFF）不处理。
   *Rust 注意*：`zip` crate 是否尊重 0x7075 需要确认；最稳妥是照搬字节预处理。
2. **限额**（在解压任何 part 之前，按 central directory 声明的解压大小）：part 数 ≤ 10,000；单 part ≤ 512 MiB；总计 ≤ 1.5 GiB。超出抛错 `docx rejected: …`（`hostile-input.test.ts` 有断言）。
3. **主文档路径**：优先 `word/document.xml`；否则读 `_rels/.rels`，取 Type 以 `/officeDocument` 结尾且非 External 的 Target（去前导 `/`），存在即用（LibreOffice 语料有 `word/trial.xml`）。
4. **不是 docx**：若有 `mimetype` 且以 `application/vnd.oasis.opendocument` 开头 → 报 "OpenDocument file … not OOXML"；否则报 "not a docx: missing word/document.xml"。
5. **内容类型**：`[Content_Types].xml` 的 `Default Extension→ContentType` 与 `Override PartName→ContentType`，按 zip 缓存。
6. **基线之后新增（工作树 `f105f36`）**：`loadDocxZip` 在限额检查后多了一步 `normalizeOoxmlParts`（`src/ooxml-normalize.ts`）：若 `word/document.xml`、其 rels 或 `_rels/.rels` 含 ISO Strict URI（`purl.oclc.org/ooxml/...`）或非规范前缀，则把包内所有 `.xml`/`.rels` 改写为 transitional URI 与规范前缀（`w`/`r`/`m`/`a`/`wp`/`wps`/`mc`…）。此后所有偏移与补丁都基于改写后的文本。同期新增 `src/font-table.ts`（`parseFontTable`：`word/fontTable.xml` → `altName`/`panose1`/`family`/`pitch`），从 `index.ts` 导出。图片 MIME 判定顺序：扩展名表 `IMAGE_MIME`（png/jpg/jpeg/gif/bmp/webp/svg/emf/wmf/emz/wmz/tif/tiff）→ Override(`/`+path) → Default(ext)，且须以 `image/` 开头。

### 4.2 关系（`parseRels`）

`Map<Id, { target, type, targetMode? }>`。路径解析统一规则（多处重复实现，建议 Rust 抽成一个函数）：

```
target 以 '/' 开头  → 去掉 '/'
否则                → 'word/' + target
再去掉前缀 'word/../'（media 用 ../media/x.png 指向包根时）
```

header/footer/diagram drawing part 有各自的 `_rels/<name>.rels`，图片必须用该 part 自己的 rels 解析（`hfImages`、`extractDiagramDrawing` 的 `mediaOf` 还做了 `..` 段归一化）。

### 4.3 主题（`theme.ts`, `parseTheme`）

- `readThemeFonts`：`a:majorFont`/`a:minorFont` 段内 `<a:latin typeface>`、`<a:ea typeface>`、`<a:cs typeface>`；空串视为无。两者都无 → null。
- `eaLang`：`settings.xml` 的 `w:themeFontLang w:eastAsia`。
- `readThemeColors`：`a:clrScheme` 内 dk1/lt1/dk2/lt2/accent1-6/hlink/folHlink，取 `<a:srgbClr val>` 或 `<a:sysClr … lastClr>`，大写。
- `resolveThemeColor(themeColor, colors, tint?, shade?)`：`w:themeColor` 名 → 槽位（`dark1/text1→dk1`, `light1/background1→lt1`, `dark2/text2→dk2`, `light2/background2→lt2`, `accentN`, `hyperlink→hlink`, `followedHyperlink→folHlink`）；dk1/lt1 缺省 `000000`/`FFFFFF`；先 shade（`c*s`）再 tint（`c*t + 255*(1-t)`），因子 = hex/255。sRGB 逐通道近似，够显示用。

### 4.4 样式（`parseStyles`）

输入 `word/styles.xml`（解析失败 → 空 Map，控制台 warn）。

**docDefaults**（`w:docDefaults/w:rPrDefault/w:rPr` 与 `w:pPrDefault/w:pPr`）：`sizeHalfPoints`、`asciiFont`（themedRFonts 后 ascii ?? hAnsi）、`eastAsiaFont`（非空槽的 eastAsia；空 EA 槽 + `w:lang w:eastAsia` 时用 `EA_LANG_DEFAULT_FONT`：ko→Malgun Gothic, ja→MS Mincho, zh-cn→SimSun, zh-tw/zh-hk→PMingLiU，并标 `eaSlotEmpty`）、`bold/italic`（`w:val` 非 0/false 即 true）、`color`、`lang`（`w:lang w:val`）、`lineRawTwips/lineRule/lineSpacing`（line>0）、`spaceBeforeTwips/spaceAfterTwips`（属性存在即记录，`|| 0`）。

**每个 `w:style`**（type 限 paragraph/character/table，需有 styleId）：

- `name`：`w:name w:val`，缺省 styleId。
- `headingLevel`（仅 paragraph）：name 匹配 `/^heading\s*([1-9])$/i` 或 styleId 匹配 `/^Heading([1-9])$/`；否则 `w:pPr/w:outlineLvl`：0–8 → level+1，其他值（如 9）记入 `outlineOffIds`（阻断 basedOn 继承标题级别，例：TOCHeading basedOn Heading1）。
- `semiHidden`、`qFormat`：元素存在且 val 非 0/false。
- `numPr`（paragraph）：`w:pPr/w:numPr`：numId `"0"` → `'none'`；否则 `{numId, ilvl}`。
- `display`（非 table）：`styleDisplayOf`，见下。
- `tableDisplay`（table）：`tableStyleDisplayOf`。
- `isDefault`：`w:default="1"|"true"`。随后按 ECMA-376 §17.7.4.17 归一：每个 type 最后一个声明 default 的胜出，没有声明的 type 取该 type 第一个样式。

**`styleDisplayOf`**：
rPr → `sizeHalfPoints`、`color`（colorFrom）、`bold/italic`（三态 onOff）、`boldCs/italicCs/sizeCsHalfPoints`、`rtl`、`underline`（`w:u w:val` 存在且非 none）、`strike`、`themedRFonts` → `font`(ea??ascii??hAnsi) / `fontAscii` / `csFont` / `eaSlotEmpty`、`charSpacingTwips`、`caps`（caps 优先于 smallCaps）、`vanish`（排除 `w:specVanish`）。
pPr → `lineRule/lineRawTwips/lineSpacing`、`spaceBefore/AfterTwips`、`keepNext`、`keepLines`、`pageBreakBefore`（三态）、`contextualSpacing`、`autoSpace`、`align`（center/right/left/justify 直取；both/distribute → justify）、`shadingFill`、`tabStops`、`indentLeft/Right/FirstLineTwips`（hanging 存为负 firstLine）。

**`tableStyleDisplayOf`**：`w:tcPr/w:shd` → `fill`；样式级 `w:rPr` → `wholeTable {color,bold,italic,sizeHalfPoints}`；`w:tblStylePr` 按 type：firstRow/firstCol/lastCol/lastRow → `{fill,bold,color,sizeHalfPoints}`，band1Horz/band2Horz → `band1Fill/band2Fill`；`w:tblPr/w:tblBorders`（含 inside）、`w:tblCellMar`；`w:pPr/w:jc` → `paraJc`；`w:pPr/w:spacing` → `paraSpacing`。

**basedOn 链解析**（带环检测）：子样式 `display = {...parent.display, ...own}`；`tableDisplay` 深合并（wholeTable/firstRow/firstCol/lastCol/lastRow/paraSpacing 逐字段）；`headingLevel` 继承（除非在 outlineOffIds）；`numPr` 继承。

**linked styles**（`w:link`）：段落样式与字符样式互相回填 `RUN_KEYS`（sizeHalfPoints/color/bold/italic/boldCs/italicCs/sizeCsHalfPoints/rtl/underline/strike/font/fontAscii/csFont/caps）中自身缺失的项；字符侧标 `linkedCharShell`。

**`themedRFonts(attrs, fonts)`**（ECMA-376 §17.3.2.26：主题属性覆盖同槽字面值）：
- `asciiTheme/hAnsiTheme`: major* → fonts.major，minor* → fonts.minor
- `eastAsiaTheme`: majorEastAsia → fonts.majorEastAsia，minorEastAsia → fonts.eastAsia
- `cstheme`: majorBidi → majorCs，minorBidi → minorCs
- 解析不到 → 退回字面值 `w:ascii/w:hAnsi/w:eastAsia/w:cs`
- **例外**：EA 主题槽为空（有 eastAsiaTheme 引用、主题存在但槽无 typeface）→ 不用字面值，改用 `emptyEaSlotFont`：eaLang 首段 `ja` → Yu Gothic(major)/Yu Mincho(minor)，`ko` → Malgun Gothic，其他 → DengXian；并返回 `eaSlotEmpty: true`。

派生：`headingStyleIds`（每级第一个有该级别的样式）、`listParagraphStyleId`（styleId 忽略大小写等于 `listparagraph`）。

### 4.5 编号（`parseNumbering`）

- `w:abstractNum`：每个 `w:lvl` → `parseNumberingLevel`；记录 `w:numStyleLink`（本 abstractNum 只是对编号样式的引用）与 `w:styleLink`（本 abstractNum 是某编号样式的定义）。
- numStyleLink 间接：沿 `numStyleLink → styleLink` 链追到终点（防环），终点 levels 作底、本身 levels 覆盖。
- `w:num`：`levels = {...abs.levels}`；`w:lvlOverride`：`w:startOverride` → `startOverrides[ilvl]`；内嵌 `w:lvl` → 覆盖该级定义。
- `formats[numId] = levels[0].numFmt === 'bullet' ? 'bullet' : 'ordered'`（粗分类；精确分类用 `listKindOf` 按 ilvl 看）。

`parseNumberingLevel`：`start`（缺省 **0**，ECMA：无 w:start 从 0 起，Word 显示 "0."）、`numFmt`（`numFmtOfLevel`：直接 `w:numFmt`；否则 `mc:AlternateContent` 的 Choice `w:numFmt w:val="custom" w:format="α, β, γ, ..."` 且 `customEnumItems` 合法 → `custom`+`customFormat`；Choice 非 custom 直取；否则 Fallback）、`lvlText`（解数字实体）、`suff`、`w:pPr/w:ind` → `indentLeft`(>0)/`hanging`(>0)/`firstLine`(无 hanging 且 >0)、`w:rPr/w:sz` → `szHalfPoints`、`w:rPr/w:rFonts` ascii??hAnsi??eastAsia → `font`。

段落侧：
- `listRefOf(pPr, styleId)`：直接 `w:numPr` 的 numId/ilvl；numId `"0"` → 无编号；无直接 numId 时用样式的 numPr（`'none'` 视为无）；ilvl 缺省用样式的 ilvl，再缺省 0。
- `listKindOf(numId, ilvl)`：`numbering[numId].levels[ilvl].numFmt === 'bullet'` → bullet，其他 → ordered；无该级定义时退 `formats[numId]`，再缺省 bullet。

### 4.6 settings.xml 杂项

| 字段 | 规则 |
| --- | --- |
| `protection` | `<w:documentProtection …/>`：`w:edit` 缺失或 `none` → null；`enforced = enforcement ∈ {1,true}`；hash/salt/cryptSpinCount/cryptAlgorithmSid |
| `writeProtection` | `<w:writeProtection …/>`：`recommended` (1/true/on)、hash/salt/spin/sid；两者都无 → null |
| `removePersonalInfo` | 前缀无关：先收集绑定到 `…/wordprocessingml/2006/main` 或 `http://purl.oclc.org/ooxml/wordprocessingml/main` 的所有前缀（含默认命名空间），再匹配 `<prefix:removePersonalInformation …/>` 或空元素形式；`val="0|false"` 为 false（引号可单可双） |
| `compatibilityMode` | `<w:compatSetting w:name="compatibilityMode" w:val="N">`，缺省 0 |
| `autoHyphenation` | `xmlFlagOn(xml, 'w:autoHyphenation')` |
| `defaultTabStopTwips` | `<w:defaultTabStop w:val>`，缺省不设（Word 720） |
| `evenAndOddHeaders` | `xmlFlagOn` |
| `themeFontLang` | `w:eastAsia` 属性 → `ThemeFonts.eaLang` |

`xmlFlagOn(xml, tag)`：任一 `<tag …>` 出现且 `w:val` 不是 `0|false|off` → true（自闭合或成对都匹配）。

### 4.7 批注（`parseComments`）

- `word/comments.xml`：每个 `w:comment` → `{id, author, initials, date, text(段落 textOf 以 \n 连接), paraId(最后一段的 w14:paraId)}`。
- `word/commentsExtended.xml`：`<w15:commentEx w15:paraId w15:paraIdParent w15:done>`，按 paraId 关联 → `done`、`parentId`（父 paraId → 父 comment 的 id）。
- `referenceOnlyComments`：在 `document.xml` 中没有任何 `w:commentRangeStart` 的批注 id（LibreOffice 风格，只有 `w:commentReference`）→ run 提取时挂到最近的 run。

### 4.8 脚注/尾注（`notes.ts`）

- part：`word/footnotes.xml` / `word/endnotes.xml`，根 `w:footnotes`/`w:endnotes`，条目 `w:footnote`/`w:endnote`。
- 带 `w:type` 属性的条目（separator/continuationSeparator）是结构性的，跳过。
- `text`：每个 `w:p` 的 `w:t` 拼接（实体解码），段落以 `\n` 连接；首段去前导空白（自引用标记后的间隔）。
- `richParas`：每段 run 列表 `{text, bold, italic, underline(w:u val 非 none), strike, color(6 hex), sizeHalfPoints, caps}`，跳过含 `w:footnoteRef/w:endnoteRef` 的 run；任一 run 有格式才保留。
- `noteNumbers`：按 part 顺序编号 1..n，键 `footnote:<id>` / `endnote:<id>`，供 `Run.noteRef.text`。

### 4.9 参考文献（`sources.ts`）

在 `customXml/item\d+.xml` 中找同时含 `Sources` 与命名空间 `http://schemas.openxmlformats.org/officeDocument/2006/bibliography` 的 part；每个 `<b:Source>` 取 Tag/SourceType/Author(Corporate 或 Last, First)/Title/Year/Publisher|JournalName|InternetSiteTitle/URL。

---

## 5. body 扫描与节几何

### 5.1 `scanBody(documentXml)`

- 找 `<w:body …>` 开标签（容忍属性），从其后扫描 `TAG_RE = /<\/?(?:[^<>"']|"[^"]*"|'[^']*')*>/g`（容忍属性值中的 `>`）。
- 跳过 `<!--`、`<![`、`<?`。
- 深度计数：depth 0 处的开标签记 `currentStart/currentName`；对应闭标签回到 0 时 push `{name, start, end}`；depth 0 的自闭合标签直接 push。
- 遇到 depth 0 的 `</w:body>`：**不结束**，继续找下一个 `<w:body>`（Word 容忍多个 sibling body，POI MultipleBodyBug.docx，内容按顺序拼接；保存时都拼回第一个 body）。其他 depth 0 闭标签 → 抛错。
- 返回 `elements`、`innerStart`（首元素 start）、`innerEnd`（末元素 end）；无元素时两者都是 body 开标签之后。

### 5.2 `sectionAt(docOffset)`

`sectSlices` = 全文所有 `<w:sectPr …/>` 或 `<w:sectPr>…</w:sectPr>` 匹配（非贪婪）。给定偏移，取第一个 `end > offset` 的 sectPr（节以其 sectPr 结束，管辖之前的内容）；没有则用最后一个；缓存 `sectionSettingsFromXml` 结果。用于页面/边距锚定绘图的绝对定位。

### 5.3 `sectionSettingsFromXml(sectPrXml)`

`w:pgSz`(w/h/orient)、`w:pgMar`(top/right/bottom/left/header/footer)、`w:vAlign`(center/both/bottom)、`pageBorder`（`w:pgBorders` 至少一边 `w:val` 非 none/nil）、`w:cols`(num, space 缺省 720, `w:col w:w` ≥2 个 → `colWidths`)、`w:bidi`、`w:docGrid`(type/linePitch/charSpace)、`w:textDirection`(非 lrTb)。

---

## 6. Block 构建

### 6.1 顶层循环中的 sdt 拆分

`w:sdt` 元素先 `splitSdtParts(xml)`：找 `<w:sdtContent>` 到 **最后一个** `</w:sdtContent>` 的区间；用标签扫描找 depth 0 的子元素，**`w:sdt`/`w:sdtContent` 标签跳过不计深度**（嵌套内容控件透明，Word 封面构件每个字段一个 sdt）；只保留 `w:p`/`w:tbl`；≥2 个才拆。第 k 个子块的切片：首块从 sdt 开头起、末块到 sdt 结尾止、中间块到下一子块开头止；这样各块的 `originalXml` 拼起来正好等于整个 sdt。每块 `buildBlock` 时传入子元素本身的 XML，然后覆盖 `originalXml = part 切片`、`sdtShell = { alias, tag, controlType, openXml: part 开头到子元素开头, closeXml: 子元素结尾到 part 结尾, group }`；`elements` 也随之拆成多条，保持 `docxIndex == elements 下标`。

`sdtMeta`：`w:sdtPr` 内 `w:alias w:val`、`w:tag w:val`；controlType：`w:date` → date，`w:dropDownList|w:comboBox` → dropdown，`w:checkbox` → checkbox，`w:text|w:richText` → text，缺省 text。

### 6.2 `buildBlock(el, index, xml, ctx)` 决策树

严格按以下顺序（顺序即优先级，任何一步命中即返回）：

1. **`w:ins` / `w:del` 顶层包裹**：`splitXmlChildren` 找第一个 `w:p`/`w:tbl` 子元素，递归 `buildBlock` 得 inner；`inner.originalXml = 整个 wrapper`；`inner.blockRevision = {kind, author, date?, id?}`（属性从 wrapper 解析，失败则空）。无子块则落到后面的通用 passthrough。
2. **`w:sectPr`** → `passthrough, label 'Section properties', hidden: true`。
3. **`w:tbl`** → `type 'table'`, `tableSummary(xml)`（`label 'Table R×C'`，R = `<w:tr` 数，C = 首行 `<w:tc` 数；`previewText` = plainText 前 120 字）, `table: extractTable(xml)`。
4. **`w:sdt`**（单子块或无子块）：
   - `sdtTableXml`：sdtContent 内首个 `<w:tbl` 且之前没有 `<w:p` → 按表格处理（`originalXml` 仍是整个 sdt）。
   - `parseSdtBlock`：sdtContent 内首个 `<w:p …>` 平衡切片 → 递归 `buildBlock`，`originalXml = 整个 sdt`，`sdtShell = {meta, openXml, closeXml}`，`label = alias || tag || 'Content control'`。
   - 否则 passthrough `'Content control'`；若 plainText 为空且无 `<w:drawing`/`<w:pict` → `invisibleMarker`；否则 `previewText`。
5. **`INVISIBLE_BODY_MARKERS`**（bookmarkStart/End, commentRangeStart/End, proofErr, permStart/End, moveFrom/ToRangeStart/End, customXmlIns/DelRangeStart/End）→ passthrough `invisibleMarker`。
6. **`w:br`**：含 `w:type="page"` → passthrough `label 'Page break', fieldDisplay {kind:'pageBreak'}`；否则 invisibleMarker。
7. **其他非 `w:p`** → passthrough `label el.name, previewText ''`。
8. **`w:p`**：先算 `detect = stripInkRuns(去掉所有 <mc:Fallback>…</mc:Fallback> 的 xml)`（Word 给每个 DrawingML 形状配 VML 孪生，直接匹配原文会把所有装饰段落误判为嵌入对象）。
   1. `detect` 含 `<w:sectPr` 且 `plainText(detect)` 为空 → passthrough `'Section break paragraph'`。（有文字的分节段落按普通段落走，sectPr 随 rawPPr 保留。）
   2. **字段**：`fieldDetect = stripTextboxes(detect)`（文本框内的字段不算）；`hasFields = fldChar | fldSimple | instrText`。
      - hasFields 且含 `<w:drawing` 且非 chart/`r:dm=`/`<dgm:` 且去文本框后无文字 → `extractImage` 成功则 `type 'image'`（INCLUDEPICTURE 等）。
      - hasFields 且含 `<w:object` 且 `onlyOleFields(fieldDetect)`（所有 instrText 都是 EMBED/LINK，且无 fldSimple）→ passthrough `'Embedded object'` + `oleDisplay`。
      - hasFields 且 `!onlyXeFields(detect)` → passthrough，`label = fieldLabel(xml)`，`previewText = plainText(xml)`，`fieldDisplay = fieldDisplayOf(xml)`，`styleId`（有 pStyle 时）。
        `onlyXeFields`：无 fldSimple；每条 instrText 是 `XE`、`REF`、`SIMPLE_INLINE_FIELD_RE`（DATE|TIME|CREATEDATE|SAVEDATE|NUMPAGES|FILENAME|AUTHOR|PAGE）、可转换 HYPERLINK（仅 `HYPERLINK "url"` 可带 `\o "tip"`，其他开关不行）、或 FORMCHECKBOX（且 `<w:checkBox` 定义数 ≥ FORMCHECKBOX 数）。这些字段会被 `extractRuns` 折叠成可编辑 run。
   3. `w:pStyle w:val="TOC ?[1-9]"`（Word `TOC1`，Pages `TOC 1`）→ passthrough `'TOC entry'` + `fieldDisplayOf` + styleId。
   4. `detect` 含 `w:delInstrText | w:cellIns | w:cellDel` → passthrough `'Revised paragraph'`。
   5. **公式**：含 `<m:oMath` 且（含 `<m:oMathPara` 或 plainText 为空）→ passthrough `'Equation'`，`formulaDisplay = { tokens(m:t 文本), omml(所有 m:oMath 片段拼接), mathml(仅纯公式段落: ommlToMathML), latex(ommlToLatex, 可能 null) }`，`previewText = tokens.join('')`。含普通文字的内联公式段落落到 `buildTextParagraph`，每个 `m:oMath` 成为原子 run。
   6. **`w:object` / `w:pict`**（VML/OLE）：
      - 非 object 且（含 `<w:txbxContent` 或 VML WordArt `v:textpath string=`）→ `extractTextboxes(detect)`；`strayText = plainText(stripTextboxes(detect))`；若 `strayText` 非空且含 `<v:imagedata` 则不走文本框（保图片走 run-image 路径）；否则若有 boxes：strayText 非空时追加 `paragraphStrayBox`（去掉 pict 后的段落作只读行）；`imageAlign` 取宿主段落（去文本框后）的 `w:jc`；→ passthrough `'Text box'` + `textboxes` + `hostPageBreak` 时 `fieldDisplay pageBreak`。
      - 非 object 且含 `<v:imagedata`：去文本框后有文字 → `resolveBlipMedia` 后 `buildTextParagraph(withImages=true)`；否则用 `r:id` 取图 → `type 'image'` + `vmlImageMeta`。
      - 非 object、无文字、`isInvisibleVmlPict` → passthrough invisibleMarker（只有 shapetype、`visibility:hidden`、或 `stroked=f` 的白色矩形）。
      - VML `<v:rect o:hr="t">` 且无文字 → passthrough `'Drawing object'` decorative，`ruleColorHex` 取 fillcolor，`ruleThicknessPx` 取 `height:Npt`（宽 0 表示铺满不设）。
      - 含 object 且去文本框后有文字：`resolveBlipMedia`；若每个 `<w:object>` 的 `v:imagedata r:id` 都在 mediaByRid → `buildTextParagraph(withImages=true)`。
      - 否则 passthrough `'Embedded object'` + `oleDisplay`。
   7. **`w:drawing`**：
      - 图表/SmartArt（`<c:chart` | `r:dm=` | `<dgm:`）：chartex（含 `/drawing/2014/chartex"`）先试 `extractImage(xml)` 的预渲染 fallback 图 → `type 'image'`；否则 `chartDisplay = extractChart`（chart）或 `diagramText = extractDiagramText` + `diagramDisplay = extractDiagramDrawing`（SmartArt）；多绘图段落时把非 diagram 的兄弟形状/图片 `extractTextboxes(shapes+pictures, section)` 放到 `textboxes`，diagram 自己的 anchor 偏移写到 `diagramDisplay.offsetX/YEmu, floating`。→ passthrough `'Chart'`/`'SmartArt'`。
      - `<lc:lockedCanvas` → `extractLockedCanvas` → passthrough `'Drawing object'` + `diagramDisplay{canvas:true}`（水平 anchor 偏移；`noWrap` 时 floating）。
      - `extractImage(detect)` 成功：若形状内还有非空 txbxContent（`boxed`）→ 跳到文本框路径；否则 `multiPic`（>1 个 `<a:blip`）或有文字：有文字 或（多图且无 `<wp:anchor`）→ `buildTextParagraph(withImages=true)`；否则 `type 'image'` + `imageMeta(detect)`。
      - 含 `<a:blipFill` → `resolveBlipMedia`。
      - `textboxes = extractTextboxes(detect, {shapes:true, section})`；`strayText` 非空且所有 box 无文字 → `buildTextParagraph`（装饰形状留在不再生成的 run 里，未编辑时字节不变）。
      - boxes 非空 → passthrough `'Text box'` + `textboxes` + `strayRuns/strayStyleId`（`strayParaRuns`）+ `hostPageBreak` + `imageMeta(detect)`。
      - 含 `<a:blip` 或 `<pic:pic`（媒体解析失败）→ passthrough `'Image'`, `brokenImage`, `previewText = docPr descr ?? name`, `imageMeta`。
      - `isInvisibleEmptyShape`（所有 `wps:wsp` 都 noFill + ln noFill + 无 effect，且无 blip 无文字）→ invisibleMarker。
      - 否则 passthrough `'Drawing object'`，`decorative = isThinRule`（`wp:extent cy` 在 (0, 130000] EMU），装饰时 `ruleDisplayOf`（`a:ln` 的 srgbClr / w → 颜色/粗细 px，`extent cx` → 宽 px）。
   8. **`buildTextParagraph(base, xml, ctx)`**。

### 6.3 `buildTextParagraph`

1. `xmlParser.parse(xml)` 失败 → passthrough `'Paragraph'` + previewText（病态嵌套）。无 `w:p` 根 → `'Unknown paragraph'`。
2. `styleId = pPr/w:pStyle`；样式 `display.vanish === true` 且 `staysVanished(xml)`（没有 `w:vanish w:val=0/false/off`，且不含 drawing/pict/object/sectPr/bookmarkStart/commentRange*/numPr）→ passthrough `'Hidden paragraph'` invisibleMarker。
3. `format = extractParaFormat(pPr)`；若 format 无 autoSpace 且样式 `display.autoSpace === false` → `format.autoSpace = false`。
4. `rawPPr = rawPPrOf(xml)`：`w:pPr` 必须是 `<w:p>` 开标签后的第一个子元素；深度感知（`w:pPrChange` 内还有 `w:pPr`）；自闭合 `<w:pPr/>` 直接返回。
5. `mathXml = stripTextboxes(去 Fallback 的 xml)`；`runs = extractRuns(pNode, ctx, ommlFragmentsOf(mathXml), rubyFragmentsOf(mathXml), withImages)`。片段数组与树遍历中遇到的 `m:oMath`/`w:ruby` 顺序一一对应（所以要先剥掉 Fallback 与文本框副本）。
6. runs 为空 → `format.emptyRunSizeHalfPoints = emptyParaSizeHalfPoints`（段落标记 `pPr/w:rPr/w:sz`，否则最后一个 run 的 `w:sz`）、`format.emptyRunFontFamily = emptyParaMarkFont`（同源 `w:rFonts` ascii??hAnsi??eastAsia）。
7. **allowOverlap=0 碰撞**（tdf#134114）：环绕型（非 front/behind）的 run 图片 >1 个时，对 `noOverlap` 的图片检查与其他非 noOverlap 图片的纵向区间重叠（`offsetYEmu/EMU_PER_PX` 与 `heightPx`），命中则改为 `wrap 'front'`, `offsetXEmu 0`, `offsetYEmu = (碰撞者高度+2)*EMU_PER_PX`。
8. `bookmarkNamesOf(stripTextboxes(xml))`：`w:bookmarkStart w:name`，`_` 前缀 → hiddenBookmarks，其余 → bookmarks，去重。
9. `crossParaCommentMarkers(stripTextboxes(xml))`：只有 start 没 end 的 id → `commentStarts`；只有 end → `commentEnds`。
10. `moveRevision`：含 `<w:moveFrom` → 'from'，否则含 `<w:moveTo` → 'to'。
11. `pPrChangeInfo`：`pPr/w:pPrChange` 的 author/date/id；其内 `w:pPr` → `old = extractParaFormat(oldPPr) + styleId + (numPr → type 'docListItem', numId, ilvl, kind) | (headingLevelOf → 'docHeading', level) | (styleId → 'docParagraph')`。
12. `paraMarkDel`：`pPr/w:rPr/w:del` 的 author/date/id。
13. 分类：`listRefOf` 命中 → `listItem {kind: listKindOf}`；否则 `headingLevelOf(pPr, styleId)` 命中 → `heading {level}`；否则 `paragraph`。三者都带 `styleId, format, rawPPr, bookmarks, hiddenBookmarks, commentStarts, commentEnds, runs, moveRevision?, pPrChangeInfo?, paraMarkDel?`。

`headingLevelOf`：直接 `pPr/w:outlineLvl`（0–8 → +1；9 → 非标题，**不再看样式**）；否则样式 `headingLevel`；否则 styleId 匹配 `/^Heading([1-9])$/i`（文档未定义的内建样式）。

### 6.4 `extractParaFormat(pPr)`

| 来源 | 规则 |
| --- | --- |
| `w:bidi` | boolProp → `bidi` |
| `w:jc` | `left/start→left, center, right/end→right, both→justify, distribute`；**bidi 段落中 left/right 互换**（Word 把 jc 当逻辑值，模型存视觉值，写回再换回） |
| `w:spacing` | `line>0`：`lineRawTwips=line`；rule auto → `lineSpacing=round(line/240,2), lineRule='auto'`；否则 `lineRule=rule`。`line==0 && rule==atLeast`：`lineRule='atLeast', lineRawTwips=0`（布局无效但退出 docGrid 吸附）。`before/after`：属性存在、≥0、且对应 `*Autospacing` 不为 1/true 时记录（Autospacing 时丢掉字面值让样式级联；显式 0 保留） |
| `w:ind` | `left ?? start`（非 0 记录，允许负）、`right ?? end`、`hanging>0 → indentFirstLine=-hanging`，否则 `firstLine>0 → indentFirstLine` |
| `w:pageBreakBefore` | 三态 `onOffOf` |
| `w:keepNext`, `w:keepLines`, `w:contextualSpacing` | boolProp → true |
| `w:snapToGrid`, `w:widowControl` | 仅 `w:val` 为 0/false 时记 false |
| `w:autoSpaceDE/DN` | 两者都显式关 → false；任一显式开 → true；否则不设 |
| `w:shd w:fill` | 非 auto → `shadingFill` |
| `w:pBdr`（可重复） | 每边取最后一个容器里的元素；`w:val` none/nil 视为无边；`borders += 't'|'b'|'l'|'r'`；每边 `color`(非 auto)、`szPt = sz/8` → `borderLines` |
| `w:tabs/w:tab` | `pos` 必须是整数；`val` 不在合法集合 → left；`leader` 非 none 且合法才记 |
| `w:framePr w:dropCap` | drop/margin → `dropCap {type, lines(缺省 3)}` |

空对象 → undefined。

### 6.5 `extractRuns(pNode, ctx, mathFragments, rubyFragments, withImages)`

状态：

- `paraRtl`：段落样式 `display.rtl`（run 无自身 rtl 时继承）。
- 批注：先收集本段内所有 `commentRangeStart/End` id，`complete = starts ∩ ends`；遍历时只对 complete 的 id 维护 `activeComments`；`pendingRefIds` 处理 reference-only 批注在任何 run 之前出现的情况。
- 修订上下文 `RevCtx {ins?, del?}`：`w:ins`→ins，`w:del`→del，`w:moveFrom`→del，`w:moveTo`→ins（复用接受/拒绝机制）。
- 字段状态机：`fieldDepth`、`fieldInstr`、`fieldSeparated`、`fieldCached`、`fieldCachedRuns`、`fieldBeginRun`。

遍历 `walk(nodes, link, rev)`：

| 节点 | 处理 |
| --- | --- |
| `w:commentRangeStart/End` | complete 才加入/移出 `activeComments` |
| `w:ins`/`w:del`/`w:moveFrom`/`w:moveTo` | 构造 RevisionInfo 后递归 |
| `w:r` | `handleRun` |
| `m:oMath` | 取 `mathFragments[mathIndex++]`，push `{text: mathTokens(omml).join(''), math:{omml}}` |
| `w:hyperlink` | `href = rels[r:id].target ?? '#'+w:anchor ?? ''`，`tooltip`；带 link 递归 |
| `w:smartTag`/`w:sdt`/`w:sdtContent` | 透明递归 |
| `w:br`（run 外） | push `{text: BREAK_CHAR[type] ?? '\n'}` |

`handleRun(node, link, rev)`：

1. 含 `w:fldChar`：
   - `begin`：depth++；depth==1 时重置字段状态并记 `fieldBeginRun`。
   - `separate`：depth==1 → `fieldSeparated=true`。
   - `end`：depth--；回到 0 时按 `fieldInstr` 折叠：
     - `XE "term"` / `XE term` → `{text:'', xeTerm}`
     - `REF name` → `{text: fieldCached || name, refField: name, refInstr: fieldInstr}`
     - 可转换 HYPERLINK → 每个 cached run 复制并加 `link {href, tooltip?}`；无 cached → `{text: href, link}`
     - `FORMCHECKBOX` → `checkboxStateOf(beginRun)`（`w:ffData/w:checkBox` 的 `w:checked ?? w:default`；无 val 视为 true）→ `{text: '☒'|'☐', instrField:'FORMCHECKBOX', fldBeginXml: serializeXNode(beginRun)}`
     - `SIMPLE_INLINE_FIELD_RE` → `{text: fieldCached || ' ', instrField: instr.trim()}`
     - 其他：丢弃（这些段落本就走了 passthrough）
   - return。
2. `fieldDepth > 0`：含 `w:ruby` 则 `rubyIndex++`（保持对齐）；`w:instrText` → 追加到 `fieldInstr`；`fieldSeparated && depth==1` → `buildRun` 结果追加到 `fieldCached/fieldCachedRuns`；return。
3. 含 `w:ruby`：`xml = rubyFragments[rubyIndex++]`；`base = rubyBase 内 w:r/w:t 文本`，`rt` 同理；有 xml → `{text: base, ruby:{rt, xml}}`，否则 `{text: base}`；return。
4. 含 `w:footnoteReference`/`w:endnoteReference`：`{text: String(noteNumbers[kind:id] ?? '*'), noteRef:{kind,id}}`；return。
5. 含 `w:commentReference` 且 id ∈ referenceOnlyComments：挂到上一个 run 的 commentIds，无上一个则进 `pendingRefIds`。
6. `buildRun(node, link, theme, themeFonts, withImages ? mediaByRid : undefined, styles, paraRtl)` → push。

`pushRun`：设置 `commentIds`（activeComments 排序 + pending）、`ins/del`。

最后 `mergeRuns`：相邻 run `sameStyle` 时合并文本。`sameStyle` 排除任何原子 run（noteRef/xeTerm/refField/instrField/math/ruby/image），其余比较 `rawRPr`、styleId、cs、bold、italic、underline、strike、color、sizeHalfPoints、font、fontAscii、csFont、highlight、vertAlign、link.href、link.rId、commentIds、ins、del（author/date/id）。

### 6.6 `buildRun(rNode, …)`

**文本**：遍历子节点：`w:t`/`w:delText` → `decodeNumericCharRefs(textOf)`，无 `xml:space="preserve"` 时去掉首尾 `[ \t\r\n]`（Word 对美化缩进的 XML 的行为）；`w:tab`/`w:ptab` → `\t`；`w:br` → BREAK_CHAR；`w:cr` → `\n`；`w:noBreakHyphen` → `‑`；`w:sym` → `decodeSymbolChar(font, hex code) ?? fromCodePoint((code & 0xff) + 0xF000)`。

**图片**（仅 `mediaByRid` 传入时）：`w:drawing` → `serializeXNode` 得 `xml`，`a:blip r:embed|r:link` 查 mediaByRid → `image {dataUrl, xml, widthPx/heightPx(extent), border(picBorderOf)}`；若含 `<wp:anchor` → `imageMeta` 的 `wrap/offsetX/offsetY`、`lineCenterV`（`positionV relativeFrom="line"` + `align center`，tdf#162551）、`noOverlap`。无 drawing 时 `w:pict ?? w:object` → `v:imagedata r:id` → `image`，尺寸取 `v:shape style` 的 width/height pt，缺省 `w:object dxaOrig/dyaOrig` twips。

文本为空且无图 → null。

**rPr**：
- `rawRPr = serializeXNode(rPr)`
- `styleId = w:rStyle`（非 Hyperlink）
- `cs`：`onOffOf(w:rtl) ?? 字符样式 display.rtl ?? paraRtl`（Word for Mac 实测：rtl run 只读 bCs/iCs/szCs，非 rtl 只读 b/i/sz，无交叉回退；脚本内容与段落 bidi 不参与）；无 rPr 但 paraRtl 也标 cs。
- `bold = onOffOf(cs ? bCs : b)`，`italic` 同理，`sizeHalfPoints = (cs ? szCs : sz)`
- `underline`：`underlineProp`（`w:u` 有 `w:val` 且非 none）；`w:u w:val="none"` → false
- `strike` 三态
- `color = colorFrom(rPr) ?? w14TextFillHex(rPr)`（`w:color w:themeColor` 优先于 w:val；`w14:textFill` solid 或 gradFill 各 stop 平均）
- `themedRFonts` → `font = ea ?? ascii ?? hAnsi`，`eaSlotEmpty`，`fontAscii = ascii ?? hAnsi`，`fontCs = 字面 w:cs`，`csFont = 解析后 cs`
- `rtl`、`charSpacingTwips`(w:spacing)、`caps`(caps > smallCaps)、`charScalePct`(w:w ≠100)、`highlight`(非 none)、`shading`(w:shd fill 非 auto)、`vertAlign`、`em`(非 none)
- `rPrChange`：author/date/id + `old`（在同一 rtl 选择下解码 bold/italic/underline/strike/color/size/font/fontAscii/charSpacing/charScale/highlight/vertAlign/styleId）

**符号字体**：`run.font` 是符号字体且 `decodeSymbolText` 全部映射成功 → 替换文本、删除 font/fontAscii/fontCs、从 `rawRPr` 去掉 `<w:rFonts …/>`。

### 6.7 `colorFrom(container, theme)`

`w:color`：有 `w:themeColor` 且有主题 → `resolveThemeColor(themeColor, theme, themeTint, themeShade)`（成功即返回，主题值胜过陈旧的 w:val）；否则 `w:val` 非 auto → `stripHash(val)`。

---

## 7. 表格提取

### 7.1 `extractTable(xml)`

整体 try/catch（恶意深度 → undefined，块降级为只读表格）。`deepXmlParser.parse` → `extractTableModel(tbl, ctx, depth=1)` → `attachRawTablePr(xml, rows, rawTrPrs)`。

### 7.2 `extractTableModel(tbl, ctx, depth)`

1. **列宽**：`w:tblGrid/w:gridCol w:w` → `colWidthsPct`（按总和归一）、`colWidthsTwips`（全部 >0 时）。再用 `tcwColumnWidths`（各行未跨列单元格的 `w:tcW`(dxa，重复取最后一个) 每列取最大；必须每列都有值）校正：若 grid 缺失/列数不同/任一列相差 >2 个百分点/（fixed 布局且总和差 > 列数）→ 以 tcW 为准（生成器常留下过时均分的 tblGrid）。
2. `w:tblW type=pct` → `widthPct = w/50`（容忍 `"NN%"`），(0,100] 有效。
3. `cellMar = cellMarginsOf(w:tblCellMar)`（top/left/bottom/right/start→left/end→right，type 缺省或 dxa，首个有效值）。
4. `tblBorders = mergedBorderLinesOf(tblPr, 'w:tblBorders', withInside)`（重复容器按边合并，后者胜）；`borderLinesOf`：`w:val` 必须存在；`szEighths`、`color`。
5. `align`：`w:jc` center → center，right/end → right。`indentTwips`：`w:tblInd`（dxa）。
6. `floatSide`：有 `w:tblpPr` 时，`tblpXSpec` right/outside 或（无 xSpec 且 `tblpX > 4680`）→ right，否则 left。
7. **行**：`childrenThroughSdt(tbl, 'w:tr')`，每行 `childrenThroughSdt(tr, 'w:tc')` → `extractCell`；`hMerge === 'continue'` 的单元格折叠进左侧单元格的 `colSpan`。行非空才计入。`w:trPr/w:trHeight`：`w:val > 0` 时 `min(val, 31680)`，`hRule` exact → 'exact' 否则 'atLeast'；`rowRevisionOf(trPr)`（`w:ins`/`w:del`）。
8. `applyTableStyleDisplay(rows, tblPr)`：按 `w:tblStyle` 的 `tableDisplay` 与 `w:tblLook`（属性 `w:firstRow/lastRow/firstColumn/lastColumn/noHBand` 优先，否则 `w:val` 位：0x20 firstRow, 0x40 lastRow, 0x80 firstColumn, 0x100 lastColumn, 0x200 noHBand；缺省 firstRow/firstColumn 开、其他关）给单元格补 `fill/bold/color`。优先级：firstRow > lastRow > firstCol > lastCol > 条带（band 行号从 firstRow 之后起算，偶数 band1，奇数 band2）> 整表 fill；只在单元格自身未设时补。
9. `borders/cellMarTwips` 缺省回退到表格样式的 `tableDisplay.borders/cellMarTwips`。
10. `autoLayout`：非 fixed、无 tblW 或 type auto 或（dxa 且 w ≤ 0）、且无 widthPct。
11. `tblStyleId`、`bidiVisual`、`rowHeightsTwips/rowHeightRules`（有任一非 null 时）、`rowRevisions`。

### 7.3 `extractCell(tc, ctx, depth)`

- `tcPr`：`gridSpan>1 → colSpan`；`w:vMerge`（val restart → restart，否则 continue）；`w:shd w:fill` 非 auto → fill；`w:vAlign`；`w:tcMar`；`w:textDirection`（tbRl/tbRlV → tbRl；btLr/btLrV → btLr）；`w:hMerge`；`mergedBorderLinesOf(tcPr,'w:tcBorders',false)`；`w:cellIns/w:cellDel` → cellRevision。
- 子块 `childrenThroughSdt(tc, ['w:p','w:tbl'])`：
  - `w:tbl`：`depth >= 8` → `flattenedTableModel`（迭代收集所有段落文本成 1×1 autoLayout 表）；否则 `extractTableModel(depth+1)`；记 `nestedTableAnchors.push(当前段落数)`。
  - `w:p`：若含 `w:drawing`/`w:pict` 后代且（含 `<wp:anchor` 或 `<w:pict`）：序列化去 Fallback 后 `extractTextboxes(pXml, {shapes:true})` → `anchoredBoxes`；然后从段落 XML 中删除锚定且非 `pic:pic` 的顶层 drawing 与含 txbxContent 的 pict，重新解析成段落节点（失败保留原节点）。
  - `cell.paras.push(textOf(p))`；`richParas.push({...extractParaFormat(pPr), styleId?, emptyRunSizeHalfPoints?, emptyRunFontFamily?, list?(listRefOf+listKindOf), runs: extractRuns(p, ctx, [], [], true)})`。
  - 聚合：有文字的段落的 `w:jc` 集合大小为 1 且 ∈ {center,right,left,justify} → `cell.align`；所有 run 都 `w:b` → `cell.bold`；有文字的 run 颜色只有一种（非 none）→ `cell.color`。
- `nestedTables/nestedTableAnchors`（锚点 clamp 到段落数）。

### 7.4 `attachRawTablePr(xml, rows, rawTrPrs)`

用 `splitXmlChildren`（深度感知）切出 `w:tbl` 直接子元素中的 `w:tr`；行数与解析结果不同 → 放弃。每行切出 `w:trPr` → `rawTrPrs[ri]`，`w:tc` 数不同 → 放弃该行；每格切出 `w:tcPr` → `rawTcPr`。保守：宁可不挂也不挂错。

---

## 8. 绘图、文本框、图片元数据

### 8.1 通用工具

- `topLevelDrawings(xml)`：平衡匹配 `<w:drawing>…</w:drawing>`（文本框内可再嵌套 drawing，留在父片段里）。
- `drawingAnchorMeta(frag)`：`<wp:anchor` 存在 → `anchored`；`positionH/V` 的 `posOffset` → `offsetX/YEmu`；`relativeFrom` → `relH/relV`；`<wp:align>` → `alignH/alignV`；`wp14:pctPosH/VOffset` → `pctH/pctV`；`wp:extent` → `extentX/YEmu`；`wp:wrapNone` 或 `behindDoc=1|true` → `noWrap`；**anchor 自身**（`<a:graphic` 之前的部分）含 `wp:wrapTopAndBottom` → `topBottom`。
- `resolveAnchorPagePos(meta, sect)`：仅 `relH ∈ {page, margin}` 且有 `pctH` 或 `alignH` 时解析。参考宽 = page 宽或 page-margins；`relX = refW*pct/100000` 或 center `(refW-w)/2` 或 right/outside `refW-w` 或 0；`pageX = relH==page ? relX : marL+relX`；返回 `xEmu = pageX - marL`，`outsideColumn = pageX+w <= marL || pageX >= pageW-marR`；同理 V（有 relV page/margin 且 pctV/alignV 时）→ `yEmu`。
- 颜色：`gradStopRgb`（`a:srgbClr` 或 `a:schemeClr`（`SCHEME_CLR_SLOTS`: tx1→dk1, bg1→lt1, tx2→dk2, bg2→lt2, …）+ lumMod/lumOff/shade/tint）；`colorNodeHex`；`gradFillApproxHex`（所有 stop 等权平均）；`w14ColorRgb`（另加 satMod → HSL 饱和度调制）。
- `composeGroupCtm(group, outer)`：`wpg:grpSpPr/a:xfrm` 的 off/ext 与 chOff/chExt → 仿射 `{sx, sy, tx, ty}`，子坐标 `X = tx + x*sx`。

### 8.2 `extractTextboxes(xml, ctx, opts {shapes?, pictures?, section?})`

前置：无 `<w:txbxContent`、无（`wp:wrapSquare` + 连线 prst）、无 `<a:txSp`、无 VML WordArt、且未要求 shapes/pictures → `[]`。

**DrawingML 路径**：对每个顶层 drawing 片段解析树，`meta = drawingAnchorMeta`，`pagePos = resolveAnchorPagePos(meta, section)`；文档序遍历，遇 `wps:wsp` → `buildWpsBox`，遇 `wpg:wgp/wpg:grpSp` → 组合 CTM 后递归。

`buildWpsBox(shape)`：
- 无 `w:txbxContent`：
  - 连线 prst（line/straightConnector1/bent/curvedConnector2-4）：`hasLineShapes` → `lineBoxOf`；否则需 `opts.shapes` 且（`cy > 130000` 或 flipH/flipV 或有箭头）才 `lineBoxOf`，否则 null（平线走装饰细线路径）。
  - 其他：`hasLineShapes && !shapes` → null；`!shapes || !prst` → null；`prst == rect` 且 `cy <= 130000` → null。
- 几何/样式：`spPr` 非 noFill：`fill = solidFill ?? gradFill 平均 ?? pattFill fgClr`；`a:blipFill` → `fillImageDataUrl`（mediaByRid）+ `fillTile`；`a:ln` 非 noFill → `borderColor`、`borderWidthPx`(w/9525, 两位小数)、`borderDash`（含 dot → dotted，否则 dashed）；`wps:style` 的 `a:fillRef/a:lnRef`（idx>0）补缺省颜色；`prst` 非 rect 记录；`a:xfrm rot` → `rotDeg`；`ext cx` → `widthPx`；`wps:bodyPr` 无 `a:spAutoFit` 且 `cy>0` → `heightPx = minHeightPx`；`bodyPr` 的 lIns/tIns/rIns/bIns → inset px，`anchor` b/ctr → vAlign。
- `paras = txbxContentParas(content)`（`w:p` → runs+format+styleId；`w:tbl` → `txbxTableParas` 每行一行、单元格间 `  `、嵌套表递归；`w:sdt` 透明）；含 `w:tbl`/`w:sdt` → `readOnly`。
- 无内容的形状：无 fill/border/图 → null；否则 readOnly。有内容但所有段落无 run → null。

`pushShape`：组内形状用 CTM 映射 `a:off` → offsetX/YEmu，缩放宽高，`floating = true`；然后 `applyAnchor(box, meta, pagePos, grouped)`：
- 非 anchored → 不动。
- `pagePos.outsideColumn` → `offset += pagePos`，`floating`，返回。
- `offset += meta.offset`。
- `meta.topBottom && relV ∈ {paragraph, line}`：`h = box.heightPx ?? (非组 && extentY ? extentY/9525)`；`top = offsetY px`；`top+h > 0` → `bandTopPx = top, bandBottomPx = top+h`；`floating`。
- `meta.noWrap || multiDrawing(片段数>1)` → `floating`。

`opts.pictures` 且片段无 wsp 且含 `<pic:pic` → 只读图片 box（`fillImageDataUrl`, extent 尺寸, inset 0）+ applyAnchor。

**VML 路径**（`/<v:(?:shape|rect|roundrect)\b/`）：解析整段；`walkVml`：`v:group` → `vmlGroupScale`（style width/height px ÷ coordsize）；`v:shape/v:rect/v:roundrect` → `vmlBox`：无 txbxContent → `vmlWordArtBox`（`v:textpath string` → 单行 run，字号取 textpath style font-size 或 高度/1.4，再按宽度 `widthPt/(0.62*len)` 压缩，`fillcolor`/`v:fill color/color2` 为文字色，`strokecolor` → textOutline，`position:absolute` → floating + margin-left/top）；有内容 → 宽高（`vmlShapeDimPx`，无单位时按组比例）、`fillcolor`(filled≠f)、`strokecolor`(stroked≠f；组内无 stroke 且未关 → 黑)、非组内 `position:absolute` → floating + `margin-left/top pt → EMU`；paras；含 tbl/sdt → readOnly。

**lockedCanvas 文本**（`<a:txSp`）：`a:txBody/a:p/a:r/a:t` → 只读 box，`a:rPr sz/100*2 → sizeHalfPoints`，`b`，`a:latin typeface`（非 `+` 开头），`algn=ctr`。

`lineBoxOf(shape)`：`prst ∈ LINE_PRSTS`；`borderColor = a:ln solidFill ?? wps:style a:lnRef ?? '000000'`；`prst` 合成：bentConnector* → lineBent，curvedConnector* → lineCurved，头尾箭头都有 → lineArrowDouble，一个 → lineArrow，否则 line；`widthPx = cx/9525`；直线时读 flipH/flipV；`cy>0` → heightPx=minHeightPx，直线且（>12px 或 flip）→ `lineDiag`；`cy==0` → heightPx 12；仅 headEnd 的 lineArrow 翻转 flipH/flipV。

### 8.3 `imageMeta(xml)`（段落级图片）

- `wp:extent` → `imageWidthPx/imageHeightPx`
- `w:jc` center → center；right/end → right
- `pic:spPr` 内第一个 `a:xfrm`：`rot` → `imageRotDeg`（`((round(rot/60000) % 360)+360)%360`）、flipH/flipV
- `picBorderOf`：`pic:spPr/a:ln` 非 noFill 且 `a:solidFill/a:srgbClr` → `{color, widthPt = w/12700 (缺省 0.75)}`
- `a:srcRect` → `imageCrop`（l/t/r/b 千分比 /100000，任一非 0）；`a:stretch/a:fillRect` → `imageFillRect`
- `wp:anchor`：
  - `allowOverlap="0|false"` → `imageNoOverlap`
  - `relativeHeight - 251658240` ≠ 0 → `imageZOrder`
  - wrap：`behindDoc=1` 且无 wrapSquare/Tight/Through/TopAndBottom → `behind`；`wrapTopAndBottom` → `topBottom`；`wrap(Square|Tight|Through)`：side = right 当 `positionH/wp:align == right` 或 `wrapText == left` 或（`wrapText != right` 且 `posOffset > 4680*635`），否则 left → `square-side|tight-side|through-side`；否则 `front`
  - `posOffset` → `imageOffsetXEmu/imageOffsetYEmu`
  - `positionH/V relativeFrom == margin` 且两轴都 `wp:align` → `imagePosH/imagePosV`；H 为 margin/page 且仅 H 有 align → 只设 `imagePosH`

`vmlImageMeta(xml)`（VML 图片）：`v:shape style` width/height pt → px；`w:jc`；`position:absolute` → `z-index` 负 → behind 否则 front；`margin-left/top pt → EMU`；`mso-position-horizontal/vertical(-relative)` 都是 margin/page 且合法 → `imagePosH/V`。

`normalizeImageZOrders(blocks)`：任一 `|imageZOrder| > 10000`（LibreOffice 写 1,2,…）→ 按 zOrder（稳定按文档序）重排为 0..n，0 删除字段，全部标 `imageZOrderNormalized`（保存时统一改写）。

### 8.4 媒体解析

`mediaDataUrl(zip, rels, rId)`：rel 缺失或 External → null；路径规则见 4.2；`imagePartMime` 无 → null；EMF/WMF/EMZ/WMZ → `metafileToDataUrl`（gzip 判定 `1f 8b`，EMZ/WMZ 按内容判 EMF：`iType==1` 且偏移 40 处 `' EMF'`；vendored `emf-converter` `dpiScale 2` 渲染到 canvas → PNG dataURL；失败 null 并 warn）；TIFF → `tiffToDataUrl`（UTIF 解码取最大页 → canvas → PNG；无 DOM 环境返回 null）；其他 → `data:<mime>;base64,…`。

`extractImage(xml, ctx)`：`a:blip r:embed ?? r:link`；External 或 `https?://` 目标直接返回 URL（浏览器自己下载）；否则同上。

`tableBlipMedia`（解析前对 `w:tbl`/`w:sdt` 内的 `a:blip` 预取）与 `resolveBlipMedia`（对单个段落按需补 `a:blip`/`v:imagedata` 的 rId）都是因为 TS 的 `extractCell/extractRuns` 是同步函数而 JSZip 是异步；Rust 全同步后可以改为惰性缓存。

### 8.5 图表 / SmartArt / OLE

- `extractChart`：`<c:chart r:id>` → chart part 路径；`parseChartPartXml(xml, path)`：`c:chartSpace/c:chart/c:plotArea` 第一个 `*Chart` 子元素决定 `kind`（`CHART_KINDS` 映射）；`c:barDir val=bar` → horizontal；每个 `c:ser`：`c:val` 缓存点 → values（非数字 → null）；第一个有 `c:cat` 的系列决定 categories（格式代码含 y/d 时把 Excel 序列号转日期文本）；`seriesName`；`chartTitle`；`cx:chartSpace` 走 chartex 解析。非 chartex 的 part 原文存入 `ctx.chartParts[path]`（保存时补丁）；`wp:extent` → widthPx/heightPx。
- `extractDiagramText`：`r:dm` → data part；`dgm:pt`（排除 type pres/parTrans/sibTrans）的 `a:t` 文本；`dgm:cxn`（无 type 或 parOf）按 `srcOrd` 建树，根（无父）先序遍历，孤立点按文件序追加。
- `extractDiagramDrawing`：`data\d*.xml → drawing\d*.xml`；`dsp:sp/dsp:spPr/a:xfrm` off/ext → px（连线允许零宽或零高）、rot、prst、`a:ln`（lnHex/lnWPx）、`a:blipFill`（用该 part 自己的 rels + fillRect）或 solidFill（srgbClr / schemeClr 查主题，缺省 `9AB5E4`）、`dsp:txBody` 文本/字号(sz/100)/颜色。宽高取 `r:dm=` 之前最后一个 `wp:extent`。
- `extractLockedCanvas`：`<lc:lockedCanvas>` 之前最后一个 `wp:extent` 为显示尺寸；`a:grpSpPr/a:xfrm` 的 chOff/chExt → 缩放；子 `a:sp`/`a:pic`：几何缩放到 px、rot、prst、图片（mediaByRid）或 fill、`a:txSp/a:txBody` 文本（字号 sz/100 **不缩放**）。然后做 LibreOffice 对齐的"溢出文本分栏"启发式（`colGeom`：每行字符数 = w/(fontPx*0.72)，行距 1.2；溢出 >2 倍高度的文本形状从 y=0 起按半列高度错开，单字母列拆成逐字形状并按 y 排序）。
- `oleDisplay`：`o:OLEObject ProgID`；`v:imagedata r:id` 的预览图（含 metafile）；尺寸取 `v:shape style` pt 或 `w:object dxaOrig/dyaOrig` twips；`w:jc`。

---

## 9. 页眉页脚

### 9.1 part 选择（`readHeaderFooterPart`）

在 `document.xml` 中找所有 `<w:headerReference …/>`（或 footer）。`hfType == default` 时：`w:type="default"` → 否则 `w:type="odd"`（非标准，Word 缺省页）→ 否则无 `w:type` 的引用；first/even 只取对应 typed。`r:id` → rels → 路径 → part XML。返回 `hfContentFromXml(...)` + `hfImages`。

`parseAllHfParts`：遍历 rels 中 type 以 `/header`、`/footer` 结尾的关系，按 rId 输出 `HfPartInfo`（多节文档由 `SectionInfo.headerRefs/footerRefs` 查）。

### 9.2 `hfContentFromXml(xml, kind, theme, styles, tableMedia)`

1. 去掉所有 `mc:Fallback`（避免 DrawingML+VML 双份文本）。
2. 把每个 `fldChar begin … fldChar end` 区间改写：instrText 拼接后，含 `NUMPAGES` → `</w:r><w:r>{rPr}<w:t></w:t></w:r><w:r>`；含 `PAGE` → `` 并 `hasPageNumber=true`；其他字段 → separate 之后的完整 `<w:r>…</w:r>`（含 `<w:t` 的）保留为缓存结果。（`</w:r>…<w:r>` 的包法是为了让被切开的 begin/end run 保持平衡；空 run 之后会被丢弃。）
3. `w:fldSimple w:instr`：NUMPAGES/PAGE 同上；其他保留 inner。
4. `text = plainText(cleaned)`，`watermark = kind==header ? readWatermarkText(xml) : null`（`v:textpath string`），`paras = hfParagraphs(去掉残留 <w:fldChar…/> 的 cleaned, …)`。

### 9.3 `hfParagraphs`

根 `w:hdr`/`w:ftr`；上下文 `ctx` 只带 `themeColors/styles`（不解析超链接目标）。遍历 `childrenThroughSdt(root, ['w:tbl','w:p'])`：

- `w:tbl` → `hfTableRowParagraphs`：每行一个 `HfParagraph{runs:[], cells}`；列宽：各 `w:tcW`（非 pct）→ 缺失时用 `tblGrid` 按 gridSpan 求和 → `widthPct`；单元格 `hfCellContent`（段落 runs（含图，但锚定/绝对定位图去掉 image 字段）、align 取第一个有 jc 的段落、`w:shd` fill、嵌套表格展平、嵌套表后的空段落去掉）；整行无文字无图无 fill 则跳过。含 `w:tblpPr` 的浮动表格延后到下一个段落之后输出。
- `w:p`：`runs = extractRuns`；若 runs 为空但有 `w:r`/`w:pict` → `textboxParagraphs`（所有 `w:txbxContent` 内的段落，政府公文的 "— PAGE —" 在 VML 文本框内）；否则 `{ ...(样式 display.align 非 justify), ...(样式 tabStops), ...直接 format, ptabAligns?(按 tab 出现顺序，`w:tab` 占位 undefined，`w:ptab w:alignment`), frameXAlign?(framePr 非 dropCap 的 xAlign: right/outside→right, center, left/inside→left), runs }`。
- 尾部空段落（无 cells）去掉。

### 9.4 `hfImages(zip, partPath, partXml)`

用 part 自己的 rels。`<w:drawing>` 片段：非锚定且位于顶层 `w:tbl` 区间内的跳过（已在单元格 run 上）；多个 `a:blip` 取第一个能解析的；`wp:extent` 尺寸；`wp:anchor` → `floating`、`behindDoc` → behind、`wp:wrap(None|Square|Tight|Through|TopAndBottom)` → wrap、`readAnchorPos`（`wp:align` → posH/posV；`posOffset` → posXPx/posYPx + posHRel(page|margin)/posVRel(page|paragraph|margin)）；内联 → 所在段落 `w:jc` → align。`<w:pict>` 片段：跳过含 `v:textpath`（水印）；`v:imagedata r:id`；`v:shape style` 尺寸；`position:absolute` → floating、`z-index` 负 → behind、`mso-position-horizontal/vertical` → posH/posV；`gain|blacklevel` → washout。

---

## 10. 公式、墨迹、节枚举、列表标记

- **公式**（`math.ts`）：`ommlFragmentsOf`（正则取 `<m:oMath …>…</m:oMath>`，`(?=[\s>])` 排除 oMathPara，oMath 不嵌套）；`mathTokensOf`/`mathTokens`（`<m:t>` 文本）；`ommlToMathML(omml)`（OMML 树 → MathML Core，覆盖 f/rad/sSup/sSub/sSubSup/d/nary/m/acc/bar/box/func/limLow/limUpp/groupChr/eqArr 等）；`ommlToLatex`（同类树 → LaTeX，含 `LATEX_FUNCTIONS`、符号映射；不支持时 null）。这两个转换是 parser 侧唯一较大的"纯算法"模块，可直接移植。
- **墨迹**（`ink.ts`）：`ANCHOR_RUN_RE = /<w:r><w:drawing><wp:anchor[\s\S]*?<\/wp:anchor><\/w:drawing><\/w:r>/g`，`wp:docPr name` 以 `aidocs-ink` 开头即墨迹 run；`findInkRuns` 读 posOffset/extent/descr(payload)/r:embed；`stripInkRuns` 用于 detect。`parseDocx` 末尾对每个 block 的 `originalXml` 收集 `InkInfo`。
- **节枚举**（`readSections(parsed)`，事后）：遍历 blocks，`originalXml` 含 `<w:sectPr` 的块（分节段落或尾部 hidden 块）关闭一个节：`SectionInfo{settings, startType(w:type 缺省 nextPage), firstBlockIndex, lastBlockIndex=docxIndex, sectPrXml, titlePg, pageNumberStart/Fmt, headerRefs/footerRefs(w:type 缺省 default)}`；没有任何 sectPr 时给一个缺省节。
- **列表标记**（`list-markers.ts`，渲染器与 `applyTocEntryNumbers` 使用）：`computeListMarkerInfos(items, defs)`：计数器按 `abstractNumId` 全文累积；出现某级时其更深级别清零（`c.length = lvl+1`）；`startOverrides[lvl]` 在该 `numId:lvl` 首次出现时生效一次；`lvlText` 中 `%n` 用第 n-1 级计数（缺省 `start ?? 1`）经 `formatNumber(value, numFmt, customFormat)` 替换；bullet：`decodeSymbolText(font, lvlText) ?? BULLET_GLYPHS[lvlText] ?? lvlText`，PUA 字符/空/符号字体不可解码 → `DEFAULT_BULLETS[lvl % 9]`，符号字体时附 `symbolChar/symbolFont`；`numFmt none` → 空串。`formatNumber` 支持 decimal/decimalZero/lower|upperLetter（27 → AA 重复式）/lower|upperRoman/lower|upperGreek（24 字母跳过 final sigma）/chineseCounting(Thousand)/japaneseCounting（`toChinese`，10–19 去"一"）/decimalEnclosedCircle（①–⑳）/custom 枚举循环/none。`markerTabAdvance` 计算标记后的缺省制表推进。
- **符号字体**（`symbol-fonts.ts`）：`Symbol`（Adobe Symbol 布局：希腊字母、数学符号）、`Wingdings`、`Wingdings 2`（0x95–0xA6 项目符号系列）、`Wingdings 3`/`Webdings`（识别为符号字体但无表）；code 在 `0xF000–0xF0FF` 时减 `0xF000`；`toSymbolPua` 反向。

---

## 11. Parser 对保存路径的契约（必须逐字保留）

| 字段 | 谁消费 | 要求 |
| --- | --- | --- |
| `internal.originalBytes` | `saveDocx` | 原始 zip 字节；无编辑时直接返回它（`roundtrip.test`: `saved === bytes`） |
| `internal.documentXml` | `saveDocx` | 主 part 的完整字符串 |
| `internal.bodyInnerStart/End` | `saveDocx` | 顶层元素覆盖的区间，其外的字节原样保留 |
| `extras.elements[i]` | `saveDocx` | 与 `blocks[i].docxIndex === i` 一一对应；sdt 拆分后 elements 同步拆分；`{name,start,end}` 与 `documentXml` 同一索引体系 |
| `Block.originalXml` | `saveDocx`（original 块拷贝）、`readSections`、ink 扫描、编辑器脏检测 | 精确切片；sdt 块为 part 切片；ins/del 包裹块为整个 wrapper |
| `Block.hidden` | `saveDocx` | 尾部 sectPr 等，保存时移到 body 末尾 |
| `Block.invisibleMarker` | `saveDocx` | 保持原位（不能当 hidden 移动） |
| `Block.rawPPr` | `generate.mergePPrFormat` | 原样复用或合并格式改动；缺失时从 format 重建 |
| `Run.rawRPr` | `generate.mergeRPrModel` | 未建模属性保真；建模字段只在与原编码不同（被编辑）时重建 |
| `Run.image.xml`、`Run.math.omml`、`Run.ruby.xml`、`Run.fldBeginXml`、`Run.refInstr` | `generate` | 原样重发 |
| `Run.link.rId` | `generate` | 复用已有关系而不新建 |
| `Run.commentIds` / `Block.commentStarts/Ends` / `bookmarks` / `hiddenBookmarks` | `generate` | 重建段落时重发范围标记与书签（否则 REF/TOC 锚点断裂） |
| `TableCell.rawTcPr`、`TableModel.rawTrPrs` | `generate.generateTableModelXml` / `patchTableCellTexts` | 手术式补丁 |
| `Block.sdtShell.openXml/closeXml/group` | `saveDocx` | 重建段落时重新包裹；同 group 只有首块带 open、末块带 close |
| `extras.chartParts[path]` | `saveDocx` 图表数据编辑 | 原始 chart part XML |
| `Block.imageZOrder/imageZOrderNormalized` | `applyImageZOrder` | 保存时统一改写 relativeHeight |
| `Block.blockRevision/pPrChangeInfo/paraMarkDel`、`Run.ins/del/rPrChange` | 审阅接受/拒绝 | 需要 `id` 以稳定重发 |

不变式（测试覆盖）：
1. 无编辑保存 → 字节完全相同。
2. 编辑一个段落 → 其他 zip 条目字节相同；`document.xml` 中其他块的 `originalXml` 子串原样出现。
3. 病态输入（3000 层嵌套 smartTag）→ 该块 passthrough 且仍能字节保真保存。

---

## 12. 兼容性细节清单（从代码注释与测试提炼）

这是重写时最容易丢的部分，按主题列出。括号内是来源语料/缺陷号。

**Zip / 包**
- Info-ZIP Unicode Path extra field 必须忽略（POI）。
- 多个 `<w:body>` 兄弟元素按序拼接（POI MultipleBodyBug）。
- 主 part 可能不叫 `document.xml`（LibreOffice 语料 `word/trial.xml`）。
- 图片 part 可能是 `media/*.bin`，靠 `[Content_Types].xml` 判 MIME。
- `xmlns` 前缀不一定是 `w:`，至少 `removePersonalInformation` 的检测是前缀无关的；其他地方假定标准前缀（`w:`/`a:`/`wp:`/`pic:`/`m:`/`v:`）。Rust 若做前缀无关需评估成本，建议先保持标准前缀假定。

**段落 / 样式**
- 没有 `xml:space="preserve"` 的 `w:t` 首尾 XML 空白被 Word 丢弃。
- `w:u` 不是布尔属性：`<w:u w:color="…"/>` 无 `w:val` 等于无下划线（Pages/LibreOffice）。
- `w:jc` 在 `w:bidi` 段落里是逻辑值，left/right 需互换。
- `w:before/afterAutospacing="1"` 时忽略字面值。
- `w:line="0" lineRule="atLeast"` 是"退出 docGrid 吸附"的信号。
- `w:pBdr` 可重复，`w:val="nil"` 与 `"none"` 都表示无边框（Word 重置样式边框时写 nil）。
- 颜色值容忍前导 `#`（tdf#57589）。
- `w:outlineLvl="9"` 表示正文，且阻断 basedOn 的标题继承。
- 样式 `w:default` 取最后一个声明；类型无声明时取第一个。
- 空 EA 主题槽（`<a:ea typeface=""/>`）：Word 用主题语言的缺省字体（DengXian / Yu Gothic+Yu Mincho / Malgun Gothic），不用字面 `w:eastAsia`。
- rtl run 读 `bCs/iCs/szCs`，非 rtl 读 `b/i/sz`，无交叉回退（Word for Mac 实测）。
- 符号字体文本解码成 Unicode 后要同时去掉 `rawRPr` 里的 `w:rFonts`。
- 数字字符引用只解一次；`fast-xml-parser` 已解五个命名实体，再解一次会把字面 `&lt;` 变成 `<`。
- 样式级 `w:vanish`（z-TopofForm/z-BottomofForm）整段隐藏，但 `w:specVanish` 是样式分隔符不算隐藏。
- 空段落行高取段落标记或最后一个空 run 的 `w:sz`/`w:rFonts`。
- 分节段落有文字时按普通段落显示，sectPr 随 rawPPr 保留。
- 顶层 `<w:br w:type="page">` 与 run 外的 `w:br` 都要识别。
- 以 `_` 开头的书签是 Word 内部书签，隐藏但要重发。

**字段**
- 只由 XE/REF/简单字段/可转换 HYPERLINK/FORMCHECKBOX 组成的段落保持可编辑，其余字段段落整体保护。
- `FORMCHECKBOX` 无 `w:checkBox` 定义时不能折叠。
- TOC 行可能没有字段字符（字面页码），靠 `TOC1`/`TOC 1` 样式识别。
- 只有 `fldCharType="end"` 的段落是"字段结束标记"（TOC 结尾 + 分页）。
- 页眉页脚里 Word 可能把 `PAGE` 拆到多个 instrText run；`fldChar` 可能带 `w:fldLock="0"`（Pages）。

**修订**
- 顶层 `w:ins`/`w:del` 包裹整段或整表。
- `moveFrom/moveTo` 按 del/ins 处理，并在块上打 `moveRevision`。
- `delInstrText`、`cellIns`、`cellDel` 出现时段落整体保护。
- `pPrChange` 内嵌 `w:pPr`，切 `rawPPr` 要深度感知。

**表格**
- `w:tr`/`w:tc`/`w:p` 可能被 `w:sdt` 包裹（研究报告模板每个字段一个 sdt），用 `childrenThroughSdt` 透明处理。
- 重复 `w:tcW` 取最后一个；tcW 与 tblGrid 不一致时以 tcW 为准。
- `w:tblW type=pct` 的值可能是 `"50%"` 字面。
- `w:hMerge`（旧式水平合并）折叠为 colSpan。
- `w:trHeight` 上限 31680（有生成器写 EMU 量级）。
- 嵌套超过 8 层展平为 1×1（POI 5000 层）。
- 重复 `w:tcBorders`/`w:tblBorders` 按边合并、后者胜。
- `w:tblLook` 既有属性形式也有 `w:val` 位掩码形式。
- 单元格内锚定形状要提取为 `anchoredBoxes` 并从段落文本中剔除（tdf134277）。

**绘图 / 图片**
- 一切探测都在去掉 `mc:Fallback` 后的 XML 上做（否则每个 DrawingML 形状的 VML 孪生会被重复识别）。
- chartex（2014 扩展图表）优先用 Fallback 预渲染图。
- `wp:anchor relativeHeight = 251658240 + rank`；LibreOffice 写 1,2,… 需归一并在保存时统一改写。
- `behindDoc=1` 但显式 `wrapTight` 时仍环绕（Word 行为）。
- `wrapText` 命名的是文字所在侧，对象浮在对侧；`bothSides` 时用 X 是否过半页（4680 twips）猜测。
- `allowOverlap="0"` 与兄弟锚点纵向重叠时 Word 把对象挤出（tdf#134114），近似为 front 叠放。
- `positionV relativeFrom="line" align="center"` 时图片以锚行居中（tdf#162551）。
- 细横线：`wp:extent cy ≤ 130000 EMU`（≈13.7px）的无文字锚定形状渲染为线而非芯片。
- 所有 `wps:wsp` 都 noFill/无描边/无效果/无图/无文字 → 不可见（转换器产物）。
- VML：只有 `v:shapetype`、`visibility:hidden`、`stroked=f` 的白矩形都不可见；`<v:rect o:hr="t">` 是 `<hr>` 导入。
- `v:group` 子形状用无单位组坐标，需按 `coordsize` 与 style 宽高换算。
- WordArt（`v:textpath`）退化为单行文字；`fitpath` 用 `widthPt/(0.62*len)` 近似压缩字号。
- 一个段落锚定多个 drawing 时不堆叠，每个按自身偏移浮动。
- `wp:wrapTopAndBottom` 且 relV 为 paragraph/line 时，锚定段落保留到 box 底部的流高度（`bandTopPx/bandBottomPx`）。
- 页面/边距锚定且位于正文栏之外（简历侧栏）→ 绝对定位，忽略 wrap 类型。
- `mc:AlternateContent` 可能含多个 `a:blip`（mac Word：PDF Choice + PNG Fallback），取第一个能解析的。
- 图片 part 可能是 EMF/WMF/EMZ/WMZ/TIFF，需要转 PNG 才能显示；OLE 预览通常是 WMF。
- `r:link` 外链图片直接返回 URL。

**页眉页脚**
- `w:headerReference` 可能无 `w:type`（视为 default）或 `w:type="odd"`（非标准，视为 default）。
- 布局表格每行一行显示；浮动表格（`w:tblpPr`）延后到其后段落之后。
- 文本框内的段落（政府公文页码）要提出来。
- `w:ptab` 带自身对齐，忽略 tab 位；`w:framePr xAlign` 的页码框与后续段落同行。
- 水印是 `v:textpath`，从图片列表排除。

**编号**
- `w:lvl` 无 `w:start` 时从 0 起。
- `w:numStyleLink → w:styleLink` 间接。
- w14 自定义 `numFmt custom`（`α, β, γ, ...`）藏在 `mc:AlternateContent`。
- `numId="0"` 是显式"无编号"，样式级 `numId 0` 取消 basedOn 继承的编号。
- 字母编号 27 → `AA`（重复而非进位）；希腊字母跳过 final sigma。
- TOC 行的编号来自 `w:numPr`（Pages 每行一个 numId + startOverride），计数与正文列表共享。

**批注 / 脚注**
- LibreOffice 风格批注只有 `w:commentReference` 无范围标记 → 挂到最近 run。
- 批注回复/已解决在 `commentsExtended.xml`，以 `w14:paraId` 关联。
- 脚注部件里带 `w:type` 的条目是分隔符，不是脚注；首段开头有自引用标记与一个空格。

---

## 13. Rust 重写设计建议

### 13.1 crate 与模块划分

建议单 crate `docx-parse`（lib），模块与 TS 文件一一对应，便于对照迁移与差分测试：

| Rust 模块 | 对应 TS | 职责 |
| --- | --- | --- |
| `package` | `zip-load.ts`, `parse.ts` 顶部 | zip 装载、Unicode Path 中和、限额、Content_Types、主 part 定位、rels、路径解析 |
| `xml` | `xml-utils.ts` | 带字节区间的有序 XML 树、`through_sdt`、on/off 属性、序列化 |
| `scan` | `scan.ts` | body 顶层元素扫描（多 body） |
| `theme`、`styles`、`numbering`、`settings`、`comments`、`notes`、`sources` | 同名 | 辅助 part |
| `section` | `section.ts` 读取部分 | `sectPr` 几何、`section_at`、`read_sections` |
| `block` | `buildBlock` | 决策树 |
| `paragraph` | `buildTextParagraph`, `extractParaFormat` | 段落 |
| `run` | `extractRuns`, `buildRun`, `mergeRuns` | run 状态机 |
| `table` | `extractTable*`, `extractCell`, `attachRawTablePr` | 表格 |
| `drawing` | `extractTextboxes`, `drawingAnchorMeta`, `imageMeta`, `vml*` | 形状/锚点/图片元数据 |
| `media` | `mediaDataUrl`, `metafile.ts`, `tiff.ts` | 媒体解码 |
| `chart`、`diagram` | `chart.ts`, `extractDiagram*`, `extractLockedCanvas` | 图表/SmartArt/画布 |
| `hf` | `readHeaderFooterPart`, `hf*` | 页眉页脚 |
| `math` | `math.ts` 读侧 | OMML → tokens/MathML/LaTeX |
| `ink`、`list_markers`、`symbol_fonts`、`watermark` | 同名 | 小工具 |
| `model` | `types.ts` | 输出类型（serde） |

### 13.2 依赖选择

| 需求 | 建议 | 备注 |
| --- | --- | --- |
| zip | `zip` crate | 先对字节做 0x7075 中和再交给库（与 TS 一致，避免依赖库行为）；限额直接读 central directory 声明大小 |
| XML | `quick-xml`（事件流 + `buffer_position`）自建树；或 `roxmltree`（节点有 `range()`） | 关键要求：字节区间、有序子节点、属性顺序、不 trim、深嵌套不递归（100k 层）。`roxmltree` 是递归下降实现且做实体解码，需验证深度与实体行为；自建树最可控 |
| 正则 | `regex` | TS 版有上百个正则，多数只是"找子串"或"读属性"，Rust 中优先用树查询替换；确实需要正则的地方（`plainText`、`sdtSlices`、VML style 解析）保留 |
| 图片 | `image`（TIFF→PNG）、`flate2`（EMZ/WMZ gunzip）、`base64` | EMF/WMF → PNG 没有成熟 Rust crate；方案：(a) 先降级为 `brokenImage` + 尺寸；(b) 移植 vendored `emf-converter` 渲染到 `tiny-skia`；(c) 通过 FFI/WASM 复用现有 JS 转换器 |
| 哈希 | `sha2` | 仅写侧/校验用，parser 只读属性 |
| 序列化 | `serde` + `serde_json` | 与 TS `ParsedDoc` 字段名、单位一致，`Option` 为 `None` 时跳过（`skip_serializing_if`），保持编辑器无感 |
| 绑定 | `napi-rs`（Electron 主进程/渲染进程 Node）或 `wasm-bindgen`（浏览器） | 见 13.4 |

### 13.3 同步模型与媒体解析

TS 的 async 只是 JSZip 的副作用，导致了 `tableBlipMedia` 预取、`resolveBlipMedia` 补取、`mediaByRid` 传参这些绕路。Rust 全同步后：

- `MediaCache { zip, rels, cache: HashMap<rId, Option<MediaRef>> }`，`get(rId)` 惰性解码并缓存。
- `MediaRef` 可以是 `DataUrl(String)`、`External(String)`、`Part { path, mime, bytes }`。**建议**输出层默认不内联 base64（大文档几十 MB 的 dataURL 会拖慢 JSON 传输），改为 `media_id` 引用 + 单独的二进制表；但这改变了 `Block.imageDataUrl`/`Run.image.dataUrl` 契约，需要编辑器配合。第一阶段可保持 dataURL 以做差分测试。

### 13.4 输出与集成

- 第一目标：`parse(bytes) -> ParsedDoc` 序列化为 JSON，字段与 TS 完全一致（包括 `Map` 序列化为对象、`null` vs 缺省的差别）。这样 `apps/docs` 可以先零改动接入，且能对 fixtures 做 **TS 输出 vs Rust 输出的 JSON diff**。
- `internal.originalBytes` 不进 JSON（编辑器自己持有字节）；`internal.documentXml` 与偏移量的索引体系要在边界声明清楚（见 2.3）。
- 建议在 Rust 侧也实现 `scanBody` 的对偶——写侧的 `saveDocx` 会需要同样的切片。

### 13.5 错误策略

- 只在"根本不是 docx / 缺主 part / zip 超限"时返回 `Err`。
- 任何单块失败（XML 无法解析、深度超限、媒体解码失败）→ 该块降级为 `passthrough`（保留 `originalXml`），文档整体成功。
- 辅助 part 解析失败（styles/numbering/comments…）→ 空集合 + warn，不影响正文。
- 所有递归都要有深度上限或改成迭代；`hostile-input.test.ts` 的三个场景要过。

### 13.6 性能注意

- TS 版对同一段落可能解析多次（detect 正则、`xmlParser.parse`、`extractTextboxes` 再 parse、`isInvisibleEmptyShape` 再 parse）。Rust 版一次建树（带区间）后所有探测都查树，`detect` 的"去 Fallback/去 ink"变成遍历时跳过对应子树。
- `plainText` 在决策树里被调用多次，可在段落节点上缓存"去文本框可见文本"与"含文本框可见文本"两个值。
- `sectionAt` 用二分查找 sectPr 区间。
- 避免为 `rawRPr` 重新序列化：直接切原文区间即可（TS 因树没有区间才序列化；注意这会让 `rawRPr` 从"语义等价"变成"字节等价"，对 `mergeRPrModel` 更友好，但 `sameStyle` 比较仍成立）。

### 13.7 里程碑

| 阶段 | 内容 | 验收 |
| --- | --- | --- |
| M0 | `model` 类型 + serde；差分工具：用 TS 对 `fixtures/generated/*.docx` 与测试合成夹具导出期望 JSON | 工具可跑 |
| M1 | package/xml/scan/rels/theme/styles/numbering/settings；纯文本段落、heading、listItem；`originalXml/rawPPr/rawRPr/docxIndex/internal/extras` | 无编辑回环 + kitchen-sink 段落 JSON 一致 |
| M2 | run 全特性：字段折叠、批注、修订、ruby、内联公式、noteRef、符号字体、主题字体、w14 textFill | `tests/` 中 raw-rpr/rtl-runs/ruby/revisions/comments/character-styles/symbol-fonts/rfonts-dual-slot 场景 |
| M3 | 表格（含嵌套、样式、rawTcPr/rawTrPr、深度上限） | table-display/table-style/nested-table-edit/deep-nested-table/table-revisions |
| M4 | 绘图：图片块、run 图片、锚点/wrap/zOrder、文本框/形状/连线/组、VML、细线、不可见判定 | image-wrap/anchored-textbox/shape-display/line-shape/vml-textbox/wordart-vml/wrap-topbottom-band/page-anchored-boxes/cell-anchored-boxes/decorated-paragraphs |
| M5 | 页眉页脚（6 变体 + hfParts）、脚注尾注、参考文献、水印、节枚举、保护 | header-footer-*/hf-*/notes/sections/watermark-theme-sources/write-protection |
| M6 | 图表、SmartArt、lockedCanvas、OLE、metafile/TIFF | chart-*/smartart-ole/metafile*/emf-image |
| M7 | ink、TOC 编号、zOrder 归一、hostile input、多 body、非常规主 part | ink/field-display/scan-multiple-body/main-part-path/hostile-input/zip-local-names |

测试策略：TS 的 77 个测试文件几乎都用 `tests/helpers/build-docx.ts` 在内存里拼 XML 生成 docx，再断言 `parseDocx` 结果。最省力的移植方式是写一个 TS 脚本把每个测试用到的合成 docx 落盘为 `.docx` + 期望 `.json`（golden），Rust 用 golden 测试；行为差异则回到本文对应小节核对。

---

## 14. 附录：`parse.ts` 函数索引

| 函数 | 行数（约） | 职责 |
| --- | --- | --- |
| `assertZipWithinLimits` | 82 | zip 炸弹限额 |
| `contentTypesOf` / `imagePartMime` | 131 / 162 | Content_Types 缓存、图片 MIME |
| `parseDocx` | 184 | 主流程 |
| `listRefOf` / `listKindOf` | 450 / 468 | 段落编号引用与类型 |
| `colorFrom` | 480 | `w:color` + 主题解析 |
| `parseSdtBlock` / `sdtMeta` / `splitSdtParts` / `sdtTableXml` | 501–649 | 内容控件 |
| `buildBlock` | 670 | 块类型决策树 |
| `headingLevelOf` / `staysVanished` | 1206 / 1225 | 标题级别、隐藏段落判定 |
| `buildTextParagraph` | 1233 | 文本段落 |
| `crossParaCommentMarkers` / `bookmarkNamesOf` / `rawPPrOf` | 1443–1508 | 段落级标记 |
| `isThinRule` / `ruleDisplayOf` / `isInvisibleEmptyShape` / `isInvisibleVmlPict` | 1515–1584 | 装饰线与不可见形状 |
| `stripTextboxes` / `hostPageBreak` / `strayParaRuns` / `txbxContentParas` / `txbxTableParas` / `paragraphStrayBox` / `txbxHasStructuredContent` | 1587–1719 | 文本框内容 |
| `vmlStyleDimPx` / `vmlColorHex` / `vmlGroupScale` / `vmlShapeDimPx` / `vmlWordArtBox` | 1722–1893 | VML 几何/颜色/WordArt |
| `lineBoxOf` | 1918 | 连线形状 |
| `gradStopRgb` / `colorNodeHex` / `gradFillApproxHex` / `w14ColorRgb` / `saturationModulate` / `w14TextFillHex` | 2008–2340 | DrawingML/w14 颜色 |
| `topLevelDrawings` / `drawingAnchorMeta` / `resolveAnchorPagePos` / `composeGroupCtm` | 2082–2385 | 锚点几何 |
| `extractTextboxes` | 2387 | 文本框/形状提取（DrawingML + VML + lockedCanvas 文本） |
| `extractParaFormat` / `autoSpaceOf` / `tabStopsOf` | 2830 / 2795 / 2804 | 段落格式 |
| `convertibleHyperlink` / `onlyOleFields` / `onlyXeFields` / `checkboxStateOf` | 2968–3023 | 字段判定 |
| `emptyParaSizeHalfPoints` / `emptyParaMarkFont` | 3030 / 3049 | 空段落度量 |
| `extractRuns` / `rubyFragmentsOf` / `rubyPartText` / `onOffOf` | 3064–3319 | run 状态机 |
| `themedRFonts` / `emptyEaSlotFont` | 3342 / 3331 | 主题字体解析 |
| `buildRun` / `mergeRuns` / `sameStyle` | 3380 / 3591 / 3601 | 单 run 构造与合并 |
| `tableSummary` / `extractTable` / `tcwColumnWidths` / `flattenedTableModel` / `extractTableModel` / `rowRevisionOf` / `attachRawTablePr` / `applyTableStyleDisplay` / `borderLinesOf` / `mergedBorderLinesOf` / `cellMarginsOf` / `extractCell` | 3637–4199 | 表格 |
| `hfPartInfo` / `hfImages` / `readAnchorPos` / `hfTblRanges` / `hfTableMedia` / `hfContentFromXml` / `readHeaderFooterPart` / `parseAllHfParts` / `hfParagraphs` / `hfTableRowParagraphs` / `hfCellContent` / `textboxParagraphs` | 4201–4771 | 页眉页脚 |
| `parseCompatibilityMode` / `parseLayoutSettings` / `parseEvenAndOddHeaders` | 4392–4419 | settings 杂项 |
| `parseNotesPart` / `parseSources` / `parseTheme` / `readThemeFontLangEa` | 4773–4804 | 辅助 part |
| `plainText` / `mathTokens` / `decodeNumericCharRefs` / `decodeEntities` | 4806–4865 | 文本工具 |
| `fieldDisplayOf` / `fieldLabel` | 4873 / 4980 | 字段显示 |
| `normalizeImageZOrders` / `applyTocEntryNumbers` | 4916 / 4937 | 后处理 |
| `rectFrac` / `picBorderOf` / `imageMeta` / `vmlImageMeta` | 5022–5192 | 图片元数据 |
| `mediaDataUrl` / `tableBlipMedia` / `resolveBlipMedia` / `extractImage` / `relPartPath` | 5195–5305 | 媒体 |
| `extractDiagramText` / `extractLockedCanvas` / `extractDiagramDrawing` / `oleDisplay` / `extractChart` | 5314–5734 | SmartArt/画布/OLE/图表 |
| `parseStyles` / `mergeTableDisplay` / `tableStyleDisplayOf` / `styleRunFormat` / `styleDisplayOf` | 5748–6169 | 样式 |
| `resolveMainDocumentPath` / `parseRels` / `parseComments` / `parseProtection` / `parseWriteProtection` / `parseRemovePersonalInfo` | 6175–6324 | 包与设置 |
| `numFmtOfLevel` / `parseNumberingLevel` / `parseNumbering` | 6328–6440 | 编号 |
