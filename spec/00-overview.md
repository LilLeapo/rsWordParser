# SPEC 00 · 规范体系总览

## 0.1 文档层次

| 层 | 位置 | 回答的问题 |
| --- | --- | --- |
| 设计 | `docs/03-architecture-v3.md` | 为什么这样分层、什么是冻结的 |
| 规范 | `spec/*.md` | 每个模块**必须**做什么、边界情况如何处理、如何验收 |
| 参考 | `docs/01-ts-parser-reference.md` | 现有 TS 实现的行为，是差分测试与兼容性细节的来源（参考实现，不是验收权威） |
| 现状 | `docs/05-status.md` | 今天已实现什么、数字是多少、明确未实现什么 |
| 约定 | `CLAUDE.md` | 在这个仓库里干活的规则（人与 AI 同用） |

规范服从设计；规范与设计冲突时以设计为准并修订规范。规范之间的依赖只允许向下层引用（`08-edit` 可引用 `02-xml-dom`，反之不行）。

## 0.2 规范条目

- 每条规范有唯一 ID：`<前缀>-<两位序号>`，前缀见下表。ID 一经发布不复用；删除的条目保留编号并标 `[已撤销]`。
- 关键词：**必须**（MUST）、**禁止**（MUST NOT）、**应**（SHOULD）、**可**（MAY）。
- 每条规范尽量附"验收"小节：可执行的测试描述。实现中的测试函数名与注释引用规范 ID，例如 `xml_12_dirty_propagation`。

| 前缀 | 文件 | 层 |
| --- | --- | --- |
| `PKG` | `01-package.md` | L0 包层 |
| `XML` | `02-xml-dom.md` | L1 无损 XML |
| `SPAN` | `03-span.md` | L2 范围 |
| `FLD` | `04-field.md` | L2 字段子系统 |
| `PROP` | `05-properties.md` | L3 属性表 |
| `MOD` | `06-model.md` | L3 事实、分类、文档模型 |
| `RES` | `07-resolve.md` | resolve 视图 |
| `EDIT` | `08-edit.md` | L4 编辑引擎 |
| `SAVE` | `09-save.md` | 校验、序列化、包写回 |
| `COMPAT` | `10-compat-ts.md` | 兼容适配器 |
| `TEST` | `11-testing.md` | 测试基础设施 |
| — | `12-m0-m1-plan.md` | M0 / M1 任务分解 |
| — | `13-m2-plan.md` | M2 任务分解 |
| — | `15-m4-plan.md` | M4 任务分解（绘图显示模型；与 M2 / M3 并行） |

## 0.3 术语

| 术语 | 定义 |
| --- | --- |
| part | OPC 包中的一个条目（`word/document.xml`、`word/media/image1.png`…）。XML part 有 DOM，二进制 part 只有字节 |
| flavor | OOXML 命名空间族：Transitional（`schemas.openxmlformats.org/.../2006/...`）或 Strict（`purl.oclc.org/ooxml/...`）。包级 flavor 还有 `Mixed` |
| scope | 某个节点位置上实际生效的 `prefix → 命名空间 URI` 映射，由祖先链上的 `xmlns` 声明决定 |
| 内容序列 | 容器元素的语义子节点中，去掉属性元素（`pPr/tcPr/trPr/tblPr/tblGrid/sectPr`）与所有范围标记元素，且不含 `Deleted` 的有序列表。Anchor 的坐标系 |
| 内容流 | 一组连续的容器构成的独立文本流：body、每个 `w:txbxContent`、每个 `w:hdr`/`w:ftr`、每个脚注/尾注/批注条目。Span 与字段不跨内容流 |
| 语义子节点 | `semantic_children(node)`：跳过 MCE 非 active 分支与可忽略元素、展平 `ProcessContent` 容器、跳过 `Deleted` 后的子节点 |
| 标记 | 范围端点对应的物理元素：`bookmarkStart/End`、`commentRangeStart/End`、`permStart/End`、`move*RangeStart/End`、`customXml*RangeStart/End` |
| 原子 | 编辑器视为单个不可键入单位的内联对象：字段、公式、ruby、图片、脚注引用、换行、符号、OLE、绘图 |
| 规范状态 | DOM + Span。唯一可保存、唯一在编辑事务中被修改的状态 |
| 投影 | Document Model 与 resolve 视图。可从规范状态重建 |
| 干净 | `Dirty::Clean`：节点自身与后代自解析以来未被修改，保存时拷原字节 |
| 词法 | `Lex`：节点在原 part 字节中的位置与写法（区间、开闭标签、原始限定名） |

## 0.4 单位与常量

| 量 | 单位 | 备注 |
| --- | --- | --- |
| 长度（页面、缩进、间距、表格宽、单元格边距、tab 位） | twips（1/1440 in） | XML 原值；解析时接受通用度量（`12pt`、`1in`、`2.54cm`…），见 `PROP-02` |
| 字号 | half-points | `w:sz`/`w:szCs` |
| 边框粗细 | eighth-points | `w:sz` on borders |
| 绘图几何 | EMU（原值）；显示模型另给 px | 1 px = 9525 EMU；1 pt = 12700 EMU；1 twip = 635 EMU |
| 字符缩放 `w:w` | 百分数 | 接受 `NN` 与 `NN%` |
| 表格 `w:tblW type=pct` | 1/50 百分点 | 接受 `NN%` 字面 |
| 颜色 | 6 位 hex，无 `#`，或 `auto` | 解析容忍前导 `#` |
| 旋转 `rot` | 1/60000 度 | |
| 文本偏移（对外） | UTF-16 code unit | 原子占 1 单位。见 `EDIT-02` |
| 字节偏移（内部） | UTF-8 字节，相对 part 原字节 | 见 `XML-15` |
| 行高上限 | 31680 twips | `w:trHeight` |
| zip 限额 | 10,000 part；单 part 512 MiB；总计 1.5 GiB | `PKG-02` |
| XML 深度上限 | 100,000 | `XML-08` |

## 0.5 诊断

所有层用同一 `Diagnostic { part, range: Option<Range<u32>>, code: DiagCode, origin: PreExistingDamage | EngineInvariantViolation, message }`。`code` 为稀疏枚举，每个 spec 定义自己的 code 前缀（`XML_UNBOUND_PREFIX`、`FLD_UNCLOSED`…）。`EngineInvariantViolation` 在调试构建与 CI 下视为错误（`SAVE-02`）。

## 0.6 与 TS 实现的关系

- `docs/01` 描述的行为是**兼容目标**，不是设计目标：凡是 `docs/03` 明确改变的行为（Strict 保持、控制字符改原子、逻辑 run 不合并、排版启发式移出），以 `docs/03` 为准，差异由 `compat_ts` 吸收。
- 兼容性细节清单（`docs/01` 第 12 节）中的每一条都应在对应 spec 中有归属条目或在 `11-testing` 的语料中有用例。
