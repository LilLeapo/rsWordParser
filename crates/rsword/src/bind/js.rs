//! JS 绑定的**语言无关**那一半（`spec/18` 7.10；M8′ 8.0② 加 `parse_diagnostics` 与 blank 选项）。
//!
//! `crates/rsword-js` 只是把这里的函数接到 wasm-bindgen 上；绑定层的 JSON 形态与错误映射
//! 因此能在原生构建里逐份对照全语料（`tests/js_binding.rs`），再由 `tools/js-parity/` 在
//! node 里对真实 wasm 产物复核字节相同。
//!
//! 面很窄，与 TS `docx-engine` 的入口一一对应：
//!
//! | 这里 | TS |
//! | --- | --- |
//! | [`parse`] | `parseDocx(bytes)` → `ParsedDoc` JSON |
//! | [`parse_diagnostics`] | —（`Document.warnings` 的投影，单独出口） |
//! | [`save`] | `saveDocx(doc, blocks, options)` → 新的 docx 字节 |
//! | [`blank`] | `buildBlankDocx(options)` |
//! | [`version`] | — |
//!
//! 错误一律是 `{ code, message }`：`code` 取诊断码（`EDIT_BAD_POSITION` 一类）或错误变体名，
//! JS 侧照它分支，不去 parse 人话。调用方传了不是 JSON 的参数是契约错误，不是文档的问题，
//! 统一报 `BIND_BAD_ARGUMENT`（`DiagCode::BindBadArgument`）。

use crate::bind::compat_ts::{apply_save_blocks, parsed_doc};
use crate::diag::{DiagCode, ValidationOrigin};
use crate::edit::EditSession;
use crate::error::Error;
use crate::package::Package;
use crate::save::SaveOptions;

/// 绑定层的错误。`code` 稳定、可依赖；`message` 是给人看的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

impl ApiError {
    fn new(code: &str, message: impl Into<String>) -> Self {
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

/// 调用方契约错误（参数不是合法 JSON、类型不对、漏了必填项）：`BIND_BAD_ARGUMENT`。
fn bad_argument(message: impl Into<String>) -> ApiError {
    ApiError::new(DiagCode::BindBadArgument.as_str(), message)
}

/// 引擎版本（`Cargo.toml` 的 `version`）。
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// `parseDocx`：一份 docx → `ParsedDoc` JSON 文本（`COMPAT-02`，与 `compat_ts` 逐字节相同）。
pub fn parse(bytes: &[u8]) -> Result<String, ApiError> {
    let mut pkg = Package::open(bytes)?;
    let json = parsed_doc(&mut pkg)?;
    serde_json::to_string(&json)
        .map_err(|e| ApiError::new("JSON_ENCODE", format!("ParsedDoc 序列化失败: {e}")))
}

/// `Document.warnings` 的 JSON 数组（`Diagnostic` 的投影），单独出口——不进 `parse` 的
/// JSON，否则 `COMPAT-02` 的逐字节合同就被打破。
pub fn parse_diagnostics(bytes: &[u8]) -> Result<String, ApiError> {
    let mut pkg = Package::open(bytes)?;
    let doc = crate::model::Document::rebuild(&mut pkg)?;
    let warnings: Vec<serde_json::Value> = doc
        .warnings
        .iter()
        .map(|d| {
            let mut w = serde_json::json!({
                "part": d.part.0,
                "code": d.code.as_str(),
                "origin": match d.origin {
                    ValidationOrigin::PreExistingDamage => "preExistingDamage",
                    ValidationOrigin::EngineInvariantViolation => "engineInvariantViolation",
                },
                "message": d.message,
            });
            if let Some(range) = &d.range {
                w["range"] = serde_json::json!([range.start, range.end]);
            }
            w
        })
        .collect();
    serde_json::to_string(&warnings)
        .map_err(|e| ApiError::new("JSON_ENCODE", format!("诊断序列化失败: {e}")))
}

/// `saveDocx`：源字节 + `finalBlocks` JSON + `SaveOptions` JSON → 新的 docx 字节
/// （`COMPAT-08`，与 `apply_save_blocks` + `save_with` 同一条路）。
/// `finalBlocks` 是**必填**的：`saveDocx(doc, [])` 的语义是"这份文档现在一个块都没有"，
/// 会把正文清空。传空串 / `null` 只可能是调用方漏了参数，直接报错，不当成 `[]`。
pub fn save(bytes: &[u8], blocks_json: &str, options_json: &str) -> Result<Vec<u8>, ApiError> {
    let t = blocks_json.trim();
    if t.is_empty() || t == "null" {
        return Err(bad_argument(
            "finalBlocks 是必填的：空的 `[]` 表示「正文清空」，漏传参数不该走到那一步",
        ));
    }
    let blocks = json_arg(blocks_json, "finalBlocks")?;
    let options = json_arg(options_json, "SaveOptions")?;
    let mut session = EditSession::open(bytes)?;
    let outcome = apply_save_blocks(&mut session, &blocks, &options)?;
    Ok(session.save_with(&outcome.save_options)?)
}

/// `buildBlankDocx`：一份最小可用的空白文档（`SAVE-05`，7.8a）。
/// `options_json` 是 `BlankDocxOptions`（`{ eastAsiaFont?: string }`）的 JSON 或空串 / `null`；
/// 给了 `eastAsiaFont` 才写 docDefaults 的 `w:eastAsia`。
pub fn blank(options_json: Option<String>) -> Result<Vec<u8>, ApiError> {
    let east_asia = match options_json {
        None => None,
        Some(s) if s.trim().is_empty() || s.trim() == "null" => None,
        Some(s) => {
            let v: serde_json::Value = serde_json::from_str(&s)
                .map_err(|e| bad_argument(format!("BlankDocxOptions 不是合法 JSON: {e}")))?;
            match v.get("eastAsiaFont") {
                None | Some(serde_json::Value::Null) => None,
                Some(serde_json::Value::String(f)) => Some(f.clone()),
                Some(_) => return Err(bad_argument("BlankDocxOptions.eastAsiaFont: 不是字符串")),
            }
        }
    };
    Ok(crate::save::blank_docx(east_asia.as_deref())?)
}

/// 空串 / `null` 当成缺省的空对象（`SaveOptions` 侧 JS 常这么传）。
fn json_arg(text: &str, what: &str) -> Result<serde_json::Value, ApiError> {
    let t = text.trim();
    if t.is_empty() || t == "null" {
        return Ok(serde_json::Value::Object(Default::default()));
    }
    serde_json::from_str(t).map_err(|e| bad_argument(format!("{what} 不是合法 JSON: {e}")))
}

/// `SaveOptions` 的缺省（`save` 之外的调用方偶尔要）。
pub fn default_save_options() -> SaveOptions {
    SaveOptions::default()
}
