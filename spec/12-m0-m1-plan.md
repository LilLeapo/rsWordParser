# SPEC 12 · M0 / M1 任务分解

对应 `docs/03` 第 12 节前两行。每个任务给出产出、依赖的规范条目与完成定义（DoD）。顺序即建议的实现顺序；同一编号内的子任务可并行。

## M0 · 无损读写骨架

目标：任意语料 `parse → serialize` 字节相同（含 Strict），tokenizer 与 zip 层通过模糊测试。

| # | 任务 | 规范 | DoD |
| --- | --- | --- | --- |
| 0.1 | 语料导出脚本（genoffice 侧）并提交首批 `corpus/synthetic`（至少 kitchen-sink、simple 与 20 个测试用例）与 `corpus/hostile` | TEST-01/02/09 | 目录就位，`expected.json` 可被读取 |
| 0.2 | crate 骨架与错误类型；`Diagnostic` 与 `ValidationOrigin` | 00 §0.5 | 编译通过 |
| 0.3 | zip 读取：0x7075 中和、限额、条目枚举、原字节保留 | PKG-01/02/11 | hostile 三用例；unicode-path 用例 |
| 0.4 | `[Content_Types].xml`、`.rels` 解析、`RelTarget`、路径归一化、`RelType` 双族 | PKG-04/05/06/07 | 验收清单 PKG-04–07 |
| 0.5 | 主 part 定位、flavor 判定、`Part` 结构、`NamespaceContext` | PKG-03/08/09 | Strict/Transitional/Mixed 三用例 |
| 0.6 | schema 名字表与 `build.rs`：`NsId`（含 `Xml`/`Xmlns`/`Unbound`）、`LocalName` | XML-05 | 表覆盖 01-ts-parser-reference 中出现的全部元素与属性名 |
| 0.7 | tokenizer：序言/尾声、`Lex`（含 `name`）、属性（`lex_name`、`quote`、重复容忍）、实体按需解码、迭代深度、畸形报错 | XML-01–08 | 3000 层与 5000 层用例；不平衡报错 |
| 0.8 | `namespace_scope`、`namespace_compatible`、`required_decls` | XML-11 | 单元测试 |
| 0.9 | MCE：`Ignorable`、`AlternateContent`、`ProcessContent`、`MustUnderstand`；`semantic_children` | XML-09/10 | 验收清单 XML-09 |
| 0.10 | `Dirty` 状态与传播；`move_within_part`；`Clean` 克隆 | XML-12 | 验收清单 XML-12 |
| 0.11 | 序列化：`Clean/DescendantDirty/SelfDirty/New/Deleted`，`write_open_tag` 复用 `lex_name` | XML-13/14 | 全部语料字节相同；SelfDirty 保留属性顺序与引号 |
| 0.12 | 包写回：`raw_copy_file`、顺序、无脏短路 | SAVE-01/06 | 往返字节相同 |
| 0.13 | `fuzz_zip`、`fuzz_xml` | TEST-06 | 各 10 分钟无崩溃 |

**不在 M0**：任何 WordprocessingML 语义、模型、compat。

## M1 · 属性表、文本段落、首个编辑往返

目标：文本段落（paragraph/heading/listItem）的 `compat_ts` JSON 与 TS 一致；改一段文字后保存满足不变式 2；Strict 文档改字后仍为 Strict。

| # | 任务 | 规范 | DoD |
| --- | --- | --- | --- |
| 1.1 | 属性表格式与 `build.rs` 生成器；codec 实现（含通用度量） | PROP-01/02/04/09 | 验收清单 PROP-02/09 |
| 1.2 | `RunProps`、`ParaProps`（含段落标记 rPr）、schema 顺序表 | PROP-05/08 | 每行往返测试（PROP-07） |
| 1.3 | `plan_apply_*` 合并算法 | PROP-06 | 验收清单 PROP-05/06 |
| 1.4 | styles.xml、numbering.xml、theme、settings（含 `CompatFacts`）、fontTable 的声明模型 | MOD-10 | 解析语料无 panic；字段与 TS 对照 |
| 1.5 | `Run`/`Segment`/坐标流；`xml:space`；`Inline::Atom(Math)` 占位 | MOD-06 | 验收清单 MOD-06 |
| 1.6 | `ParagraphFacts`（M1 只需：文本、sectPr、样式、编号、outline；绘图/字段/VML 事实先置空） | MOD-04 | — |
| 1.7 | 分类规则表 R01/R02(占位 Table)/R07/R08/R10/R19 与 `TextKind` 判定 | MOD-05/03 | 文本段落分类与 TS 一致 |
| 1.8 | `Document::rebuild` 骨架、`FlowId` 映射 | MOD-01/13, SPAN-01 | rebuild 幂等 |
| 1.9 | `resolve` 首版：默认样式、basedOn 链、linked、主题字体/颜色、Cs 选择（toggle 先用占位规则并标注） | RES-02/03/05/06 | 与 TS `StyleDisplay` 对照 |
| 1.10 | `compat_ts`：顶层字段、文本 Block、Run 映射、`docxIndex/originalXml/rawPPr/rawRPr`、UTF-16 索引 | COMPAT-02/04/06/07 | `diff-parse` 对文本用例为 0 |
| 1.11 | `EditSession`、`EditContext`、`InlinePos` 定位、`MutationPlan/validate/commit` 框架 | EDIT-01/02/05 | 失败回滚测试 |
| 1.12 | 操作：`InsertText`、`DeleteRange`（同段）、`SetRunProps`、`SetParaProps`、`ReplaceInlines`（不含修订生成） | EDIT-03 | 验收清单 EDIT-03 对应行 |
| 1.13 | `SaveBlock[]` 兼容映射（original/generated 文本块） | EDIT-04, COMPAT-08 | `text-patch` 类用例 XPath 等价 |
| 1.14 | 保存流程：校验框架（先只做命名空间与 `PROP-05` 顺序）、`w:t` preserve、flavor 编解码 | SAVE-01/02/03 | Strict 编辑测试 |
| 1.15 | `diff-parse`、`xpath-assert` 工具 | TEST-03/05 | CI 接入 |

**推迟到 M2**：Span 索引与 Anchor 变换（M1 的 `DeleteRange` 遇到标记元素时暂按"标记不动"处理并记 `EngineInvariantViolation` 以便发现）、字段。

## 风险提示（实现前确认）

1. `quick-xml` 是否满足 `XML-03` 的区间精度（属性值区间、开标签结束位置）；不满足则自写 tokenizer（预计 1.5k 行）。
2. `zip` crate 对 0x7075 与 `raw_copy_file` 的行为在目标版本上验证。
3. 属性表生成器先用最小格式（Rust 宏或 TOML），不要一开始追求通用。
4. `compat_ts` 的 `docxIndex` 对齐（sdt 拆分）在 M1 就要做对，否则差分工具全盘失效。
