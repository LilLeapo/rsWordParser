# SPEC 04 · 字段子系统（span/field）

对应 `docs/03` 第 5.4 节。职责：把 `w:fldChar` begin/separate/end 与 `w:fldSimple` 识别为字段区间，解析指令关键字，按策略表决定显示形态、可编辑性与保存行为。字段是 Span 层的子系统，依赖 `03-span.md` 的内容序列与 `FlowId`。

## FLD-01 两种形式

- **复杂字段**：三个含 `w:fldChar` 的 `w:r`（`w:fldCharType` = `begin` | `separate` | `end`），`separate` 可缺省（无结果的字段，如 XE）。指令在 begin 与 separate 之间的 `w:instrText`；结果在 separate 与 end 之间。
- **简单字段**：`w:fldSimple[@w:instr]`，子节点即结果；可嵌套 `w:fldSimple`。
- 两者统一为 `FieldSpan { form: Complex{begin, separate, end, instr_nodes, result_nodes} | Simple{node} }`（`docs/03` 5.4）。

## FLD-02 构建

对每个内容流，按语义遍历顺序访问所有 `w:r`（包括位于 `w:hyperlink w:ins w:del w:moveFrom w:moveTo w:sdt/w:sdtContent w:smartTag w:customXml w:dir w:bdo` 内部的 run），维护栈：

1. `begin` → 压栈 `Open{begin_run, instr: [], separate: None, result: []}`。
2. `separate` → 栈顶 `separate = Some(run)`；栈空 → 诊断 `FLD_STRAY_SEPARATE`，忽略。
3. `end` → 弹栈生成 `FieldSpan`；栈空 → 诊断 `FLD_STRAY_END`，忽略。弹出的字段若栈非空，则成为新栈顶的 `nested`（位于其指令区或结果区，按 `separate` 是否已出现判断）。
4. 非 fldChar 的 run：栈顶存在时，按 `separate` 是否已出现归入 `instr_nodes` 或 `result_nodes`。
5. `w:fldSimple` → 直接生成 `Simple` 字段；其子 run 归入结果；子 `fldSimple` 为 nested。
6. 流结束时栈非空 → 每个未闭合字段记诊断 `FLD_UNCLOSED`，其 begin 所在段落及之后至流末的段落**不**归入字段；该 begin run 视为普通 run 内容（保存原字节）。
7. 字段**禁止**跨内容流配对：`w:txbxContent` 内的 fldChar 只与同一 txbxContent 内的配对。

字段跨越段落（begin 与 end 的最近 `w:p` 祖先不同）→ `cross_paragraph = true`。跨表格单元格同理。

## FLD-03 指令文本

- `instr.raw` = `instr_nodes` 中所有 `w:instrText` 文本按顺序拼接（`w:instrText` 视为 `xml:space="preserve"`，不 trim）。
- `w:delInstrText`：拼入 `raw` 但标记 `instr_deleted_parts`；含它的字段在模型中带 `Revision::FieldInstrDelete`（`MOD-09`），策略照常。
- nested 字段在 `raw` 中以占位 `\u{FFFC}` 出现，`tokens` 中为 `InstrToken::Nested(id)`。

## FLD-04 fldChar 与 ffData

- begin run 的 `w:fldChar` 属性：`w:fldLock`（`lock`）、`w:dirty`（`dirty_flag`）。
- begin `w:fldChar` 的子元素：`w:fldData`（原样保留）、`w:ffData`（表单字段定义，见 `FLD-10`）、`w:numberingChange`（原样保留）。
- `separate`/`end` 的 fldChar 属性原样保留。

## FLD-05 指令 tokenization

只做 tokenization 与关键字提取，不解释语义。

```
instruction := ws* keyword (ws+ item)* ws*
keyword     := '=' | ident                 // '=' 为公式字段；ident 不区分大小写，规范化为大写
item        := switch | argument
switch      := '\' switch-char argument?   // switch-char: 字母（不区分大小写）或 '*' '#' '@' '!'
argument    := quoted | bare | NESTED
quoted      := '"' ( '\' any | [^"\\] )* '"'   // \" 与 \\ 为转义
bare        := [^ \t\r\n"\\]+
NESTED      := U+FFFC                      // 嵌套字段占位
```

