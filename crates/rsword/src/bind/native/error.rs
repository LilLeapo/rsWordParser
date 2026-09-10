//! 语言无关的协议错误（`BIND-07`）；引擎错误码原样保留。

use crate::error::Error;

/// 绑定层的错误。`code` 稳定、可依赖；`message` 是给人看的。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[non_exhaustive]
#[cfg_attr(rsword_api_docs, deny(missing_docs))]
pub struct ApiError {
    /// 稳定机器码；调用方应按此分支处理错误。
    pub code: String,
    /// 人类可读信息，不承诺文本稳定。
    pub message: String,
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl ApiError {
    pub(crate) fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.to_string(), message: message.into() }
    }
}

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        let code = match &e {
            Error::LatexUnsupported => "LATEX_UNSUPPORTED",
            Error::OpenDocument(_) | Error::MissingMainPart => "NOT_OOXML",
            #[cfg(test)]
            Error::Absent | Error::Offset(_) | Error::Context { .. } | Error::Wrapped(_) => "TEST",
            Error::Limit { code, .. } | Error::Edit { code, .. } | Error::EditPlan { code, .. } => {
                code.as_str()
            }
            Error::EditUnsupported { .. } => crate::diag::DiagCode::EditUnsupported.as_str(),
            Error::Zip(_) => "ZIP",
            Error::Malformed { .. } => "XML_MALFORMED",
            Error::Invariant(d) => d.code.as_str(),
        };
        ApiError::new(code, e.to_string())
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::DiagCode;

    #[test]
    fn flattened_package_errors_preserve_protocol() {
        for (error, message) in [
            (Error::MissingMainPart, "not a docx: missing main document part"),
            (
                Error::OpenDocument("application/vnd.oasis.opendocument.text".into()),
                "OpenDocument file (application/vnd.oasis.opendocument.text) is not OOXML",
            ),
        ] {
            assert!(std::error::Error::source(&error).is_none());
            let api = ApiError::from(error);
            assert_eq!(api.code, "NOT_OOXML");
            assert_eq!(api.message, message);
        }
    }

    #[test]
    fn legacy_edit_errors_preserve_protocol_codes_and_messages() {
        for (error, code) in [
            (
                Error::EditPlan {
                    code: DiagCode::EditBadPosition,
                    message: "invalid cursor".into(),
                },
                DiagCode::EditBadPosition,
            ),
            (Error::EditUnsupported { operation: "legacy operation" }, DiagCode::EditUnsupported),
        ] {
            let message = error.to_string();
            let api = ApiError::from(error);
            assert_eq!(api.code, code.as_str());
            assert_eq!(api.message, message);
        }
    }
}
