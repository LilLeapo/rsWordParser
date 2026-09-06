# 与 TS 解析器的已知差异（`COMPAT-*`，`docs/04` §5）

差分测试（`tests/decl.rs`、M1 1.15 起的 `diff-parse`）按"文档名前缀 + 字段"放行下列条目；每条写清
原因与谁对。新增条目必须同时在这里登记。

| 用例 | 字段 | 差异 | 原因 | 以谁为准 |
| --- | --- | --- | --- | --- |
| `numbering-defs__012` | `numbering[1].levels[0].numFmt` | TS `custom`，本引擎 `decimal` | `mc:Choice Requires="w14"` 但文档没有声明 `w14` 前缀；MCE 规定无法解析的前缀不算理解，本引擎取 `mc:Fallback`（`XML-09`）；TS 用正则直接取 Choice | 本引擎（规范行为；Word 同样走 Fallback） |
| `cell-anchored-boxes__002` / `__003` | `blocks[*].table.rows[*][*].anchoredBoxes*` | TS 给 `mc:Choice` 里的三角形，本引擎给 `mc:Fallback` 里的 VML 文本框 | 同上：`mc:Choice Requires="wps"` 但整份文档没有声明 `wps` 前缀（TS 的构造器漏了，真实 Word 输出总会声明），所以 Choice 不算能理解，走 Fallback；TS 用正则把 `mc:Fallback` 删掉直接取 Choice | 本引擎（`XML-09`） |

