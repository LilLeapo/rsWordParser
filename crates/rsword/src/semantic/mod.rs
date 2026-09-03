//! L3 语义层：属性表、`ParagraphFacts`、分类规则表（`spec/05-properties.md`、`spec/06-model.md`
//! `MOD-04/05`，`docs/03` §6.1–6.2）。
//!
//! 属性表由 `build.rs` 从声明式表格生成读取 / 比较 / 合并写回（`PROP-07`）；
//! 分类是 facts 的纯函数，用按优先级排列的规则表实现，每条可单测。M1 任务 1.1–1.3、1.6–1.7。
