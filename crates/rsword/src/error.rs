//! 顶层错误。只有"根本不是 docx / 缺主 part / zip 超限 / 主 part 畸形 / 引擎不变式被破坏"
//! 才返回 `Err`（`docs/01` §13.5、`PKG-02`、`PKG-03`、`XML-08`、`SAVE-02`）；
//! 其余一律局部降级并记 [`crate::Diagnostic`]。

use crate::diag::{DiagCode, Diagnostic};

// 分组显式使用 @group，避免类型属性、分组文档与变体属性之间的匹配歧义。
// 变体正文使用 Rust 原生语法，保留 thiserror 的任意消息表达式及字段属性。
// @error 从调用处捕获 derive 路径，使 thiserror 的透明 source 绑定与字段属性使用相同
// 宏卫生上下文；在定义处硬编码 derive 路径会令当前工具链报 transparent 未绑定。
macro_rules! declare_error {
    (
        @error($error:path)
        $(#[$type_attr:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[doc = $group_doc:literal])*
                @group $group:ident { $($variants:tt)* }
            )+
        }
    ) => {
        $(#[$type_attr])*
        $(
            #[doc = concat!("\n## ", stringify!($group))]
            $(#[doc = $group_doc])*
        )+
        #[derive(Debug, $error)]
        $vis enum $name {
            $($($variants)*)+
        }
    };
}

pub type Result<T> = std::result::Result<T, Error>;

declare_error! {
    @error(thiserror::Error)
    /// `PKG-03`：输入不是 OOXML 文字处理文档。
    #[derive(Clone, PartialEq, Eq)]
    pub enum NotOoxml {
        @group package {
            /// 存在 `mimetype` 且以 `application/vnd.oasis.opendocument` 开头。
            #[error("OpenDocument file ({0}) is not OOXML")]
            OpenDocument(String),
            /// 既无 `word/document.xml` 也无 `_rels/.rels` 的 `officeDocument` 关系。
            #[error("not a docx: missing main document part")]
            MissingMainPart,
        }
    }
}

declare_error! {
    @error(thiserror::Error)
    #[non_exhaustive]
    /// 包打开、编辑或保存的具名错误。枚举允许增加变体，调用方匹配时须保留兜底分支。
    #[cfg_attr(rsword_api_docs, deny(missing_docs))]
    pub enum Error {
        @group operation {
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
    }
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl Error {
    /// 构造带稳定机器码的编辑拒绝错误。
    #[inline]
    pub fn edit(code: DiagCode, message: impl Into<String>) -> Self {
        Error::Edit { code, message: message.into() }
    }
}

declare_error! {
    @error(thiserror::Error)
    #[cfg(test)]
    #[derive(Clone, PartialEq, Eq)]
    enum DeclarationFixture {
        /// 无字段与元组字段的错误。
        @group simple {
            #[error("absent")]
            Absent,
            #[error("offset {0}")]
            Offset(u32),
        }
        /// 具名字段、参数表达式、错误链与透明转换。
        @group causes {
            #[error("{}: {cause}", .context.len())]
            Context {
                /// 上层操作的说明。
                context: String,
                #[source]
                cause: NotOoxml,
            },
            #[error(transparent)]
            Wrapped(#[from] NotOoxml),
        }
    }
}

#[cfg(test)]
mod test_model {
    #[test]
    fn declarations_preserve_messages_conversions_and_sources() {
        let absent = super::DeclarationFixture::Absent;
        let _: &dyn std::error::Error = &absent;
        assert_eq!(absent.to_string(), "absent");
        assert_eq!(super::DeclarationFixture::Offset(7).to_string(), "offset 7");
        let cause = super::NotOoxml::MissingMainPart;
        let _: &dyn std::error::Error = &cause;
        let context =
            super::DeclarationFixture::Context { context: "read".into(), cause: cause.clone() };
        assert_eq!(context, context.clone());
        assert_eq!(context.to_string(), "4: not a docx: missing main document part");
        assert_eq!(std::error::Error::source(&context).unwrap().to_string(), cause.to_string());
        let wrapped = super::DeclarationFixture::from(cause.clone());
        assert_eq!(wrapped.to_string(), cause.to_string());
        assert!(std::error::Error::source(&wrapped).is_none());
        let error = super::Error::from(cause);
        let _: &dyn std::error::Error = &error;
        assert_eq!(error.to_string(), "not a docx: missing main document part");
        assert!(std::error::Error::source(&error).is_none());
    }
}
