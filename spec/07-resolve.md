# SPEC 07 · 有效属性视图（resolve/）

对应 `docs/03` 第 7 节。职责：从声明值（模型）计算编辑器需要的有效值，带来源；不修改规范状态，不做排版。每条规则配"规范依据"与"校准要求"（`TEST-08`）。

## RES-01 输出形态

```
Effective<T> { value: T, source: Provenance }
Provenance = Direct | CharStyle(StyleId) | ParaStyle(StyleId) | NumberingLevel{num_id, ilvl} | TableStyle{style, cond: Option<CondType>} | DocDefaults | Theme | Default
```

`resolve::run(...) -> EffectiveRunProps`、`resolve::para(...)`、`resolve::cell(...)`、`resolve::sections(...)`、`resolve::numbering(...)`。结果可缓存，缓存键含样式表版本号；样式/编号 part 变脏时整体失效。

## RES-02 样式查找

- **默认样式**（ECMA-376 §17.7.4.17）：每个 `w:type` 中最后一个 `w:default="1|true"` 的样式为默认；该类型无声明 → 该类型第一个样式。未指定 `pStyle` 的段落用默认段落样式；未指定 `rStyle` 的 run 不隐含默认字符样式（默认字符样式只提供 docDefaults 之上的一层，Word 行为：`DefaultParagraphFont` 通常为空）。
- **basedOn 链**：从样式沿 `based_on` 向上，遇到已访问的 id 停止并记诊断 `RES_STYLE_CYCLE`；缺失的 based_on 目标视为链结束。类型必须一致（段落样式只能基于段落样式），不一致时忽略该 based_on 并记诊断。
- **linked**：段落样式与字符样式经 `w:link` 互指时，字符侧的 rPr 缺失项从段落侧补（TS 行为）；`resolve::run` 对 `rStyle` 指向 linked 字符样式时按其 rPr 链处理。
- **heading_level**：样式名匹配 `/^heading\s*([1-9])$/i` 或 id 匹配 `^Heading([1-9])$`；否则样式 pPr 的 `outlineLvl` 0–8 → +1；`outlineLvl == 9` 阻断 basedOn 继承；否则继承 basedOn。

## RES-03 run 有效属性

非 toggle 属性按以下顺序覆盖（后者覆盖前者，`None` 不覆盖）：

1. `doc_defaults.rpr`
2. 段落样式链（从根到叶）的 `rpr`
3. 编号级别 `rpr`（**仅**用于列表标记本身，不用于正文 run）
4. 表格样式的 rPr（整表 → 条件格式，`RES-08`）（仅表格内段落）
5. 字符样式链（从根到叶）的 `rpr`
6. run 直接 `rpr`

字体解析（`RES-05`）与 Cs 选择（`RES-06`）在覆盖之后进行。

## RES-04 toggle 属性

toggle 属性：`b bCs i iCs caps smallCaps strike dstrike outline shadow emboss imprint vanish`（ECMA-376 §17.7.3；`specVanish` 不是 toggle）。

**不采用** `child ?? parent` 合并。**规则按真实 Word 实测定案**（2026-09-06，Word 网页版，六份 fixture，记录在 `fixtures/resolve/README.md`）：

1. run 直接格式指定 → 用直接值（实测与规范一致）。
2. 否则：

   ```text
   有效值 = docDefaults ⊕ 段落样式层 ⊕ 表格样式层 ⊕ 字符样式层
   ```

   每个**层级**先按"子覆盖父"取一个值（`basedOn` 链内**不**计次数），层级之间才做异或。
   段落样式层在链里一处都没声明时**取 docDefaults 的值**——每个段落都有样式（没写 `w:pStyle` 就是 Normal），样式链的根是 docDefaults，于是 docDefaults 的值在两处各出现一次、自己抵消。
3. **与 ECMA-376 §17.7.3 的差异**：规范说的是"层级各样式中值为 true 的次数"，实测是"层级数"——`basedOn` 链上两层都 `b=true` 时 Word 仍然加粗。这属于 [MS-OI29500] 记录的 Word 偏差一类（该文档也记了 docDefaults、表格样式、多层 basedOn 的处理与 Word 版本相关）。
4. **只测了 `b`**，其余八个 toggle 按同一规则处理；`docDefaults=true` 且段落样式显式 `w:val="0"` 这个角没有实测。两条都记在 `fixtures/resolve/README.md` 的"还没测到的角"。

