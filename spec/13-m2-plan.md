# SPEC 13 · M2 任务分解

对应 `docs/03` 第 12 节 M2 行。格式同 `spec/12`：每个任务给出产出、依赖的规范条目与完成定义（DoD）。
顺序即建议的实现顺序；同一编号内的子任务可并行。排期依据（实测差距排名）见 `docs/04` §10 与
`docs/05-status.md`。

## M2 · L2 层：范围（Span）与字段

目标：范围标记与字段进入规范状态，编辑操作能正确变换它们；含字段的段落可编辑。

CI 门（`spec/11` TEST-10）：字段与 Span 域的 `synthetic` diff 为 0；`fuzz_instr` 10 分钟无崩溃。
**两条都已达成**（2026-09-05）：`diff-parse --scope fields` 253 份文档 0 未知差异（`.github/workflows/ci.yml`
里是一步），`fuzz_instr` 13,572,886 次执行 / 601 秒无崩溃（`fuzz.yml` 的 matrix 里）。

| # | 任务 | 规范 | DoD |
| --- | --- | --- | --- |
| 2.1 ✅ | Span 索引：`Anchor` / `Affinity` / `RangeSpan` / `RangeKind` / `SpanId`，内容序列与按 `FlowId` 构建；起终点配对、同流、`compare` 文档序；孤儿 / 未闭合 / 重复 / 跨流各记诊断 | SPAN-01–SPAN-05 | 全语料标记成对；`corpus/hostile/span-orphan-end.docx` 记 `SPAN_ORPHAN_END` 且保存成功 |
| 2.2 ✅ | 四条变换规则（插入 / 删除 / 拆分 / 合并）+ 整体删除策略，索引接进 `EditSession` 与事务 | SPAN-06, SPAN-07 | `EDIT-03` 验收行：删除覆盖书签起点后起点落到删除处；`DeleteRange` 不再记 `EDIT_ANCHOR_UNMOVED` |
| 2.3 ✅ | Span 物化与保存校验：接进 `EditSession::save_with` 的第 3 步；孤儿端点成对删除 / 补齐 | SPAN-08, SPAN-09, SAVE-02 | 编辑后标记位置与 Anchor 一致；未动过的标记字节不变；注入破坏的 Span 在调试构建下 `Err(SAVE_INVARIANT)` |
| 2.4 ✅ | 字段子系统：`FieldSpan` 解析（begin / instr / separate / end 与嵌套）、指令解析器、`FieldForm` 的 Link / Atom / Block 三种策略 | FLD-01–FLD-08, FLD-13 | 验收清单 FLD 对应行；未闭合字段保持原字节；`corpus/hostile/field-unclosed.docx` 记诊断 |
| 2.5 ✅ | 字段进模型与 compat：`Inline::Field{id,result}`、`Run.field`、透明字段的 `Link`；`fieldDisplay` | MOD-06, COMPAT-07 | `bookmarks-crossref` / XE / 复选框 / PAGE 等用例 diff 为 0（约 36 处字段显示差异归零） |
| 2.6 | 批注与注释部件：解析 `comments.xml` / `commentsExtended.xml`，`Run.comments` 与块级 `commentStarts/Ends`；`AddComment` / `RemoveComment` / `SetCommentText`；**新建 part**（关系 + 内容类型） | SAVE-05, EDIT-03 | 保存语料 7 份 comments 与 3 份 footnotes 用例通过；首次加批注后其他条目原压缩数据不变 |
| 2.7 ✅ | 符号字体解码（`w:sym` 与符号字体 run 的文本映射） | RES-05 | 删掉 `KNOWN_DIFFS.md` 里整份放行的 `symbol-fonts__*` |
| 2.8 ◐ | `EDIT-06` id 分配落地（`rId` 完成，批注 / 书签 / `paraId` 随 2.6 / 2.9）：`rId`（新外链）、书签 `w:id`、批注 `w:id`、修订 `w:id` 预留、`w14:paraId` | EDIT-06 | 验收清单 EDIT-06；`insert-and-layout__001.save.10`（新超链接关系）通过 |
| 2.9 | 字段与段落操作：`InsertField`、`SetFieldResultProps`、`ToggleCheckbox`、`SetFormText`、`SetLinkTarget`、`UpdateBlockField`、`SplitParagraph`、`MergeWithNext`、`AddBookmark` / `RemoveBookmark` | FLD-09–FLD-12, EDIT-03 | 各操作的验收行；跨段透明字段拆分返回 `Err(EDIT_SPLIT_FIELD)` |
| 2.10 ✅ | `fuzz_instr` 目标与 M2 门接入 CI | TEST-06, TEST-10 | 指令解析 10 分钟无崩溃（实跑 1,357 万次执行）；`diff-parse --scope fields` 253 份 0 未知差异 |

