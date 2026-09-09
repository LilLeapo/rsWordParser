# SPEC 05 · 属性表（semantic/properties）

对应 `docs/03` 第 6.1 节。职责：用一张声明式表格驱动 `rPr/pPr/tcPr/tblPr/trPr/sectPr` 及其子容器的读取、比较、合并写回与修订快照，并携带 schema 顺序。

## PROP-01 表格格式

每个属性容器一张表，每行一个建模字段。由 `build.rs` 从表生成 Rust 类型与函数（`PROP-07`）。行的列：

| 列 | 含义 | 示例 |
| --- | --- | --- |
| `field` | Rust 字段名 | `bold` |
| `element` | 子元素 QName（`ns:local`） | `w:b` |
| `codec` | 值编解码（`PROP-02`） | `OnOff` |
| `attrs` | 复合元素的属性列表与各自 codec | `w:color`: `val:HexColorOrAuto, themeColor:Enum, themeTint:Hex2, themeShade:Hex2` |
| `cs_twin` | Cs 孪生元素 | `w:bCs` |
| `order` | 在容器 schema 顺序中的序号（`PROP-05`） | 3 |
| `in_change` | 是否出现在 `*PrChange` 旧值快照 | yes |
| `multi` | 是否允许多次出现（如 `w:tab`） | no |

未建模的子元素**不需要**出现在表里：它们以 `Raw` 形式保留在 DOM 中，合并时原位不动。

## PROP-02 编解码

| codec | 解析 | 生成（Transitional / Strict） |
| --- | --- | --- |
| `OnOff` | 元素不存在 → `None`；存在无 `w:val` → `Some(true)`；`w:val ∈ {1,true,on}` → `Some(true)`；`{0,false,off}` → `Some(false)`；其他 → `Some(true)` + 诊断 | true → 裸元素；false → `w:val="0"` / `w:val="false"` |
| `HalfPoints` (`ST_HpsMeasure`) | 无单位十进制 → 直接；带单位（`pt in cm mm pc pi`）→ 换算到半点后四舍五入 | 无单位整数 |
| `Twips` (`ST_TwipsMeasure`/`ST_SignedTwipsMeasure`) | 无单位十进制 → 直接（允许负号）；带单位 → 1in=1440，1pt=20，1pc=1pi=240，1cm=566.93，1mm=56.69，四舍五入 | 无单位整数 |
| `EighthPoints` | 同 `Twips` 规则，单位换算到 1/8 pt | 无单位整数 |
| `HexColorOrAuto` | `auto` → `Auto`；6 位 hex（容忍前导 `#`、大小写）→ `Rgb`；其他 → `Raw(text)` + 诊断 | 6 位大写 hex 或 `auto` |
| `Hex2` | 2 位 hex（tint/shade） | 2 位大写 hex |
| `Percent` (`ST_TextScale` 等) | 整数或 `NN%` | 整数 |
| `Enum<E>` | 匹配枚举成员；不匹配 → `Raw(text)` + 诊断，**不**丢失 | 枚举字面 |
| `Str` | 原文 | 原文（转义） |
| `Int`/`UInt` | 十进制 | 十进制 |
| `MeasureOrPercent` (`ST_MeasurementOrPercent`，`CT_TblWidth/@w:w`) | 无单位十进制 → `Number`；带单位 → 换算 twips 的 `Number`；`NN%` 字面 → `Percent`（1/100 百分点）；单位由同元素 `w:type` 决定 | `Number` 写整数，`Percent` 写 `NN%` |
| `Raw` | 整个元素按 DOM 保留 | 原字节 |

Strict 下 `ST_OnOff` 只接受 `true/false/1/0`；解析时两族都接受，生成按 `PartFlavor`。

## PROP-03 Cs 孪生

`w:b/w:bCs`、`w:i/w:iCs`、`w:sz/w:szCs` 各自独立建模（两个字段），不做交叉回退。选择哪一组由 `resolve`（`RES-06`）按 run 的 rtl 状态决定，属性表本身不选择。

## PROP-04 三态

所有 `OnOff` 字段为 `Option<bool>`：`None` = 文档未声明（继承）、`Some(false)` = 显式关闭（覆盖样式）、`Some(true)` = 显式开启。TS 中 `boolProp`（两态）与 `onOffOf`（三态）不一致的问题由此消除；`keepNext`、`keepLines`、`widowControl`、`snapToGrid`、`contextualSpacing`、`pageBreakBefore`、`bidi` 等全部三态。

## PROP-05 schema 顺序

以下顺序来自 ECMA-376 Part 1 的 XSD（Transitional），生成新元素时按此插入；用 XSD 生成并与 LibreOffice `docxattributeoutput.cxx` 输出交叉校验（`docs/03` 第 14 节风险项）。

**CT_PPr**（`CT_PPrBase` 为 sequence，顺序强制）：
`pStyle, keepNext, keepLines, pageBreakBefore, framePr, widowControl, numPr, suppressLineNumbers, pBdr, shd, tabs, suppressAutoHyphens, kinsoku, wordWrap, overflowPunct, topLinePunct, autoSpaceDE, autoSpaceDN, bidi, adjustRightInd, snapToGrid, spacing, ind, contextualSpacing, mirrorIndents, suppressOverlap, jc, textDirection, textAlignment, textboxTightWrap, outlineLvl, divId, cnfStyle, rPr, sectPr, pPrChange`

