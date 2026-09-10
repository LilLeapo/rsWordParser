//! 顶层错误。只有"根本不是 docx / 缺主 part / zip 超限 / 主 part 畸形 /
//! 编辑位置或计划非法 / 引擎不变式被破坏"返回 `Err`（`docs/01` §13.5、
//! `PKG-02/03`、`XML-08`、`EDIT-02/05`、`SAVE-02`）；其余一律局部降级并记
//! [`crate::Diagnostic`]。

use crate::diag::{DiagCode, Diagnostic};

// 分组显式使用 @group，避免类型属性、分组文档与变体属性之间的匹配歧义。
// 一次调用生成唯一的 Error 枚举；分组只组织声明，不产生额外错误类型。
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
    #[non_exhaustive]
    /// 包打开、编辑或保存的具名错误。枚举允许增加变体，调用方匹配时须保留兜底分支。
    #[cfg_attr(rsword_api_docs, deny(missing_docs))]
    pub enum Error {
        @group package {
            /// 存在 `mimetype` 且以 `application/vnd.oasis.opendocument` 开头。
            #[error("OpenDocument file ({0}) is not OOXML")]
            OpenDocument(String),
            /// 既无 `word/document.xml` 也无 `_rels/.rels` 的 `officeDocument` 关系。
            #[error("not a docx: missing main document part")]
            MissingMainPart,
        }
        @group formula {
            /// OMML 超出 LaTeX 投影子集；读取器局部回退到 token 编辑。
            #[error("OMML cannot be represented by the supported LaTeX subset")]
            LatexUnsupported,
        }
        @group operation {
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

            /// `EDIT-02` / `EDIT-05`：位置或变更计划非法（用户输入或 plan 阶段缺陷）。
            #[error("edit rejected ({code}): {message}")]
            EditPlan {
                /// 稳定诊断代码。
                code: DiagCode,
                /// 人类可读原因。
                message: String,
            },

            /// `EDIT-01`：`EditOp` 已进入公开 API，但对应操作语义在任务 1.12 才实现。
            #[error("edit operation `{operation}` is not implemented yet")]
            EditUnsupported {
                /// 尚未实现的操作名称。
                operation: &'static str,
            },

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
        /// 无字段与元组字段的错误。
        @group simple {
            #[cfg(test)]
            #[error("absent")]
            Absent,
            #[cfg(test)]
            #[error("offset {0}")]
            Offset(u32),
        }
        /// 具名字段、参数表达式、错误链与透明转换。
        @group causes {
            #[cfg(test)]
            #[error("{}: {cause}", .context.len())]
            Context {
                /// 上层操作的说明。
                context: String,
                #[source]
                cause: std::num::ParseIntError,
            },
            #[cfg(test)]
            #[error(transparent)]
            Wrapped(#[from] std::num::ParseIntError),
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

#[cfg(test)]
mod test_model {
    #[test]
    fn declarations_preserve_messages_conversions_and_sources() {
        let absent = super::Error::Absent;
        let _: &dyn std::error::Error = &absent;
        assert_eq!(absent.to_string(), "absent");
        assert_eq!(super::Error::Offset(7).to_string(), "offset 7");
        let cause = "invalid".parse::<u32>().unwrap_err();
        let _: &dyn std::error::Error = &cause;
        let context = super::Error::Context { context: "read".into(), cause: cause.clone() };
        assert_eq!(context.to_string(), format!("4: {cause}"));
        assert_eq!(std::error::Error::source(&context).unwrap().to_string(), cause.to_string());
        let wrapped = super::Error::from(cause.clone());
        assert_eq!(wrapped.to_string(), cause.to_string());
        assert!(std::error::Error::source(&wrapped).is_none());
        let error = super::Error::MissingMainPart;
        let _: &dyn std::error::Error = &error;
        assert_eq!(error.to_string(), "not a docx: missing main document part");
        assert!(std::error::Error::source(&error).is_none());
    }
}
