//! 顶层错误。只有"根本不是 docx / 缺主 part / zip 超限 / 主 part 畸形 / 引擎不变式被破坏"
//! 才返回 `Err`（`docs/01` §13.5、`PKG-02`、`PKG-03`、`XML-08`、`SAVE-02`）；
//! 其余一律局部降级并记 [`crate::Diagnostic`]。

macro_rules! declare_error {
    (
        // ⛔ Behind `@derive` rather than a leading `$(#[$attr:meta])*`, and ⛔ not behind
        // a bare `derive:` either. Two ambiguities to get past, both hard errors:
        // a leading attribute repetition collides with each group's own `#[doc]`
        // ("built-in NTs meta"), and a bare `derive` collides with `$group:ident`
        // ("built-in NTs ident"). `@` cannot start an identifier, so it settles both.
        $(@derive[$($derive:path),* $(,)?])?
        $(
        $(#[doc = $group_doc:literal])*
        $group:ident {
            $(
                $(#[$attr:meta])*
                // ⭐⭐ Three variant shapes, ⛔ not one. A bare variant, a tuple, and
                // **named fields** -- and the third is not a convenience: the message
                // formats its fields by name, so a variant with five of them reads
                // `{declared}`/`{implied}` instead of `{2}`/`{3}`. Forcing those into
                // tuples is how a message drifts from the value it prints.
                //
                // ⚠ The named arm comes **first**: `{` cannot start a type, so the two
                // are unambiguous, but a `tt`-munching matcher tries arms in order and
                // the tuple arm's `$(...)?` would otherwise match the empty case and
                // then choke on the brace.
                $variant:ident
                    $( { $($field:ident : $fty:ty),* $(,)? } )?
                    $( ( $($ty:ty),* $(,)? ) )?
                    => $message:literal
            ),* $(,)?
        }
    )*) => {
        // ⭐ `Debug` and `thiserror::Error` are what this macro is *for*, so they stay.
        // ⛔ `Clone`/`PartialEq`/`Eq` are **not** added here: whether an error enum can
        // have them is decided by the fields the caller puts in it -- `io::Error` is
        // none of the three -- so the caller writes those `#[derive(..)]` itself, above
        // its first group. Deriving them here made every caller's field list answer to
        // this macro instead.
        #[derive(Debug, thiserror::Error $(, $($derive),*)?)]
        pub enum Error {
            $($(
                $(#[$attr])*
                #[error($message)]
                $variant
                    $( { $($field : $fty),* } )?
                    $( ( $($ty),* ) )?,
            )*)*
        }
    };
}

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
/// 包打开、编辑或保存的具名错误。枚举允许增加变体，调用方匹配时须保留兜底分支。
#[cfg_attr(rsword_api_docs, deny(missing_docs))]
pub enum Error {
    /// 输入不是 OOXML 文字处理文档。
    #[error(transparent)]
    NotOoxml(#[from] NotOoxml),

    /// `PKG-02`：在解压任何 part 之前按 central directory 声明大小拒绝。
    #[error("docx rejected ({code}): {message}")]
    Limit {
        /// 被触发的限额代码。
        code: DiagCode,
        /// 人类可读原因。
        message: String,
    },

    /// zip 结构无法读取（不是限额问题）。
    #[error("zip: {0}")]
    Zip(String),

    /// `XML-08`：主 part 畸形。非主 part 畸形不走这里，而是降级为 `Opaque`。
    #[error("malformed XML in {part} at byte {offset}: {message}")]
    Malformed {
        /// 包内 part URI。
        part: String,
        /// 原 XML 中的字节偏移。
        offset: u32,
        /// 人类可读原因。
        message: String,
    },

    /// `SAVE-02`：调试构建与 CI 下的 `EngineInvariantViolation`。
    #[error("engine invariant violated: {0}")]
    Invariant(Diagnostic),

    /// `EDIT-01/02/03/05`：编辑操作被拒绝（位置非法、跨段、计划校验失败、当前阶段不支持）。
    /// 会话状态不变（`EDIT-05`）。
    #[error("edit rejected ({}): {message}", code.as_str())]
    Edit {
        /// 稳定诊断代码。
        code: DiagCode,
        /// 人类可读原因。
        message: String,
    },
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl Error {
    /// 构造带稳定机器码的编辑拒绝错误。
    pub fn edit(code: DiagCode, message: impl Into<String>) -> Self {
        Error::Edit { code, message: message.into() }
    }
}