**CT_RPr**（`EG_RPrBase` 是 choice 组，schema 不强制顺序；本引擎按 Word 输出顺序生成）：
`rStyle, rFonts, b, bCs, i, iCs, caps, smallCaps, strike, dstrike, outline, shadow, emboss, imprint, noProof, snapToGrid, vanish, webHidden, color, spacing, w, kern, position, sz, szCs, highlight, u, effect, bdr, shd, fitText, vertAlign, rtl, cs, em, lang, eastAsianLayout, specVanish, oMath`，随后 `w14:glow, w14:shadow, w14:reflection, w14:textOutline, w14:textFill, w14:scene3d, w14:props3d, w14:ligatures, w14:numForm, w14:numSpacing, w14:stylisticSets, w14:cntxtAlts`（Word 实际输出位置，待语料校验），最后 `rPrChange`。段落标记 rPr（`pPr/rPr`）同表，另允许 `ins, del, moveFrom, moveTo` 在最前。

**CT_TcPr**：`cnfStyle, tcW, gridSpan, hMerge, vMerge, tcBorders, shd, noWrap, tcMar, textDirection, tcFitText, vAlign, hideMark, headers, cellIns, cellDel, cellMerge, tcPrChange`

**CT_TblPr**：`tblStyle, tblpPr, tblOverlap, bidiVisual, tblStyleRowBandSize, tblStyleColBandSize, tblW, jc, tblCellSpacing, tblInd, tblBorders, shd, tblLayout, tblCellMar, tblLook, tblCaption, tblDescription, tblPrChange`

**CT_TrPr**（choice 组；按 Word 顺序）：`cnfStyle, divId, gridBefore, gridAfter, wBefore, wAfter, cantSplit, trHeight, tblHeader, tblCellSpacing, jc, hidden, ins, del, trPrChange`

**CT_TblPrEx**（`w:tr` 的第一个子元素，行级表格属性例外；复用 `TableProps` 表读取，缺的字段为 `None`）：`tblW, jc, tblCellSpacing, tblInd, tblBorders, shd, tblLayout, tblCellMar, tblLook, tblPrExChange`

**`w:tr` / `w:tc` / `w:tbl` 子元素顺序**（不是属性容器，但新建容器时要按它插入）：`w:tbl`: `tblPr, tblGrid, tr*`；`w:tr`: `tblPrEx?, trPr?, tc*`；`w:tc`: `tcPr?, (p | tbl | sdt | …)+`，末尾必须是 `w:p`。

**CT_SectPr**：`headerReference/footerReference`（0–6 个，顺序任意），然后 `footnotePr, endnotePr, type, pgSz, pgMar, paperSrc, pgBorders, lnNumType, pgNumType, cols, formProt, vAlign, noEndnote, titlePg, textDirection, bidi, rtlGutter, docGrid, printerSettings, sectPrChange`

**边框容器**：`CT_TblBorders`: `top, start|left, bottom, end|right, insideH, insideV`；`CT_TcBorders`: 同上加 `tl2br, tr2bl`；`CT_PBdr`: `top, left, bottom, right, between, bar`。

**复合属性元素的属性**（顺序不强制，按 Word 习惯生成）：`w:spacing`: `before beforeLines beforeAutospacing after afterLines afterAutospacing line lineRule`；`w:ind`: `start|left startChars end|right endChars hanging hangingChars firstLine firstLineChars`；`w:rFonts`: `hint ascii hAnsi eastAsia cs asciiTheme hAnsiTheme eastAsiaTheme cstheme`；`w:numPr` 子元素 `ilvl, numId, numberingChange, ins`。

`start/end` 与 `left/right` 是同义对（Strict 只允许 start/end）：解析两者都接受，生成按 flavor（Transitional 写 `left/right`，Strict 写 `start/end`）。

## PROP-06 合并写回

输入：属性容器节点 `C`（可能不存在）、patch（每个建模字段 `Set(value) | Unset | Keep`）。算法：

1. `C` 不存在且 patch 全 `Keep` → 无操作。`C` 不存在且有 `Set` → 在父节点按父容器的 schema 顺序插入 `New` 容器（如 `w:pPr` 必为 `w:p` 第一个子元素；`w:rPr` 必为 `w:r` 第一个子元素）。
2. 对每个 `Set(v)`：容器内已存在该字段元素 → 将该元素替换为按 codec 生成的新元素（旧元素 `Deleted`，新元素 `New` 插在原位置）；不存在 → 按 `order` 找到第一个 `order` 更大的现存子元素，插在它之前；没有更大的 → 追加到末尾（但在 `*PrChange` 之前）。
3. 对每个 `Unset`：存在 → `Deleted`。
4. 未建模子元素、`Keep` 字段：不动。
5. 容器 `C` 自身只因子列表变化而变 `DescendantDirty`（`XML-12` 规则 B），其开闭标签字节保持。
6. `multi` 字段（如 `w:tabs/w:tab`）作为整体子容器处理：patch 给出完整新列表时替换整个子容器。