**来源（`RES-01`）**：toggle 的有效值可能由多个层级异或得出，那个值谁都没单独写过，所以来源是 `Provenance::Toggle { levels }`（`levels` 按最具体到最不具体列出参与的层）。只有一个层级参与、且有效值就是它写的那个值时才指那一层；直接格式一票定音时是 `Direct`。"只有 docDefaults 声明"也落到 `Toggle`——段落样式层会把 docDefaults 的值再贡献一次。

**实现**（任务 5.8）：`resolve::toggle::resolve_toggle(rule, &ToggleLayers { direct, char_chain, table, para_chain, doc_default })`，规则由 `ToggleRule` 参数化。激活的是 `WordObserved`（上面那条实测规则）；`MostSpecificWins`（最具体胜出，M1 起的行为，也是 TS `display` 的行为）与 `OddParity`（规范字面）保留在枚举里备查，各有单测。层叠（`Resolver::run_in_table`）按层把各层声明喂给它，所以换规则只改 `ACTIVE_TOGGLE_RULE` 一行。九个 toggle 字段的枚举、读写与常量由 `toggle_fields!` 一张表展开。

**校准已完成**：`fixtures/resolve/toggle/*` 五份 + `fixtures/resolve/sections/*` 一份，六份最小 docx 由 `cargo run -p gen-fixtures` 生成，观察值来自真实 Word，`expected.toml` 全部 `verified = true`，`tests/resolve_fixtures.rs` 全绿。原来激活的 `MostSpecificWins` 在六份里错了三份。

**影响面**：这条规则只作用于 `resolve` 这个公开只读视图。`bind/compat_ts` 的 `runs[].bold` 发的是 run 自己 `w:rPr` 的声明值（复现 TS 的形态），不走 `Resolver::run`；`tests/resolve.rs` 的 `StyleDisplay` 比的是每个样式自己的链合并，也不走 toggle 规则。所以换规则后五道差分门与保存语料的数字一个没变——语料并不覆盖这条路径（语料里 `docDefaults` 带 `w:b` 的文档为 0 份）。

## RES-05 主题字体、颜色与符号字体

- **rFonts**（ECMA-376 §17.3.2.26：主题属性覆盖同槽字面值）：`asciiTheme/hAnsiTheme` 的 `major*` → 主题 major latin，`minor*` → minor latin；`eastAsiaTheme` → major/minor ea；`cstheme` → major/minor cs。解析不到 → 字面值。**空 EA 槽**（有 `eastAsiaTheme`、主题存在、槽 typeface 为空）→ 按 `settings/themeFontLang/@eastAsia`：`ja` → Yu Gothic(major)/Yu Mincho(minor)，`ko` → Malgun Gothic，其他 → DengXian；并标 `ea_slot_empty`。主题 `a:font script="..."` 表（Jpan/Hang/Hans/Hant…）在 `themeFontLang` 命中时优先于以上缺省。
- **docDefaults 的 EA 字体**：EA 槽空且 `rPrDefault/w:lang/@eastAsia` 存在 → `ko*` → Malgun Gothic，`ja*` → MS Mincho，`zh-cn` → SimSun，`zh-tw/zh-hk` → PMingLiU。
- **显示字体选择**（TS 兼容视图）：`font = eastAsia ?? ascii ?? hAnsi`，`font_ascii = ascii ?? hAnsi`，`cs_font` 独立；`w:hint="eastAsia"` 影响标点归属，仅记录。
- **颜色**：`w:color/@themeColor` 存在且主题存在 → `resolve_theme_color(slot, tint, shade)`：槽位映射 `dark1/text1→dk1, light1/background1→lt1, dark2/text2→dk2, light2/background2→lt2, accentN, hyperlink→hlink, followedHyperlink→folHlink`；dk1/lt1 缺省 `000000/FFFFFF`；先 shade（`c*s/255`）再 tint（`c*t/255 + 255*(1-t/255)`）。否则 `w:val`（`auto` → 视为未指定，由渲染器决定）。`w14:textFill` 只用于显示近似（solid 直取；gradFill 各 stop 等权平均），不改变声明。
- **DrawingML 颜色**（形状显示模型）：按 `oox::drawingml::Color` 的变换顺序在规范要求的色彩空间实现 `lumMod/lumOff/tint/shade/satMod/hueMod/alpha`，输出 sRGB；`schemeClr` 别名 `tx1→dk1, bg1→lt1, tx2→dk2, bg2→lt2`。
- **符号字体**：`Symbol`、`Wingdings`、`Wingdings 2`、`Wingdings 3`、`Webdings` 视为符号字体；`w:sym/@w:char` 与符号字体 run 的字符按 TS `symbol-fonts.ts` 的映射表解码为 Unicode 供显示；`0xF000–0xF0FF` 先减 `0xF000`。解码失败保留原字符。