`compare`（`SPAN-05`）原计划在 2.2，实际随 2.1 落地——"起在终前"是索引自己的验收项，绕不开它。

**顺序说明**：2.1 → 2.2 → 2.3 是一条链（Anchor 依赖 Span 索引，物化依赖 Anchor）；2.4 → 2.5 → 2.9 是
另一条。2.6 / 2.7 / 2.8 可与两条主链并行。2.2 完成后应立刻回头删掉 M1 在 `edit/ops.rs` 里的
`EDIT_ANCHOR_UNMOVED` 分支，别让临时行为留成语义。

## 从 M1 带过来的债（M2 内解决）

| 债 | 位置 | 解决任务 |
| --- | --- | --- |
| `DeleteRange` 覆盖范围标记时"标记不动"并记 `EngineInvariantViolation` | `edit/ops.rs` | 2.2 |
| `DeleteRange` 覆盖字段结构段（`fldChar` / `instrText` / `commentReference`）时整 run 保留，只删文本段 | `edit/ops.rs` | 2.4 |
| `save_with` 第 3 步（Span 物化）是空操作 | `edit/session.rs` | 2.3 |
| 缺 `word/settings.xml` 时清洗标志写不进去，只记诊断 | `save/options.rs` | 2.6（新建 part 能力） |
| 新外部超链接无法分配 `rId`，`apply_save_blocks` 直接 `EditUnsupported` | `bind/compat_ts/save_blocks.rs` | 2.8 |

## 不在 M2

表格模型与表格操作（M3）、绘图显示模型与图片原子（M4）、页眉页脚 / 脚注尾注正文管线与节 / 保护 /
toggle 校准（M5）、图表 SmartArt OLE 媒体（M6）、修订生成与 `EditOp` 全集（M7）。

M1 已实现的编辑操作在 M2 内**不重写**，只补 Anchor 与字段两块；`EditContext.track_changes` 继续忽略。

## 风险提示（实现前确认）

1. **Anchor 与标记的对偶**：编辑期 Anchor 是事实、标记是投影（`docs/03` §5.2）。实现前先确定"什么时候把
   Anchor 写回成标记元素"——建议只在 `save` 的物化步骤写回，编辑期只动 Anchor，否则每个操作都要维护标记顺序。
2. **交叠范围**：`RangeSpan` 允许交叠（书签与批注可以交叉），索引不能假设树形。删除区间时要按 `SPAN-07`
   决定是删标记还是搬标记。
3. **字段嵌套与跨段**：`Block` 策略的字段（TOC 等）结果段落只读，与今天的 TS 一致；放开需要逐字段评估
   生成器能力（`docs/03` §14）。M2 只保证"不破坏"。
4. **指令解析的容错**：`instr_nodes` 的原字节是保存真相（`docs/03` §13）。解析器只产出语义视图，
   **禁止**用解析结果重新序列化指令，否则开关（`\r` `\h` `\* MERGEFORMAT`）会丢。
5. **语料基线**：已决（2026-09-04）接受当前基线不重导，理由见 `docs/04` §9 第 1 条。
6. **语料在 Span / 字段域很薄**：573 份里只有 15 份带范围标记（31 个标记）。差分数字归零不等于实现正确，
   2.1 起的每条规则都要有自己的单元用例（`tests/span.rs`）。