修订快照（`in_change`）：`rPrChange/pPrChange` 内的旧属性由同一读取函数解析，得到 `RunProps`/`ParaProps` 的旧值。

## PROP-07 生成的 API

对每张表生成：

```rust
pub struct XxxProps { pub field: Option<Codec::Value>, ... , pub raw_unmodeled: Vec<NodeId> }
pub fn read_xxx(dom: &Dom, container: Option<NodeId>) -> XxxProps
pub fn diff_xxx(a: &XxxProps, b: &XxxProps) -> XxxPatch
pub fn plan_apply_xxx(dom: &Dom, parent: NodeId, container: Option<NodeId>, patch: &XxxPatch, flavor: PartFlavor) -> Vec<NodeEdit>   // 不修改 DOM，产出 MutationPlan 片段
pub fn order_index_xxx(name: &QName) -> Option<u16>
```

`plan_apply_*` 只产出计划（`EDIT-05` 原子性），由 `commit` 执行。

## PROP-08 第一批建模字段

与 TS 解析器建模范围对齐（差分测试需要），其余保留为 `Raw`：

- `RunProps`：`rStyle, rFonts(全部 9 个属性), b, bCs, i, iCs, caps, smallCaps, strike, dstrike, vanish, color(val/themeColor/themeTint/themeShade), spacing, w, kern, position, sz, szCs, highlight, u(val/color/themeColor), shd(val/color/fill/themeFill), vertAlign, rtl, cs, em, lang(val/eastAsia/bidi), specVanish`；`w14:textFill` 读为显示用颜色近似（`RES-05`）但写回保持 `Raw`。
- `ParaProps`：`pStyle, keepNext, keepLines, pageBreakBefore, framePr(全部属性), widowControl, numPr(ilvl/numId), pBdr(六边), shd, tabs, autoSpaceDE, autoSpaceDN, bidi, snapToGrid, spacing(全部属性), ind(全部属性), contextualSpacing, jc, outlineLvl, rPr(段落标记，嵌套 RunProps)`。
- `CellProps`（`cell.toml`，任务 3.1）：`cnfStyle, tcW, gridSpan, hMerge, vMerge（元素存在无 val = continue）, tcBorders（8 边，start/end 的 Transitional 拼写 left/right）, shd, noWrap, tcMar, textDirection, tcFitText, vAlign, hideMark, headers（Raw）, cellIns, cellDel, cellMerge`；`tblLayout` 一类属性在 `w:type` 而非 `w:val` 的元素建成 struct。
- `TableProps`（`table.toml`）：`tblStyle, tblpPr（全部属性）, tblOverlap, bidiVisual, tblStyleRowBandSize, tblStyleColBandSize, tblW, jc, tblCellSpacing, tblInd, tblBorders（6 边）, shd, tblLayout, tblCellMar, tblLook（val + 6 个开关）, tblCaption, tblDescription`；同一张表读 `w:tblPrEx`。表格样式（`styles.toml`）的 `tblPr / trPr / tcPr` 用这三张表，不再是 `Raw`。
- `RowProps`（`row.toml`）：`cnfStyle, gridBefore, gridAfter, wBefore, wAfter, cantSplit, trHeight, tblHeader, tblCellSpacing, jc, hidden, ins, del`（`ins/del` 与单元格修订标记都不进 `*PrChange` 快照）。
- `SectionProps`：`headerReference*, footerReference*, footnotePr, endnotePr, type, pgSz, pgMar, pgBorders, lnNumType, pgNumType, cols(含 col 子元素), formProt, vAlign, titlePg, textDirection, bidi, rtlGutter, docGrid`。

## PROP-09 值保真

枚举与颜色解析失败时**禁止**丢值：保留 `Raw(text)` 并记诊断；比较时按原文比较；写回按原文。

## 验收清单

| ID | 用例 |
| --- | --- |
| PROP-02 | `<w:sz w:val="12pt"/>` → 24；`<w:ind w:left="1in"/>` → 1440；`<w:b w:val="off"/>` → Some(false)；Strict 生成 `w:val="false"` |
| PROP-04 | 段落 `<w:keepNext w:val="0"/>` 读为 Some(false)，样式为 true 时 resolve 结果为 false |
| PROP-05 | 向只有 `w:jc` 的 pPr 加 `w:spacing` → 插在 `w:jc` 之前；向只有 `w:sz` 的 rPr 加 `w:b` → 插在 `w:sz` 之前 |
| PROP-06 | 改 `w:color` 后 rPr 中未建模的 `w:bdr` 原字节保留且位置不变；rPr 自身开标签字节不变 |
| PROP-07 | 对每张表每一行：构造含该字段的容器 → read → plan_apply(Set 同值) 得到空计划；plan_apply(Set 新值) → commit → read 得新值 |
| PROP-09 | `<w:jc w:val="weird"/>` 读为 Raw 且写回原文 |
