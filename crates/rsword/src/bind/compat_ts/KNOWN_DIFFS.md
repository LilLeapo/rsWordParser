# 与 TS 解析器的已知差异（`COMPAT-*`，`docs/04` §5）

差分测试（`tests/decl.rs`、M1 1.15 起的 `diff-parse`）按"文档名前缀 + 字段"放行下列条目；每条写清
原因与谁对。新增条目必须同时在这里登记。

| 用例 | 字段 | 差异 | 原因 | 以谁为准 |
| --- | --- | --- | --- | --- |
| `numbering-defs__012` | `numbering[1].levels[0].numFmt` | TS `custom`，本引擎 `decimal` | `mc:Choice Requires="w14"` 但文档没有声明 `w14` 前缀；MCE 规定无法解析的前缀不算理解，本引擎取 `mc:Fallback`（`XML-09`）；TS 用正则直接取 Choice | 本引擎（规范行为；Word 同样走 Fallback） |

| `symbol-fonts__*` | 文本段落 `runs[].text` | TS 把 Symbol / Wingdings 字体的字符解码成 Unicode（`•`），本引擎保持原字符 `U+F0B7` | 符号字体映射表在 `RES-05`（M2）；`MOD-06` 规定 `Run.text` 保持原字符、解码结果放 `Segment.display`，`compat_ts` 再折回 TS 形态 | 暂时放行，M2 接入 `compat_ts` 后删除本条 |

## 定位辅助 part 的差别（不算差异，测试里已对齐）

TS 按固定路径读 `word/theme/theme1.xml`、`word/settings.xml` 等；本引擎按关系（`PKG-05`）。
语料里若干 TS 构造的文档有 part 而没有对应关系，测试在关系缺失时退回按路径查找以便对照。