| `extra__strict-minimal` | `internal.documentXml`、`internal.bodyInner*`、`extras.elements[*]` | TS 装载时把 Strict URI 改写为 Transitional（`normalizeOoxmlParts`），偏移随之变化 | 本引擎不归一化（Strict stays Strict） | 本引擎；整份文档在 `tests/compat.rs` 的 `KNOWN_DOCS` 放行 |
| `balance-dbcs-spacing__*` | `blocks[*].runs[*].charSpacingTwips` | TS 在 `balanceSingleByteDoubleByteWidth` 下按双字节字符比例缩放显示值 | 显示层决定（`MOD-11` 禁止排版字段进模型） | 本引擎；渲染器接管后删除 |
| 任意 | `blocks[*].format.charIndents*` 及由其换算的 indent* | TS `withCharIndents` 用字号换算字符单位缩进 | 需字体度量，属显示层 | 暂放行（`KNOWN_PATHS`） |
| 含字段 / `w14:textFill` 的段落 | 整段 | 字段折叠、`w14:textFill` 取色在 M2 | — | `tests/compat.rs` 的文本用例过滤排除 |
| `inline-image-mixed__009` | `blocks[*].runs[*].rawRPr` | 源文件写的是 `<w:rPr></w:rPr>`，TS 输出 `<w:rPr/>` | TS 把 `w:rPr` 重新序列化，空元素折成自闭合；本引擎按 `COMPAT-04` 给原字节切片 | 本引擎（原字节才是真相） |
| `field-display__015` | 框内段落 `runs[*].link` | 目标带反斜杠的 `HYPERLINK` 字段：TS 不给链接，本引擎给 | TS 的 `convertibleHyperlink` 正则是 `"([^"\\]+)"`，目标里有反斜杠就整个不认（Windows 路径 `file:///C:\Users\…`）。引号里的反斜杠是字面量、开关只在引号外有意义，所以本引擎照常折出链接，地址原样保留 | 本引擎（功能更强；TS 那条正则是保守回避） |
| `wordart-vml__004` | `blocks[*].textboxes[*].paras[*].runs[*]` | VML 文本框里的随文图片：TS 输出空 run，本引擎给出图片 run | TS 只在**宿主**段落上预取媒体（`stripTextboxes` 之后），框里的 `a:blip` 拿不到 dataURL 就把整个 run 丢了；Word 是画得出这张图的 | 本引擎（TS 的缺陷不跟随） |
| `hf-images__011` | `headerImages[*]` / `hfParts.*.images[*]` 的 `floating` / `wrap` / `pos*` | 页眉里的 `mc:Choice Requires="wps"` 而 `wps` 前缀**没有声明**（Choice 里也没用到 `wps:` 元素）：本引擎按 `XML-09` 走 `mc:Fallback`（里面是随文副本），TS 用正则直接取 Choice（锚定副本） | 同 `numbering-defs__012` 一条：`Requires` 里的前缀必须在作用域内声明（ECMA-376 Part 3 §10.2.1），没声明就不算"理解"。真实 Word 文档都会声明 `wps`，这份是 TS 测试生成器造出来的 | 本引擎（规范行为） |
| `m6-canvas__006` | `blocks[0]` 的 `type` / `label` / `imageDataUrl` / `previewText` / `diagramDisplay` | 没有 `wp:extent` 的画布：TS 放弃画布、拿画布里第一张图当 `Image` 块；本引擎按画布投影，显示尺寸退回子坐标系 `a:chExt` 的原尺寸（缩放 1） | `wp:inline` 没有 `wp:extent` 是畸形文档（schema 里它是必填的），TS 那条路是 `extractLockedCanvas` 返回 null 后的兜底，不是有意的显示规则；画布里的形状与文字仍然是真相 | 本引擎（功能更强） |
| `m6-omml__033` | `blocks[*].table.rows[*][*].richParas[*].runs[*]` | 单元格里的 `m:oMath`：TS 一个 run 都不出，本引擎给公式 run（`text` = token，`math.omml`） | TS 的 `extractCell` 调 `extractRuns` 时不传公式片段（`mathFragments` 为空），公式在格里直接消失；Word 是画出来的 | 本引擎（功能更强） |
| `m6-ruby__005` | `blocks[*].table.rows[*][*].richParas[*].runs[*].ruby` | 单元格里的 `w:ruby`：TS 只给被注正文 `{text}`，本引擎带 `ruby: {rt, xml}` | 同上：`rubyFragments` 为空时 TS 退成 `{text: base}` | 本引擎（功能更强） |
| `extra__mixed-flavor` | `internal.documentXml`、`internal.bodyInner*`、`extras.elements[*]` | TS 装载时把混合口味（Strict + Transitional）的主 part 改写成 Transitional，偏移随之变化 | 同 `extra__strict-minimal`：本引擎不归一化 | 本引擎（M6 6.9 登记） |
| `write-protection__004` | `blocks[*].originalXml`、`internal.*`、`extras.elements[*]` | 主 part 用 `x:` 前缀绑定 `w` 命名空间（`<w:p xmlns:x="…/wordprocessingml/2006/main"><w:ins x:id=…>`）；TS 装载时把前缀改写成规范的 `w:`，本引擎按 `COMPAT-04` 给原字节 | 原字节才是真相；`x:id` 与 `w:id` 语义相同，模型侧早已按命名空间解析 | 本引擎（M6 6.9 登记） |
| `shape-extraction__014` | `blocks[0]` 的 `type` / `label` / `previewText` / `runs` | `mc:Choice Requires="wps"` 而 `wps` 前缀**没有声明**：本引擎按 `XML-09` 走 `mc:Fallback`（VML 文本框 → `Text box` 芯片），TS 用正则取 Choice（随文图片 + 文字的段落） | 同 `cell-anchored-boxes__002` / `hf-images__011` 一条 | 本引擎（规范行为；M6 6.9 登记） |
| `strict-basic*`（`corpus/real`） | `internal.*`、`extras.elements[*]`、`blocks[*].originalXml` | Word 另存的 Strict 文档带单位的度量（`w:w="595.30pt"`）与 Strict URI：TS 装载时改写成 Transitional twips，偏移随之变化 | 同 `extra__strict-minimal`：本引擎不归一化（Strict stays Strict，`SAVE-03`） | 本引擎 |
| 任意 | `styles.*.display.indentChars` | TS `withCharIndents` 在样式显示模型里也用字号换算字符单位缩进 | 与 `blocks[*].format.charIndents*` 同一条：需字体度量，属显示层 | 暂放行 |
| `fields-toc`（`corpus/real`） | `blocks[9].*` | 真实 Word 把 `REF ChapterOne \h \* MERGEFORMAT` 的指令拆成三个 `w:instrText`（首个只有一个空格）：TS `fieldLabel` 只看第一个，关键字为空 → 整段 passthrough `Field (TOC/page number/etc.)`；本引擎把连续 `instrText` 攒起来认出 REF，按可折叠字段给出可编辑 run + `instrField` | 与 `PAGE` 被拆成 `PA` + `GE` 同一条：指令是拼起来的文本，不是第一个片段 | 本引擎（功能更强） |
| `ink-pen` / `ink-highlighter` / `ink-to-shape` / `ink-math`（`corpus/real`） | `blocks[*].runs*` | Word 原生墨迹：`mc:Choice Requires="wpi"` 里是 `w14:contentPart`（InkML part），`mc:Fallback` 是 Word 自己栅格化的 PNG。`wpi` 不在本引擎理解的命名空间里（`XML-09`），走 Fallback → 墨迹成为 run 级图片；TS 剥掉 Fallback 后在 Choice 里找不到 `pic:pic`，什么都不画 | Word 渲染的是笔迹；不会画 InkML 的消费者拿到的最好结果就是 Word 留下的栅格。InkML part 原字节保留 | 本引擎（功能更强） |
| `*-resaved-by-word`、`05-ink-insert`（`corpus/real/_roundtrip`） | `blocks[*].runs[*]`、`inks*` | 本引擎写的 `aidocs-ink` 墨迹层：经 Word 另存后 run 多了 `w:rPr`（`w:noProof`）与 rsid；以真实 Word 文档为底稿直接生成时，根上没声明的 `xmlns:a` / `xmlns:pic` 被序列化器提到新 run 上（`<w:r xmlns:a=…>`）。TS `stripInkRuns` / `findInkRuns` 的正则要求 `<w:r><w:drawing>` 紧邻，两种情况都不再认它是墨迹，当成两张图片 | `docs/04` §8「墨迹的判据」：前缀才是语义；墨迹层应当在 Word 一次另存后仍然可编辑 | 本引擎（功能更强） |
| `image-emf`（`corpus/real`） | `blocks[*].imageDataUrl`、`brokenImage`、`previewText`、`type` | TS 的 metafile 转换器返回 null → `brokenImage` passthrough；本引擎给 EMF 原字节的 dataURL 图片块 | 同 `emf-image__*`：转换是可插拔服务，不在 Rust 侧做 | 本引擎（有意不同） |
| `ole-*`（`corpus/real`） | `blocks[*].imageDataUrl`、`blocks[*].table.rows[*][*].richParas[*].runs[*].image.dataUrl` | OLE 预览是 EMF / WMF：TS 转不出来就不给 `dataUrl`，本引擎给原字节 dataURL | 同上 | 本引擎（有意不同） |
| `math-in-table`（`corpus/real`） | `blocks[*].table.rows[*][*].richParas[*].runs[*]` | 单元格里的公式 TS 一个 run 都不出 | 同 `m6-omml__033` | 本引擎（功能更强） |
| `smartart-cycle` / `smartart-process` / `smartart-styled` / `smartart-edited-text` / `smartart-floating`（`corpus/real`） | `blocks[*].diagramDisplay.shapes[*].fillHex` | 真实 Word 的图示绘图 part 给连接箭头写 `<a:solidFill><a:schemeClr val="accent1"><a:tint val="60000"/>…`：本引擎按 `RES-05` 施加 tint（`8FAADC`），TS 的实心填充取色不看 `a:schemeClr` 的变换（`4472C4`） | Word 画出来的箭头就是浅的（`_previews/smartart/*.pdf`）；变换是颜色定义的一部分 | 本引擎（规范行为） |
| `ole-with-text`（`corpus/real`） | `blocks[1].*` | 文字 + `w:object` 同段：TS 只在每个对象的预览都能解析成媒体时才走 run 级图片（`smartart-ole__017`），EMF 预览转不出来就退成 `Embedded object` passthrough；本引擎的预览是原字节 dataURL，总能解析，所以按 6.4 给 run 级图片 | 根因同 `emf-image__*`：metafile 转换不在 Rust 侧做 | 本引擎（有意不同） |
| `canvas-*`（`corpus/real`） | `blocks[*].textboxes[*].offsetXEmu` / `offsetYEmu` / `bandTopPx` / `bandBottomPx`；`canvas-floating` 的 `textboxes[*].floating` | 真实 Word 的绘图画布 `wpc:wpc`：TS 不把它当容器，每个子形状都落在锚点原点（`a:off` 被忽略）；本引擎把画布当 `chOff = 0` 的组，子形状 = 锚点 + 画布内偏移（矩形在画布里内缩 0.25 in，`_previews/canvas/*.pdf` 可见）；浮动画布的子形状也跟着浮动 | 位置来自 XML，TS 那条路只是没实现画布的坐标系 | 本引擎（功能更强） |
| `canvas-picture`（`corpus/real`） | `blocks[*].textboxes*` | 画布里的 `pic:pic`：本引擎与组里的图片一样出成图片框（排在前面），TS 不认画布容器所以丢掉它，`textboxes[0]` 是矩形 | 画布里的图片 Word 是画出来的 | 本引擎（功能更强） |
| `ole-in-table`（`corpus/real`） | `blocks[*].table.rows[*][*].richParas[*].runs[*]` | 单元格里的 `w:object` TS 一个 run 都不出（与单元格里的公式 / ruby 同一条路） | 同 `m6-omml__033` | 本引擎（功能更强） |
| `emf-image__*`（4 份） | 任何 `dataUrl` | TS 把 EMF 渲染成 PNG（导出工具打的占位 `data:image/png;base64,EMFPNG`），本引擎输出 EMF 原字节的 dataURL 并标 `MediaKind::Metafile` | `docs/03` §3.5 冻结：EMF/WMF/EMZ/WMZ 与 TIFF 的转换是可插拔服务，不在 Rust 侧做，由 TS / 渲染端继续转 | 本引擎（有意不同）；语料里 4 份 metafile 媒体全在这些文档 |

