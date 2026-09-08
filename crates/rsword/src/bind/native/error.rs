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
            Error::NotOoxml(_) => "NOT_OOXML",
            Error::Limit { code, .. } | Error::Edit { code, .. } => code.as_str(),
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
