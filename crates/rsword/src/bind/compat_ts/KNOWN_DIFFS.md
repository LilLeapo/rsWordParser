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
emf-image__*             *image.dataUrl                           # 同上，表格 / run 内的图片
field-display__015*      *paras[*].runs[*].link                   # 目标带反斜杠的 HYPERLINK，TS 的正则不认，本引擎照折
wordart-vml__004*        blocks[*].textboxes[*].paras[*].runs[*]  # TS 没给框里的随文图片预取媒体，整个 run 丢了
hf-images__011*          headerImages[*].*                        # 未声明的 mc:Choice Requires="wps"，本引擎走 Fallback（同 numbering-defs__012）
hf-images__011*          hfParts.*.images[*].*                    # 同上
```
