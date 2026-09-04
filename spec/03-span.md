# SPEC 03 · L2 范围层（span）

对应 `docs/03` 第 5.1–5.3、5.5、5.6 节。职责：把跨节点的范围结构（书签、批注、权限、移动范围、customXml 修订范围）表示为附着在 DOM 上的位置语义，并在编辑时维护它们。字段子系统见 `04-field.md`。

## SPAN-01 内容序列与内容流

- **内容序列** `content(container)`：`semantic_children(container)` 去掉属性元素（`w:pPr w:tcPr w:trPr w:tblPr w:tblGrid w:sectPr w:tblPrEx`）与所有范围标记元素后的有序列表。容器包括 `w:body w:p w:tc w:tr w:tbl w:txbxContent w:sdtContent w:hdr w:ftr w:footnote w:endnote w:comment w:ins w:del w:hyperlink w:smartTag w:customXml w:fldSimple`。
- 边界 `k` 表示 `content[k-1]` 与 `content[k]` 之间，`0 ≤ k ≤ len`。
- 内容序列**只含元素节点**：容器里的文本与 Opaque（注释 / PI）节点不是内容项。缩进排版产生的空白
  文本节点若占据边界，同一份文档换个产出工具就会改变锚点坐标；而这些容器的合法内容本来只有元素。
- **内容流**：body（`w:body` 及其后代容器）、每个 `w:txbxContent`、每个 `w:hdr`/`w:ftr`、每个脚注/尾注/批注条目各为独立流。范围**禁止**跨流。
- **FlowId**：每个流分配 `FlowId(u32)`；`flow_of(container) -> FlowId` 由"容器 → 流根"的缓存映射给出（流根：`w:body`、`w:txbxContent`、`w:hdr`、`w:ftr`、`w:footnote`、`w:endnote`、`w:comment`）。`flow_of(&Anchor) = flow_of(anchor.container)`。同流判定**必须**比较 `FlowId`，不得靠祖先树临时推断。子树移动跨越流根时，缓存对该子树失效并重建。

## SPAN-02 Anchor

```
Anchor { container: NodeId, index: u32, affinity: Left | Right, marker: Option<NodeId> }
```

- `index` 是内容序列边界，**标记自身不计入**。
- `affinity`：`Left` 吸附左侧内容（在该边界插入的内容落在锚点之后）；`Right` 吸附右侧内容（插入落在锚点之前）。默认：起点 `Right`，终点 `Left`。效果：边界处输入落在范围外，范围内部输入扩展范围。
- **空范围例外**：两端落在同一 `(container, index)` 时终点也取 `Right`。否则 `SPAN-05` 的 `Left < Right` 会判成"起在终后"，而且边界插入会把空范围拆反（起点右移、终点不动）。空范围整体吸附右侧内容，与"位置书签跟着后面的内容走"一致。
- `marker` 指向物理标记元素；字段边界的 Anchor 为 `None`（`FLD`）。
- 解析时由标记位置建立 Anchor；此后 Anchor 是事实，标记是投影（`SPAN-08`）。编辑引擎**禁止**通过移动标记节点来移动范围。

## SPAN-03 范围种类与 XML 来源

| 种类 | 起点元素（属性） | 终点元素 | 配对键 | 附加 |
| --- | --- | --- | --- | --- |
| Bookmark | `w:bookmarkStart` (`w:id`, `w:name`, `w:colFirst`, `w:colLast`, `w:displacedByCustomXml`) | `w:bookmarkEnd` (`w:id`, `w:displacedByCustomXml`) | `w:id` | `hidden = name 以 _ 开头`；`_GoBack` 是 Word 的"上次编辑位置"，按普通隐藏书签处理 |
| Comment | `w:commentRangeStart` (`w:id`) | `w:commentRangeEnd` (`w:id`) | `w:id` | `reference` = 含 `w:commentReference[@w:id]` 的 run；无范围只有 reference（LibreOffice 风格）→ 折叠范围，起终点同位于 reference run 之前的边界 |
| Permission | `w:permStart` (`w:id`, `w:edGrp`, `w:ed`, `w:colFirst`, `w:colLast`) | `w:permEnd` (`w:id`) | `w:id` | |
| MoveFrom | `w:moveFromRangeStart` (`w:id`, `w:name`, `w:author`, `w:date`) | `w:moveFromRangeEnd` (`w:id`) | `w:id` | 与 `w:moveFrom` 内容修订配对（`MOD-08`） |
| MoveTo | `w:moveToRangeStart` | `w:moveToRangeEnd` | `w:id` | |
| CustomXmlIns/Del/MoveFrom/MoveTo | `w:customXml{Ins,Del,MoveFrom,MoveTo}RangeStart` (`w:id`, `w:author`, `w:date`) | 对应 `…RangeEnd` | `w:id` | |