## 定位辅助 part 的差别（不算差异，测试里已对齐）

TS 按固定路径读 `word/theme/theme1.xml`、`word/settings.xml` 等；本引擎按关系（`PKG-05`）。
语料里若干 TS 构造的文档有 part 而没有对应关系，测试在关系缺失时退回按路径查找以便对照。

## 机器可读清单

`tools/diff-parse` 与 `tests/compat.rs` 读下面这个围栏块：每行 `<文档名 glob> <JSON 路径 glob>`，
`*` 通配任意字符，路径 `*` 表示整份文档，`#` 后是注释。新增登记须同时更新上表与此块。

```known-diffs
numbering-defs__012*     numbering.*                              # 未声明前缀的 mc:Choice Requires，本引擎走 Fallback
cell-anchored-boxes__002* blocks[*].table.rows[*][*].anchoredBoxes*   # 同上：Requires="wps" 但没声明 wps，走 Fallback
cell-anchored-boxes__003* blocks[*].table.rows[*][*].anchoredBoxes*   # 同上
char-unit-indents__*     *                                        # *Chars 缩进换算需字体度量（TS withCharIndents）
extra__strict-minimal*   *                                        # TS 装载时把 Strict 改写为 Transitional
balance-dbcs-spacing__*  blocks[*].runs[*].charSpacingTwips       # TS 按双字节比例缩放显示值
*                        blocks[*].format.charIndents*            # 字符单位缩进（显示层）
inline-image-mixed__009* blocks[*].runs[*].rawRPr                 # 源文件写 <w:rPr></w:rPr>，TS 重序列化成 <w:rPr/>
emf-image__*             blocks[*].imageDataUrl                   # EMF 不在 Rust 侧渲染（docs/03 §3.5）
strict-basic*            internal.*                               # 真实 Word 的 Strict：TS 装载时改写成 Transitional（单位 → twips），同 extra__strict-minimal
strict-basic*            extras.elements[*].*                     # 同上（偏移）
strict-basic*            blocks[*].originalXml                    # 同上（"595.30pt" vs 11906）
*                        styles.*.display.indentChars             # 样式层的字符单位缩进（显示层，同 format.charIndents*）
fields-toc*              blocks[9].*                              # REF 指令拆成三个 instrText，TS 认不出关键字整段 passthrough；本引擎折成 run
ink-pen*                 blocks[*].runs*                          # Word 原生墨迹（Requires="wpi"）：本引擎走 Fallback 出栅格 run 图片，TS 什么都不画
ink-highlighter*         blocks[*].runs*                          # 同上
ink-to-shape*            blocks[*].runs*                          # 同上
ink-math*                blocks[*].runs*                          # 同上
*-resaved-by-word*       blocks[*].runs[*]                        # Word 另存后 aidocs-ink run 带了 rPr，TS 正则不再认它是墨迹
*-resaved-by-word*       inks*                                    # 同上
05-ink-insert*           blocks[*].runs[*]                        # 以真 Word 文档为底稿生成的墨迹 run 根上带 xmlns 声明，TS 正则同样不认
05-ink-insert*           inks*                                    # 同上
image-emf*               blocks[*].imageDataUrl                   # EMF 不在 Rust 侧渲染（同 emf-image__*）
image-emf*               blocks[*].brokenImage                    # 同上
image-emf*               blocks[*].previewText                    # 同上
image-emf*               blocks[*].type                           # 同上
ole-*                    blocks[*].imageDataUrl                   # OLE 预览 EMF/WMF：TS 转不出来不给，本引擎给原字节
ole-*                    blocks[*].table.rows[*][*].richParas[*].runs[*].image.dataUrl   # 同上（表格里）
math-in-table*           blocks[*].table.rows[*][*].richParas[*].runs[*]      # 单元格里的公式 run：TS 丢（同 m6-omml__033）
canvas-*                 blocks[*].textboxes[*].offsetXEmu        # 真实 Word 画布 wpc:wpc：TS 不做画布坐标系，子形状全落在锚点原点
canvas-*                 blocks[*].textboxes[*].offsetYEmu        # 同上
canvas-*                 blocks[*].textboxes[*].bandTopPx         # 同上（带随偏移变）
canvas-*                 blocks[*].textboxes[*].bandBottomPx      # 同上
canvas-floating*         blocks[*].textboxes[*].floating          # 同上：浮动画布的子形状跟着浮动
canvas-picture*          blocks[*].textboxes*                     # 画布里的图片：TS 丢，本引擎出图片框
ole-in-table*            blocks[*].table.rows[*][*].richParas[*].runs[*]      # 单元格里的 OLE：TS 丢（同单元格公式）
ole-with-text*           blocks[*].imageHeightPx                  # 文字 + OLE 同段：TS 因 EMF 预览转不出来退成 passthrough，本引擎给 run 级图片（6.4）
ole-with-text*           blocks[*].imageWidthPx                   # 同上
ole-with-text*           blocks[*].label                          # 同上
ole-with-text*           blocks[*].oleProgId                      # 同上
ole-with-text*           blocks[*].previewText                    # 同上
ole-with-text*           blocks[*].runs                           # 同上
ole-with-text*           blocks[*].type                           # 同上
smartart-cycle*          blocks[*].diagramDisplay.shapes[*].fillHex   # 图示箭头 schemeClr + tint：TS 不施加变换
smartart-process*        blocks[*].diagramDisplay.shapes[*].fillHex   # 同上
smartart-styled*         blocks[*].diagramDisplay.shapes[*].fillHex   # 同上
smartart-edited-text*    blocks[*].diagramDisplay.shapes[*].fillHex   # 同上
smartart-floating*       blocks[*].diagramDisplay.shapes[*].fillHex   # 同上
extra__mixed-flavor*     internal.*                               # TS 装载时把混合口味的主 part 改写成 Transitional（同 extra__strict-minimal，6.9）
extra__mixed-flavor*     extras.elements[*].*                     # 同上（偏移）
write-protection__004*   blocks[*].originalXml                    # 主 part 用 x: 前缀绑定 w 命名空间，TS 改写成 w:，本引擎原字节（6.9）
write-protection__004*   internal.*                               # 同上
write-protection__004*   extras.elements[*].*                     # 同上（偏移）
shape-extraction__014*   blocks[0].type                           # 未声明的 mc:Choice Requires="wps"，本引擎走 Fallback（同 cell-anchored-boxes__002，6.9）
shape-extraction__014*   blocks[0].label                          # 同上
shape-extraction__014*   blocks[0].previewText                    # 同上
shape-extraction__014*   blocks[0].runs                           # 同上
m6-canvas__006*          blocks[0].type                           # 没有 wp:extent 的画布：TS 退成第一张图，本引擎按画布画（6.3）
m6-canvas__006*          blocks[0].label                          # 同上
m6-canvas__006*          blocks[0].imageDataUrl                   # 同上
m6-canvas__006*          blocks[0].previewText                    # 同上
m6-canvas__006*          blocks[0].diagramDisplay*                # 同上
m6-omml__033*            blocks[*].table.rows[*][*].richParas[*].runs[*]      # 单元格里的公式 run：TS 丢，本引擎给（6.5）
m6-ruby__005*            blocks[*].table.rows[*][*].richParas[*].runs[*].ruby # 单元格里的 ruby：TS 只留正文，本引擎带 ruby（6.5）
emf-image__*             *image.dataUrl                           # 同上，表格 / run 内的图片
field-display__015*      *paras[*].runs[*].link                   # 目标带反斜杠的 HYPERLINK，TS 的正则不认，本引擎照折
wordart-vml__004*        blocks[*].textboxes[*].paras[*].runs[*]  # TS 没给框里的随文图片预取媒体，整个 run 丢了
hf-images__011*          headerImages[*].*                        # 未声明的 mc:Choice Requires="wps"，本引擎走 Fallback（同 numbering-defs__012）
hf-images__011*          hfParts.*.images[*].*                    # 同上
```
