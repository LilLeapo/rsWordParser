# edit 重构实施记录

基线：`843d898`。开始时 edit 目录已只有 `mod.rs`，不重复执行文件合并。
已有用户改动：`docs/13-public-api.md`、`fuzz/corpus/.gitignore`；未跟踪的
`PROMPT.md`、`PROMPT_EDIT.md`、`tools/merge-edit-text.py`。这些内容不纳入任务提交。
基线 `cargo check -p rsword` 通过；`cargo test -p rsword --lib`：425 通过。

## 依赖与剩余迁移清单

当前调用方 → 被依赖方：

```mermaid
graph TD
  bindings[bind / agent / examples] --> edit
  edit --> model
  edit --> xml
  edit --> package
  edit --> props[semantic::props / span / resolve]
  edit --> error
  edit --> save
  save --> edit
```

`save` 与 `edit` 的双向依赖尚需拆解：保存编排保留在 save，共享数据契约与
编辑状态维护分开处理。当前图不表示该问题已经解决。

| 当前项 | 目标归属 | 调用方 / 可见性 | 所有权、布局与宏决策 |
| --- | --- | --- | --- |
| `Act::Restore` 静态保留字段表 | 私有修订策略枚举 | `MutationPlan::restore` | 按值策略替代静态切片；u8 标签，无堆分配 |
| `DROPPABLE_EMPTY` | 修订动作的私有判定方法 | 清理与还原计划 | 判定不需要容器所有权 |
| 表格自由函数 | EditSession / MutationPlan / Geometry | 编辑分派、修订处理；内部私有 | 计划继续拥有 NodeId 序列，查询优先借用 |
| track / twin 自由函数 | Tracker / EditSession | 修订与孪生同步 | 修改前快照与 part 配对保留 |
| shape 生成自由函数 | 生成类型 / EditSession | NewBlock 编辑入口 | 保留递归 Box；审查单位转换 |
| CT、part 与命名空间常量 | 语义枚举与 XML 现有枚举 | 编辑与保存调用方 | 标准 From 转换，字符串允许静态生命周期 |
| 分散领域错误 | error.rs 的 declare_error! | 解析、绑定、agent、保存 | 保留结构化载荷、格式与 source |
| 旧 locate / tests 文件历史 | 定位类型 / test_edit | 需核对 git 历史 | 不以当前文件缺失推定历史行为已完整迁移 |

这是一份进行中的审计记录；全部硬性规则和最终验证完成前不宣称完成。

## 已验证单元：修订还原策略

- `PPR_KEEP` / `SECT_KEEP` / `ROW_KEEP` / `CELL_KEEP` → 私有 `RevisionKeep`
  的 Paragraph / Section / Row / Cell 变体。None 表示没有排除字段，保持旧空表语义。
- `Act::Restore` 不再含静态容器引用；策略以 `repr(u8)` 存储，无分配。
  `DROPPABLE_EMPTY` → 私有 inline 判定 `Act::droppable_empty`。
- `MutationPlan::unwrap`、`restore` 的三处临时 NodeId Vec 改为 DOM 借用迭代器。
  DOM 在整个计划构建期间不可变，计划持有待执行命令，未改变提交与校验顺序。
  快照克隆、计划集合、合法 Option 与递归 Box 均未改变。
- 检查：fmt、diff --check、rsword check、库测试（425 通过），以及
  revisions / revision_grid / tracked_ops 专项回归。

## 已验证单元：修订使用的表格查询

- `cell_column`、`column_cells`、`grid_cols`、`absorb_cell_width` 成为
  EditSession 的私有 inline 方法，删除原受限可见性自由函数。
- 前三个查询直接借用会话 Document 中的 TableBlock / Row，无几何快照分配。
  `RowGeometry::cell_spans` 按原 gridBefore 与 gridSpan 顺序计算位置。
  缺表仍返回 None 或空迭代器，不新增错误或默认表格。
- 整列守卫与删除计划重新创建同语义借用迭代器；提交前 Document 不变。
  单格吸收宽度暂保留几何数据，避免把相邻格选择与这次查询变更混在一起。
- check、fmt、diff --check、库测试 425 项以及修订专项回归通过。

## 表格操作方法迁移

`table_of`、`insert_row`、`delete_row`、`insert_column`、`delete_column`、
`merge_cells`、`bump_row_gap`、`patch_cell_span`、`shift_bookmark_columns`
归入 EditSession 私有方法。原分派、错误信息、计划写入和提交顺序保持不变。
使用 syn 解析器按选定 ItemFn 和 ExprCall 的源码跨度生成副本，先审阅完整补丁，
再写入工作树；同名的一参数局部 `table_of` 闭包未误改。
check、fmt、diff --check、库测试 425 项以及 edit / cell_edit / revision_grid 回归通过。

历史核对：`5a140b2^` 中 locate.rs 没有模块声明或 include/path 引用。
它是早期主正文专用定位实现，当前定位已支持 part 与段内边界；其唯一测试的
`A😀B` 六个边界断言在 `position_utf16_text_conversion_preserves_boundaries_and_errors`
中完整保留，并区分代理对与越界的现行诊断。不恢复重复类型或过时调用路径。

## 通用 DOM 查询归属

- `element_children` → XML 层 `Dom::live_element_children`：借用迭代器，
  维持原始顺序、元素筛选和 Deleted 过滤，不改变为 MCE 语义展开。
- `direct_child` → `Dom::direct_child_containing`，保留包装锚点查找行为。
- `table_ops_child_named` 删除，调用方传完整 QName 给既有
  `Dom::live_children_named(...).next()`；`table_ops_w` 删除，使用 QName::w。
- 移除首项查询的临时集合；计划生成继续只读 DOM。新断言检查原始节点顺序、
  删除过滤、后代包装与不属于容器的节点。check、fmt、diff 检查、425 项库测试和
  edit / cell_edit / revision_grid 回归通过。
- `cargo check --workspace` 在表格方法单元后通过。基础层源码扫描未发现对编辑类型
  的实现依赖，仍有旧文档链接待统一更新；save 双向编排依赖仍待处理。
