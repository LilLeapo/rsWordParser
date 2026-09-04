//! 字段子系统（`spec/04-field.md`，`docs/03` §5.4）。
//!
//! `w:fldChar` begin/separate/end 与 `w:fldSimple` 配对为正确嵌套的 [`FieldSpan`]（`FLD-01`/`FLD-02`）；
//! 指令只做 tokenization 与关键字提取（`FLD-05`），策略表（`FLD-06`/`FLD-07`）决定显示形态、
//! 可编辑性与保存行为。**保存真相永远是 `instr_nodes` 的原字节**：解析结果不得用来重新序列化指令。
//!
//! 与范围（`RangeSpan`）的区别：字段正确嵌套，而且 [`FieldSpan`] 里的每条事实都能从 DOM 重新读出来，
//! 所以它是 DOM 的投影——编辑之后重建即可，不需要像 `Anchor` 那样增量维护。
//!
//! 里程碑：M2 任务 2.4 建索引与策略；进模型与 compat 在 2.5，编辑操作在 2.9。

pub mod form;
pub mod index;
pub mod instr;

pub use form::{FormData, form_name, read_form_data};
pub use index::{FieldForm, FieldIndex, FieldSpan};
pub use instr::{FieldPolicy, InstrToken, Instruction, Keyword, NESTED_PLACEHOLDER};

/// 字段的会话内稳定 id。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FieldId(pub u32);