## RES-06 rtl 与 Cs 选择

run 的 `cs` 状态 = 直接 `w:rtl` ?? 字符样式链 `rtl` ?? 段落样式链 `rtl` ?? false。`cs == true` → `bold/italic/size` 读 `bCs/iCs/szCs`（各层同样规则），`cs == false` → 读 `b/i/sz`；**无交叉回退**（Word for Mac 实测，`docs/01` 6.6）。段落 `bidi` 与文本脚本不参与。

## RES-07 段落有效属性

覆盖顺序：`doc_defaults.ppr` → 段落样式链 → 编号级别 `ppr`（仅 `ind`，且仅当段落自身无 `ind`）→ 表格样式 pPr（表格内）→ 直接 `ppr`。

- `jc`：`start→left, end→right, both→justify`；`bidi` 段落的 `left/right` 互换为视觉值（模型存逻辑值）。
- `spacing`：`beforeAutospacing/afterAutospacing` 为 true 时忽略字面 `before/after`（视为未指定）；`line>0` 且 `lineRule=auto` → 倍数 `line/240`；`line=0 && lineRule=atLeast` → 标记"退出 docGrid 吸附"。
- 缺省：`widowControl=true`、`snapToGrid=true`、`autoSpaceDE/DN=true`、`adjustRightInd=true`。
- `autoSpace` 视图：DE 与 DN 都为 false → false。
- `pBdr`：`nil/none` 视为无边；重复 `pBdr` 容器按边后者胜。
- 空段落度量来源：段落标记 `pPr/rPr` 的 `sz`/`rFonts`，否则最后一个空 run 的 rPr。

## RES-08 表格有效属性

- `tblLook`：属性形式（`firstRow lastRow firstColumn lastColumn noHBand noVBand`，`0|false` 为关）优先；否则 `w:val` 位掩码 `0x20 firstRow, 0x40 lastRow, 0x80 firstColumn, 0x100 lastColumn, 0x200 noHBand, 0x400 noVBand`；缺省等价于 `w:val="04A0"`（firstRow / firstColumn 开、noVBand 开，即横向条带开、纵向条带关）。
- **重复声明**：一般属性元素取第一个（属性表通则）；`w:tcW` 取**最后一个**（Word 与 TS 的规则，生成器会留下过时的首个值）；`w:tblBorders` / `w:tcBorders` 容器重复出现时按边合并、后者胜（同 `RES-07` 对 `pBdr` 的规则）。这三条都只在视图里生效，模型保持声明值。
- 条件格式优先级（Word）：`firstRow > lastRow > firstCol > lastCol > 条带（band1Horz/band2Horz，行号从 firstRow 之后起算）> 整表`；单元格自身声明优先于一切。
- 表格样式链：`tblStyle` 的 basedOn 链；`tblPr/tblBorders`、`tblCellMar` 文档未声明时回退样式。
- 单元格边距缺省：上下 0、左右 108 twips。
- **列宽视图** `ColumnView { widths_twips: Vec<i32>, source: Grid | TcW | Stretched | Reconciled, spans: Vec<Vec<u16>> /* 每行每格占的列数 */, gaps: Vec<(u16, u16)> /* 每行 gridBefore / gridAfter */ }`：按 TS 顺序应用四条启发式并标 `source`——① `grid = tblGrid`（总和 > 0 才有；全部 > 0 才有 twips）；② `tcwColumnWidths`（每行从 `gridBefore` 起算，未跨列格的**最后一个** dxa `tcW` 每列取最大，须每列都有值）与 grid 不一致（列数不同 / 任一列相差 > 2 个百分点 / fixed 布局且总和差 > 列数）→ 以 tcW 为准；③ 非 fixed 且 grid 总和 < `tblW dxa` − 列数 → 按比例拉伸到 `tblW`；④ 各行 gridSpan 总和不等 → `reconcileGridColumns`（每行累计右边界取并集、容差内吸附、> 96 个边界放弃）重算列数与各格跨度。`tblW pct` 优先于绝对宽。全部是显示层规则，**不改模型、不写回**。
- `Resolver::table(&TableBlock) -> TableView`；`TableView::cell(r, c) -> EffectiveCellProps`（底纹、8 边边框、边距、`vAlign`、`textDirection`、条件 rPr / pPr 叠加），每项带 `Provenance::TableStyle{style, cond}`；`Row.tbl_pr_ex` 在该行优先于 `tblPr`。
- `hMerge continue` 折叠到左侧单元格的 `colSpan`。
- `trHeight` 上限 31680。