- 通用格式开关：`\*`（`MERGEFORMAT`、`CHARFORMAT`、`Upper`、`Lower`、`FirstCap`、`Caps`、`Ordinal`、`Roman`、`roman`、`Arabic`、`ALPHABETIC`、`alphabetic`、`CardText`、`DollarText`、`Hex`、`OrdText`）、`\#` 数字格式、`\@` 日期格式、`\!` 锁定结果。记为 `InstrToken::GeneralFormat`。
- 其他 `\x` 记为 `InstrToken::Switch { name, arg }`，`arg` 为紧随的 argument（若下一个 item 不是 switch）。
- tokenizer **不得**因任何输入失败：无法识别的字符归入 `bare`。
- `raw` 与 `instr_nodes` 是保存真相；`tokens` 只是视图。

## FLD-06 关键字 → 策略

未命中 → `Unknown`。关键字比较不区分大小写。

| 策略 | 关键字 |
| --- | --- |
| `Marker` | XE, TA, TC, RD, PRIVATE, SET |
| `Atom` | PAGE, NUMPAGES, SECTION, SECTIONPAGES, NUMWORDS, NUMCHARS, DATE, TIME, CREATEDATE, SAVEDATE, PRINTDATE, EDITTIME, AUTHOR, TITLE, SUBJECT, KEYWORDS, COMMENTS, LASTSAVEDBY, FILENAME, FILESIZE, TEMPLATE, DOCPROPERTY, DOCVARIABLE, USERNAME, USERINITIALS, USERADDRESS, SEQ, STYLEREF, PAGEREF, NOTEREF, REF, QUOTE, SYMBOL, LISTNUM, AUTONUM, AUTONUMLGL, AUTONUMOUT, REVNUM, INFO, `=`, MERGEFIELD, MERGEREC, MERGESEQ, NEXT, NEXTIF, SKIPIF, IF, COMPARE, ADVANCE, EQ, GOTOBUTTON, MACROBUTTON, CITATION, FILLIN, ASK, GREETINGLINE, ADDRESSBLOCK, AUTOTEXT, AUTOTEXTLIST, BIDIOUTLINE |
| `Link` | HYPERLINK |
| `Form` | FORMCHECKBOX, FORMTEXT, FORMDROPDOWN |
| `Picture` | INCLUDEPICTURE |
| `Object` | EMBED, LINK |
| `Block` | TOC, INDEX, BIBLIOGRAPHY, INCLUDETEXT, DATABASE |

覆盖规则：`cross_paragraph == true` → 无论关键字一律 `Block`。表外关键字（ECMA-376 §17.16.5 全集之外、厂商私有）→ `Unknown`。

## FLD-07 策略语义

字段在模型中有三种形态（`MOD-06`）：**原子形态**（`Inline::Field { id, result: Vec<Inline> }`，坐标流中 1 个 `U+FFFC`）、**透明形态**（结果 run 直接出现在 inlines 中并带 `field: Some(id)`，结构 run 贡献 0 长度）、**块形态**（结果段落被标记）。