`colFirst/colLast`：书签或权限范围在表格中覆盖的列区间；模型保留原值，编辑表格列时按 `EDIT-03` 更新。

## SPAN-04 构建

对每个 part、每个内容流，按文档序遍历所有容器的语义子节点：

1. 遇到标记元素：计算其所在容器与内容序列边界 `k`（其前面的内容子节点数），生成 `Anchor{container, k, 默认 affinity, marker}`。
2. 起点入 `open[kind][id]`；终点查 `open` 配对，得 `RangeSpan{start, end}`；找不到起点 → `RangeSpan{start: None, end}`，记诊断 `SPAN_ORPHAN_END`。
3. 未闭合的起点 → `RangeSpan{start, end: None}`，记诊断 `SPAN_UNCLOSED`。实现在**整个 part 扫完**后统一报告（不是每个流结束时）：这样后面的流里出现同 id 终点还能被识别为跨流配对（`SPAN_CROSS_FLOW`），而不是退化成一对"未闭合 + 孤儿终点"。范围集合与按流报告时相同。
4. 同一 `id` 重复起点：后者视为新范围，记诊断 `SPAN_DUP_START`。终点就近配对（后开先闭），两个范围因此正确嵌套。
   终点在别的流里找到同 id 起点：记 `SPAN_CROSS_FLOW`，**不配对**（范围禁止跨流），两端各按损坏处理。
5. Comment 的 `reference` 在遍历中按 `w:id` 关联；只有 reference 的批注生成折叠范围。

索引：`spans: Vec<RangeSpan>` 平铺；辅助索引 `by_container: Map<NodeId, Vec<(SpanId, End)>>`。
每个范围另记 `origin: Parsed | New`：`Parsed` 且某端 `marker == None` 表示**文件里本来就没有这个标记**
（只有 `commentReference` 的批注就是这样），`SPAN-08` 物化**不得**为它补写标记，否则未编辑内容会被改写。

## SPAN-05 文档序

`Dom::compare(a: &Anchor, b: &Anchor) -> Ordering`：

1. 同容器：比较 `index`；相等则 `Left < Right`；仍相等 → `Equal`。
2. 不同容器：求两容器的祖先路径，找最近公共祖先 `C`；比较两侧在 `C.children` 中的子序号（若一侧就是 `C`，则用其 `index` 与另一侧所在子节点在内容序列中的位置比较）。
3. `flow_of(a) != flow_of(b)` → `Err(SPAN_CROSS_FLOW)`（在步骤 1 之前检查）。

有效范围**必须** `compare(start, end) != Greater`。

## SPAN-06 编辑时的 Anchor 变换

编辑引擎对每种 DOM 变更**必须**应用以下变换（`k`、`a`、`b` 为内容序列边界）：

| 变更 | 对同容器 Anchor 的影响 |
| --- | --- |
| 在边界 `k` 插入 `n` 个内容项 | `index > k` → `+n`；`index == k`：`Right` → `+n`，`Left` → 不变 |
| 删除 `[a, b)` | `index ≤ a` → 不变；`index ≥ b` → `-(b-a)`；`a < index < b` → `a` |
| 拆分容器于 `k`（`P1=[0,k)`, `P2=[k,len)`） | `index < k` → 留 P1；`index > k` → P2，`index-k`；`index == k`：`Left` → P1 末尾 `k`，`Right` → P2 起点 `0` |
| 合并 `P1 + P2` | P2 内 Anchor → P1，`index + len(P1)` |
| 删除容器 | 容器内 Anchor 按 `SPAN-07`；若容器是某内容序列的一项，外层容器按"删除 `[a,a+1)`"处理 |
| 移动子树 | 子树内部 Anchor 不变（`container` 未变）；源父与目标父分别按删除与插入处理 |

