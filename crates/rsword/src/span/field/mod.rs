//! 字段子系统（`spec/04-field.md`，`docs/03` §5.4）。
//!
//! `w:fldChar` begin/separate/end 与 `w:fldSimple` 配对为正确嵌套的 `FieldSpan`；
//! 指令只做 tokenization 与关键字提取（`FLD-05`），策略表（`FLD-06/07`）决定显示形态、
//! 可编辑性与保存行为。保存真相永远是 `instr_nodes` 的原字节。M2 实现；M1 只有 id 类型。

/// 字段的会话内稳定 id。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FieldId(pub u32);