| 策略 | 形态 | 显示 | 编辑 | 保存 |
| --- | --- | --- | --- | --- |
| `Marker` | 原子（不可见） | 零宽；编辑器可不渲染，但占 1 个坐标单位 | 随文本移动/删除；`DeleteRange` 覆盖它即删除 begin..end | 干净则原字节；段落重生成不影响其节点 |
| `Atom` | 原子 | 显示 `result` 中的 run（保留多 run 格式） | `SetFieldResultProps` 改结果 run 的 rPr；`DeleteRange` 覆盖即删除 begin..end；**禁止**光标进入 | 干净则原字节；改格式只重生成被改的结果 run |
| `Link` | 透明 | 结果 run 显示为链接，目标来自指令（`HYPERLINK "url"`、`\l anchor`、`\o tooltip`、`\t target`） | 结果 run 可正常编辑；`SetLinkTarget` 改写指令 run（`instr_nodes` 中的 `w:instrText` 文本） | 结构 run 干净则原字节；不转换为 `w:hyperlink` |
| `Form` | 原子 | FORMCHECKBOX：`☐/☒`（`FLD-10`）；FORMTEXT：结果文本；FORMDROPDOWN：`ddList/result` 指向的 `listEntry` | `ToggleCheckbox` 改 `ffData/checkBox/checked`；`SetFormText` 改结果 run 文本；下拉改 `ffData/ddList/result` | 重生成 begin run（含 ffData）与被改的结果 run |
| `Picture` | 原子 | 结果中的图片（`INCLUDEPICTURE` 的结果是 `w:drawing`/`w:pict`） | 同图片原子 | 原字节 |
| `Object` | 原子 | 结果中的 OLE 预览 | 同对象原子 | 原字节 |
| `Block` | 块 | 结果段落各自按普通段落解析用于显示，但块标 `FieldBlockResult` | 结果段落只读；`UpdateBlockField` 由生成器重算 | 干净则原字节；更新时替换 separate..end 之间的全部节点 |
| `Unknown` | 原子 | 同 `Atom` | 同 `Atom` | 原字节 |

补充：

- `lock == true` 的字段：`UpdateBlockField` 拒绝并返回 `FLD_LOCKED`；其他编辑照常。
- 结果为空的 `Atom`（无 separate 或结果无内容）显示为空原子，仍占 1 单位。
- 原子形态的字段其 `props`（用于新输入继承格式）取 begin run 的 rPr。

## FLD-08 跨段字段与块标记

- 对 `Block` 字段：begin 所在段落为**头段**，end 所在段落为**尾段**，两者及其间所有段落（含表格内的）在 `ParagraphFacts.inside_field_result = Some(id)`，分类为 `Protected(FieldBlockResult(id))`（`MOD-05` R09）。
- 头段中 begin 之前、尾段中 end 之后若有普通内容（例如 TOC 尾段带分页符），仍属该保护块；`compat_ts` 复现 TS 的"字段结束标记 + 分页"标签。
- Block 字段常被 `w:sdt`（`docPartGallery="Table of Contents"`）包裹；sdt 只是外层容器，不改变以上规则。

## FLD-09 块字段更新

```
trait BlockFieldGenerator { fn regenerate(&self, field: &FieldSpan, doc: &Document) -> Result<Vec<NewBlock>>; }
```

- `UpdateBlockField` 用生成器结果替换 `separate..end` 之间的全部节点（`Deleted` + `New`），保留 begin、指令、separate、end 四组 run 与外层 sdt。
- TOC 生成器语义与 TS `generateTocFieldXml` 对齐（标题级别范围来自 `\o "1-3"`、`\h` 超链接、`\z`、`\u` 用 outlineLvl），M7 落地。
- 更新后可选设置 begin 的 `w:dirty="true"`（`EditContext.mark_updated_fields_dirty`）以让 Word 打开时重算。

## FLD-10 表单字段

`w:ffData` 子元素顺序：`w:name`, `w:label`, `w:tabIndex`, `w:enabled`, `w:calcOnExit`, `w:entryMacro`, `w:exitMacro`, `w:helpText`, `w:statusText`, 然后其一：`w:checkBox`（`w:size` | `w:sizeAuto`，`w:default`，`w:checked`）、`w:ddList`（`w:result`, `w:default`, `w:listEntry*`）、`w:textInput`（`w:type`, `w:default`, `w:maxLength`, `w:format`）。

- FORMCHECKBOX 状态：`checked ?? default`；元素存在而无 `w:val` → true；`w:val ∈ {1,true,on}` → true；都不存在 → false。显示 `☒`/`☐`。
- `ToggleCheckbox`：写 `w:checked w:val="1|0"`（存在则改，不存在则按顺序插入 `w:default` 之后）；`default` 不动。
- FORMCHECKBOX 无 `w:ffData/w:checkBox` → 策略降为 `Unknown`（TS 的同一保护规则）。