变换在 `MutationPlan` 中计算并与 DOM 变更同一事务提交（`EDIT-05`）。

## SPAN-07 整体删除策略

范围两端都落入被删除内容（或被删除容器）时：

| 种类 | 策略 |
| --- | --- |
| Bookmark | 折叠：起终点都置于删除点，范围保留（`_Toc/_Ref` 目标仍存在，REF/TOC 不断链） |
| Comment | 删除批注：范围与 reference run 删除，`comments.xml`/`commentsExtended.xml`/`commentsIds.xml` 对应条目删除（Word 行为）；`EditContext.keep_orphan_comments` 可改为折叠 |
| Permission | 删除 |
| MoveFrom/MoveTo | 由修订操作决定（`EDIT-03` Accept/Reject Move） |
| CustomXml 范围 | 随所属修订处理 |

仅一端落入删除区间 → 该端按 `SPAN-06` 移到删除点（范围缩短）。

## SPAN-08 物化

保存前，对每个 `RangeSpan` 的每个 Anchor：

- 有 `marker` 且标记在 DOM 中仍位于 `(container, index)` 对应位置 → 不动（`Clean` 拷字节）。
- 位置不符 → 旧标记 `Deleted`，在新位置插入 `New` 标记，属性从旧标记复制（`Raw` 值可直接引用旧字节）。
- 无 `marker` 且 `origin == New`（本次会话新建的范围）→ 插入 `New` 标记；`w:id` 按 `EDIT-06` 分配。
- 无 `marker` 且 `origin == Parsed`（文件里本来就没有标记的折叠批注）→ **不插入**，保持原样。
- 起点标记插在边界 `index` 处所有 `Left` 锚点标记之后、`Right` 锚点标记之前？——**规定**：同一边界上先输出所有终点标记，再输出所有起点标记（Word 输出习惯，避免相邻范围的标记交叉）。**例外**：同一个范围自己的两端落在同一边界（空范围）时按"起点、终点"顺序输出，否则物理上就成了反序的一对。

## SPAN-09 校验

保存前校验（`SAVE-02`）对范围检查：起终点都存在；`flow_of(start) == flow_of(end)`；`compare(start, end) != Greater`；Comment 有 reference 与条目；`id` 在 part 内唯一。失败按 `origin` 处理：解析阶段就存在的缺陷为 `PreExistingDamage`（成对删除或补齐并记诊断）；编辑后新出现的为 `EngineInvariantViolation`。

## SPAN-10 与字段的关系

- 范围端点落在字段**结果**区内允许；落在字段**指令**区内（begin 与 separate 之间）视为落在字段原子内部，编辑时移到原子边界（起点 → 原子前，终点 → 原子后）。
- 范围端点与字段边界重合时的顺序：标记在 `fldChar begin` run 之前、在 `fldChar end` run 之后。

## 验收清单

| ID | 用例 |
| --- | --- |
| SPAN-01 | 含 `w:pPr`、书签标记、run 的段落：内容序列只含 run |
| SPAN-04 | 跨三个段落的书签解析出正确 Anchor；只有 reference 的批注生成折叠范围；未闭合书签记诊断 |
| SPAN-05 | 跨段落、跨单元格范围的 compare 正确；同边界 Left < Right |
| SPAN-06 | 在书签起点边界插入文字 → 文字在书签外；在书签内部插入 → 书签扩展；删除跨越起点的区间 → 起点移到删除点 |
| SPAN-07 | 删除整段批注文本 → 批注条目消失且 `comments.xml` 变脏；删除整段书签文本 → 空书签留在原处 |
| SPAN-08 | 未移动的标记保存后字节不变；移动后的标记属性完整 |
| SPAN-09 | 人为构造孤儿 `bookmarkEnd` → PreExistingDamage 诊断且保存成功；编辑引擎制造孤儿 → 调试构建报错 |
