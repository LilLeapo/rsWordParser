//! 保存（`spec/09-save.md`，`docs/03` §9）。
//!
//! 校验（`SAVE-02`，区分 `PreExistingDamage` 与 `EngineInvariantViolation`）→ 物化 Span →
//! 按 `Dirty` 序列化脏 part（`XML-13`）→ 包写回：未变 part 用 `zip::ZipWriter::raw_copy_file`
//! 直接拷压缩数据（`SAVE-06`）。无脏节点且无新增 part → 直接返回原字节（不变式 1）。
//! M0 任务 0.11–0.12 做序列化与 `raw_copy_file` 写回；M1 任务 1.14 加校验（[`validate`]）。

pub mod blank;
pub mod options;
pub mod package_writer;
pub mod prune;
pub mod serialize;
pub mod validate;

pub use blank::{BLANK_BULLET_NUM_ID, BLANK_ORDERED_NUM_ID, blank_docx};
pub use options::SaveOptions;
pub use serialize::{SerializeError, serialize, serialize_subtree};
pub use validate::{enforce, ensure_extension_declarations, validate_part};
