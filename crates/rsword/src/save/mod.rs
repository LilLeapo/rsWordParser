//! 保存（`spec/09-save.md`，`docs/03` §9）。
//!
//! 校验（`SAVE-02`，区分 `PreExistingDamage` 与 `EngineInvariantViolation`）→ 物化 Span →
//! 按 `Dirty` 序列化脏 part（`XML-13`）→ 包写回：未变 part 用 `zip::ZipWriter::raw_copy_file`
//! 直接拷压缩数据（`SAVE-06`）。无脏节点且无新增 part → 直接返回原字节（不变式 1）。
//! M0 任务 0.11–0.12 只做序列化与 `raw_copy_file` 写回。

pub mod package_writer;
pub mod serialize;

pub use serialize::{SerializeError, serialize, serialize_subtree};

/// `SAVE-07` 保存选项。`saved_at` 与 `remove_personal_info` 的 DOM 翻译在任务 1.14；
/// 任务 1.11 的 [`crate::edit::EditSession::save`] 只接受默认值。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveOptions {
    /// `core.xml` 的 `dcterms:modified`。
    pub saved_at: Option<String>,
    /// TS `SaveOptions.removePersonalInformation`。
    pub remove_personal_info: Option<bool>,
}
