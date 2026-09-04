//! 顶层错误。只有"根本不是 docx / 缺主 part / zip 超限 / 主 part 畸形 /
//! 编辑位置或计划非法 / 引擎不变式被破坏"返回 `Err`（`docs/01` §13.5、
//! `PKG-02/03`、`XML-08`、`EDIT-02/05`、`SAVE-02`）；其余一律局部降级并记
//! [`crate::Diagnostic`]。

use crate::diag::{DiagCode, Diagnostic};

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// `PKG-03`：输入不是 OOXML 文字处理文档。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotOoxml {
    /// 存在 `mimetype` 且以 `application/vnd.oasis.opendocument` 开头。
    #[error("OpenDocument file ({0}) is not OOXML")]
    OpenDocument(String),
    /// 既无 `word/document.xml` 也无 `_rels/.rels` 的 `officeDocument` 关系。
    #[error("not a docx: missing main document part")]
    MissingMainPart,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error(transparent)]
    NotOoxml(#[from] NotOoxml),

    /// `PKG-02`：在解压任何 part 之前按 central directory 声明大小拒绝。
    #[error("docx rejected ({code}): {message}")]
    Limit { code: DiagCode, message: String },

    /// zip 结构无法读取（不是限额问题）。
    #[error("zip: {0}")]
    Zip(String),

    /// `XML-08`：主 part 畸形。非主 part 畸形不走这里，而是降级为 `Opaque`。
    #[error("malformed XML in {part} at byte {offset}: {message}")]
    Malformed { part: String, offset: u32, message: String },

    /// `EDIT-02` / `EDIT-05`：位置或变更计划非法（用户输入或 plan 阶段缺陷）。
    #[error("edit rejected ({code}): {message}")]
    EditPlan { code: DiagCode, message: String },

    /// `EDIT-01`：`EditOp` 已进入公开 API，但对应操作语义在任务 1.12 才实现。
    #[error("edit operation `{operation}` is not implemented yet")]
    EditUnsupported { operation: &'static str },

    /// `SAVE-02`：调试构建与 CI 下的 `EngineInvariantViolation`。
    #[error("engine invariant violated: {0}")]
    Invariant(Diagnostic),
}