## FLD-11 页眉页脚中的字段

- `PAGE`/`NUMPAGES`/`SECTIONPAGES`/`SECTION` 是普通 `Atom`；渲染器按 `Keyword` 替换显示文本，模型不做 `PAGE_MARK` 之类的改写。
- `HfPart.has_page_number = 该 part 任一字段 keyword ∈ {PAGE}`；`has_num_pages` 同理。
- `compat_ts` 把这些原子折回 `PAGE_MARK`/`TOTAL_PAGES_MARK`（`COMPAT-02`）。

## FLD-12 新建字段

`InsertField { at, field: NewField { keyword, instr: String, result: Vec<NewInline>, form: Complex|Simple, mark_dirty } }` 生成：

```
<w:r>{rPr}<w:fldChar w:fldCharType="begin"[ w:dirty="true"]/></w:r>
<w:r>{rPr}<w:instrText xml:space="preserve"> INSTR </w:instrText></w:r>
<w:r>{rPr}<w:fldChar w:fldCharType="separate"/></w:r>
{result runs}
<w:r>{rPr}<w:fldChar w:fldCharType="end"/></w:r>
```

- `INSTR` 前后各一个空格（Word 习惯）。`rPr` 取插入点的继承格式（`EDIT-03`）。
- 生成后立即注册 `FieldSpan`，策略按 `FLD-06`。
- 新建超链接优先用 `w:hyperlink` + 关系（与 TS 一致），不生成 HYPERLINK 字段；只有编辑已存在的 HYPERLINK 字段时才保留字段形式。

## FLD-13 校验

保存前：每个 `Complex` 字段 begin/separate/end 顺序正确且同流；`nested` 的区间包含于父区间；`Block` 字段的结果段落连续。解析阶段的缺陷（未闭合、孤儿 end）为 `PreExistingDamage`；编辑后新出现的为 `EngineInvariantViolation`。

## FLD-14 坐标

原子形态字段在坐标流中恒为 1 个 `U+FFFC`，与显示结果长度无关；透明形态字段的结构 run（begin/instr/separate/end）长度 0，结果 run 按文本长度；块形态字段不参与内联坐标（`EDIT-02`、`MOD-06`）。

## 验收清单

| ID | 用例 |
| --- | --- |
| FLD-02 | begin 在 `w:hyperlink` 内、end 在其外的字段正确配对；嵌套 `IF { MERGEFIELD }` 得到 nested；未闭合 begin 记诊断且段落可编辑 |
| FLD-03 | `PAGE` 拆成两个 `w:instrText`（`PA` + `GE`）仍识别为 PAGE；`w:delInstrText` 标记修订 |
| FLD-05 | `HYPERLINK "http://x" \o "tip"` → keyword HYPERLINK，Quoted，Switch{o, Quoted}；`=SUM(ABOVE) \# "0.00"` → keyword `=`，GeneralFormat{#}；引号内 `\"` 正确 |
| FLD-06/07 | 含 REF 的段落可编辑且 REF 为原子；REF 结果两个 run（粗体 + 普通）改格式后只重生成结果 run；含 XE 的段落编辑后 XE 原字节保留 |
| FLD-07 Link | HYPERLINK 字段结果文字可编辑，保存后仍是字段而非 `w:hyperlink`；`SetLinkTarget` 只改 instrText |
| FLD-08 | 跨 12 段的 TOC：12 段全部 `FieldBlockResult`；TOC 后一段普通可编辑 |
| FLD-10 | `w:checked` 缺省取 `w:default`；无 `w:checkBox` 的 FORMCHECKBOX 为 Unknown |
| FLD-11 | 页脚 `PAGE` 为 Atom 且 `has_page_number` 为 true；`compat_ts` 输出 `PAGE_MARK` |
| FLD-12 | 插入 SEQ 字段后保存 → XPath 断言 begin/instrText/separate/end 顺序与 `xml:space` |
