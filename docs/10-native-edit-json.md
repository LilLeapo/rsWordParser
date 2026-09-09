# 10 · 原生编辑 JSON 验收与拒绝集（BIND-03 v3）

`edit_op_json!` 的同一张表展开 66 个线型变体、上下文化双向转换和每变体测试。
`tests/native_edit.rs` 的独立规范清单与宏清单双向对表。

反向 `edit_op_to_json` 是往返测试、调试及 M9′ 审计设施，不是协议数据路径。
引擎 `EditOp` / `NewElement` 不实现 serde。原生请求的结构化属性在边界由 patch 应用到
缺省属性，再交给生成的 `emit_para_props` / `emit_run_props`；引擎仍持 `Option<NewElement>`。

## 成文拒绝集

逐变体测试保留表外属性样例，实际分类为 **57 个无损往返 + 9 个具名拒绝 = 66**。
以下拒绝的是每个变体中的不可表达载荷，并非拒绝整个操作。九个变体另有结构化正向往返，
断言 JSON → 引擎 → JSON → 引擎的引擎值相等。
反向只有在属性读取、生成后与原 `NewElement` 完全相等时才成功，否则返回
`EditJsonError::Unrepresentable`（显示码 `BIND_EDIT_UNREPRESENTABLE`），错误包含字段路径。
未知属性 / 子元素、非生成顺序、不能原样重建的属性写法都不静默归一化。

| 变体 | 拒绝样例中的间接属性路径 |
| --- | --- |
| `InsertAtom` | `NoteRef.content[][] → NewRun.props` |
| `InsertBlock` | `Paragraph.props / inlines → NewRun.props` |
| `InsertField` | `NewField.result → NewRun.props` |
| `ReplaceInlines` | `inlines → NewRun.props` |
| `SetHeaderFooter` | `content → Paragraph.props / inlines` |
| `SetNoteContent` | `content[][] → NewRun.props` |
| `SetSdtContent` | `inlines → NewRun.props` |
| `SetTextboxContent` | `blocks → Paragraph.props / inlines` |
| `UpdateBlockField` | `blocks → Paragraph.props / inlines` |

测试从此表读出变体集合，与独立 `REFUSED` 常量严格比较。递归的 Hyperlink / Ins / Del / Field、
Wrapped / Textbox / Many 都通过穷尽解构与显式工作栈转换，新增字段不能悄悄消失。

## 线型补充

带载荷的 NewBlock / NewInline / NewAtom 使用 `{"kind":"…","value":…}`；结构体字段为 camelCase。
属性 `Change` 的 Keep 缺席、Unset 为 null、Set 为值本身；TableChange 另有
`{"$patch":…}`，空 Patch 不得省略。schema 的 Set / Patch 用 oneOf 区分，声明并要求 `$patch`。
BIND-02 键集检查器也覆盖这条线型，注入未声明的属性必须失败。

XML 逃生口每次使用收集一条 BIND_XML_ESCAPE 转换元数据，apply 成功才并入会话诊断。
`edit_diagnostics_json` 返回诊断与 `xmlEscapeCount`，`xml_escape_count` 从会话诊断计数；apply 失败不提交 DOM、驻留名或诊断。转换使用的克隆及
会话导出的性能预算在 8.4 实测。XML 元素入口必须恰有一个元素，不能承载的顶层文本、
注释 / PI / CDATA 明确拒绝；part 整体替换不受 NewElement 形态限制。

原生 SaveOptions 只有五项；隐私清洗缺省 false。compat 的宿主策略显式读标志并传 true，
测试专用 CompatSaveOptions / save_with_compat 保留旧选项调用点，后续随 compat-ts feature 隔离。
六个声明操作复用原计划生成器，按 apply 的事务边界提交；编号按 numId 去重，样式按 styleId
upsert，相同请求无新计划，主题相同值也不重复写入。参考文献的未变条目按原子树字节断言。

本任务的全语料保存字节比较覆盖 synthetic + real 的 1065 份。包括 hostile 的全语料
无编辑 EditSession::save() 专门门，以及 TEST-07 全部改走协议、fuzz_bind，仍按排期在 8.6 落地。
