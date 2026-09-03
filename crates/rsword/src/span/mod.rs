//! L2 范围层（`spec/03-span.md`，`docs/03` §5.1–5.3、5.5、5.6）。
//!
//! 书签、批注、权限、移动范围、customXml 修订范围的端点是 run 的兄弟元素，可任意交叠，
//! 树表达不了：这里用附着在 DOM 上的 `Anchor`（容器 + 内容序列边界 + affinity）与平铺的
//! `RangeSpan` 列表表示，并在编辑时维护。字段子系统见 [`field`]。
//!
//! 里程碑：M1 只需 `FlowId` 映射（任务 1.8，`SPAN-01`）；完整索引与 Anchor 变换在 M2。

pub mod field;
