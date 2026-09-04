# 与 TS 解析器的已知差异（`COMPAT-*`，`docs/04` §5）

差分测试（`tests/decl.rs`、M1 1.15 起的 `diff-parse`）按"文档名前缀 + 字段"放行下列条目；每条写清
原因与谁对。新增条目必须同时在这里登记。

| 用例 | 字段 | 差异 | 原因 | 以谁为准 |
| --- | --- | --- | --- | --- |
| `numbering-defs__012` | `numbering[1].levels[0].numFmt` | TS `custom`，本引擎 `decimal` | `mc:Choice Requires="w14"` 但文档没有声明 `w14` 前缀；MCE 规定无法解析的前缀不算理解，本引擎取 `mc:Fallback`（`XML-09`）；TS 用正则直接取 Choice | 本引擎（规范行为；Word 同样走 Fallback） |

| `symbol-fonts__*` | 文本段落 `runs[].text` | TS 把 Symbol / Wingdings 字体的字符解码成 Unicode（`•`），本引擎保持原字符 `U+F0B7` | 符号字体映射表在 `RES-05`（M2）；`MOD-06` 规定 `Run.text` 保持原字符、解码结果放 `Segment.display`，`compat_ts` 再折回 TS 形态 | 暂时放行，M2 接入 `compat_ts` 后删除本条 |

| `extra__strict-minimal` | `internal.documentXml`、`internal.bodyInner*`、`extras.elements[*]` | TS 装载时把 Strict URI 改写为 Transitional（`normalizeOoxmlParts`），偏移随之变化 | 本引擎不归一化（Strict stays Strict） | 本引擎；整份文档在 `tests/compat.rs` 的 `KNOWN_DOCS` 放行 |
| `balance-dbcs-spacing__*` | `blocks[*].runs[*].charSpacingTwips` | TS 在 `balanceSingleByteDoubleByteWidth` 下按双字节字符比例缩放显示值 | 显示层决定（`MOD-11` 禁止排版字段进模型） | 本引擎；渲染器接管后删除 |
| 任意 | `blocks[*].format.charIndents*` 及由其换算的 indent* | TS `withCharIndents` 用字号换算字符单位缩进 | 需字体度量，属显示层 | 暂放行（`KNOWN_PATHS`） |
| 任意 | `styles.*.tableDisplay*` | 表格样式显示模型 | M2 随表格实现 | 暂放行 |
| 含字段 / `w14:textFill` 的段落 | 整段 | 字段折叠、`w14:textFill` 取色在 M2 | — | `tests/compat.rs` 的文本用例过滤排除 |

## 定位辅助 part 的差别（不算差异，测试里已对齐）

TS 按固定路径读 `word/theme/theme1.xml`、`word/settings.xml` 等；本引擎按关系（`PKG-05`）。
语料里若干 TS 构造的文档有 part 而没有对应关系，测试在关系缺失时退回按路径查找以便对照。

## 机器可读清单

`tools/diff-parse` 与 `tests/compat.rs` 读下面这个围栏块：每行 `<文档名 glob> <JSON 路径 glob>`，
`*` 通配任意字符，路径 `*` 表示整份文档，`#` 后是注释。新增登记须同时更新上表与此块。

```known-diffs
numbering-defs__012*     numbering.*                              # 未声明前缀的 mc:Choice Requires，本引擎走 Fallback
symbol-fonts__*          *                                        # 符号字体解码（M2 RES-05）
char-unit-indents__*     *                                        # *Chars 缩进换算需字体度量（TS withCharIndents）
extra__strict-minimal*   *                                        # TS 装载时把 Strict 改写为 Transitional
balance-dbcs-spacing__*  blocks[*].runs[*].charSpacingTwips       # TS 按双字节比例缩放显示值
*                        styles.*.tableDisplay*                   # 表格样式显示模型（M2）
*                        blocks[*].format.charIndents*            # 字符单位缩进（显示层）
```