## RES-09 编号

- 级别查找：`num → abstractNum`；`abstractNum.num_style_link → 样式.numPr.numId → 该 num 的 abstractNum`（沿 `style_link` 匹配），防环；`num.overrides[ilvl]` 覆盖 `start`/整级。
- 标记计算 `resolve::numbering::markers(items: &[ListRef]) -> Vec<Option<Marker>>`（TS `list-markers.ts` 语义）：计数器按 `abstractNumId` 全文累积；出现某级时更深级清零；`startOverride` 在该 `numId:ilvl` 首次出现时生效一次；`lvlRestart`（`w:lvlRestart w:val=N`：当第 N 级出现时重置，0 = 永不重置）；`isLgl`：`lvlText` 中更高层引用按 decimal 渲染；`lvlText` 的 `%n` 用第 n-1 级计数经 `format_number(numFmt, customFormat)`；bullet：符号字体解码 → 常见 PUA 映射 → 缺省项目符号表；`numFmt none` → 空。
- `numId 0` → 无编号；`ilvl` 缺省 0；级别缺失 → 无标记。

## RES-10 节

- 节序列由 `SectionInfo` 顺序给出；每节 `headerReference/footerReference` 缺失的 `type`（default/first/even）**继承上一节**的同类型引用（Word "链接到前一节"）；第一节缺失 → 无。三个变体**各自**继承（不是整组继承）。
- `Resolver::section(sections, idx) -> EffectiveSection`：六个槽（kind × variant）各是 `HfSlot::Absent | Declared(rId) | Inherited { from, id }`。`Declared` 与 `Inherited` 的区别是 `SetHeaderFooter`（`EDIT-03`）改写 part 还是新建 part 的分界。
- `titlePg` 为该节属性；`evenAndOddHeaders` 为文档属性；有效页眉选择：首页且 `titlePg` → first；偶数页且 `evenAndOddHeaders` → even；否则 default。选中的变体为空时**禁止**回退 default——Word 里"首页不同"而没有首页页眉就是首页没有页眉。
- `w:type` 缺省 `nextPage`；第一节的 type 无意义。
- `section_of(node) -> SectionIdx`：节点所属节 = 第一个 `sectPr` 在其之后（文档序）的节。

## RES-11 SDT 与内容控件

`resolve` 不涉及 sdt；编辑策略在 `EDIT-03`。

## RES-12 校准 fixture 格式

```
fixtures/resolve/<area>/<case>/
  doc.docx            # 真实 Word 文档（尽量最小）
  expected.toml       # [[run]] para = 3, run = 1, bold = true, source = "ParaStyle:Heading1" ...
  README.md           # Word 版本、观察方法（截图/属性面板）、来源
```

每条 `RES-*` 至少一个 fixture；`RES-04` 至少五个。

## 验收清单

| ID | 用例 |
| --- | --- |
| RES-02 | basedOn 环不死循环；最后一个 default 胜出；`outlineLvl 9` 阻断 |
| RES-04 | 五个 toggle fixture |
| RES-05 | 空 EA 槽 + `themeFontLang ja` → Yu Mincho；`themeColor accent1 + tint 99` 与 Word 显示一致（允许 ±1/255 误差） |
| RES-06 | `w:rtl` run 只读 `bCs`，`w:b` 被忽略 |
| RES-08 | `tblLook w:val="04A0"` 解出 firstRow / firstColumn / noVBand（= 横向条带开、纵向条带关），属性形式优先于位；重复 `w:tcW` 取最后一个；重复 `tblBorders` / `tcBorders` 按边合并后者胜；`trHeight` 截到 31680；全语料的列宽与格跨度与 TS 一致 |
| RES-09 | `lvlRestart=0` 的级别不重置；`isLgl` 的 `%1.%2` 中 %1 为 decimal |
| RES-10 | 第二节无 header 引用时继承第一节 |
