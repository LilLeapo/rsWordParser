//! `rsword` 的 wasm 绑定（`spec/18` 7.10 的面 + M8′ 8.0② 的扩展）：`parse` /
//! `parse_diagnostics` / `save` / `blank` / `version`。差分工具（`diff-parse --via-js`、
//! `tools/js-parity/`）继续走这五个兼容入口；8.4 新增 [`native::NativeSessions`] 有状态原生协议。
//!
//! 两条合同：
//!
//! 1. **无状态**：每次调用从字节开始，wasm 里不留会话——M8′ 的有状态会话由 `bind::native`
//!    接管（`spec/19` 8.4）。
//! 2. **逐字节相同**：`parse` 的输出 = `bind::compat_ts::parsed_doc` 的
//!    `serde_json::to_string`（`COMPAT-02`）；`save` 的输出 = `apply_save_blocks` +
//!    `save_with` 的原样字节（`COMPAT-08`）。
//!
//! 这里只做**类型转换**：真正的实现在 `rsword::bind::js`，错误是它那层的 `ApiError`——
//! 抛成 JS `Error`，带 `code`（`DiagCode::as_str` 的字串或错误变体名）与 `message`；
//! 调用方自己的 JSON 参数错误是 `BIND_BAD_ARGUMENT`。五个导出同形，由 `bind_export!`
//! 一张表收拢：展开 wasm 包装与「非法输入 → 错误码」的原生单测（核心函数不走 JS 边界，
//! `cargo test --workspace` 就能跑）。

pub mod native;

use rsword::bind::js;
use wasm_bindgen::prelude::*;

rsword::bind_export! { adapter [wasm_bindgen] error(js_error, JsValue);
    /// TS `ParsedDoc` JSON（`COMPAT-02`）。逐字节合同：与 `compat_ts::parsed_doc` 的
    /// `serde_json::to_string` 完全一致；`internal.originalBytes` 不进 JSON（调用方自己持有字节）。
    parse(bytes: &[u8]) -> String => js::parse {
        test parse_rejects_garbage {
            assert_eq!(js::parse(NOT_A_ZIP).unwrap_err().code, "ZIP");
        }
    },
    /// `Document.warnings` 的 JSON 数组（`Diagnostic` 的投影），单独出口——
    /// 不进 `parse` 的 JSON，否则 `COMPAT-02` 的逐字节合同就被打破。
    parse_diagnostics(bytes: &[u8]) -> String => js::parse_diagnostics {
        test parse_diagnostics_rejects_garbage {
            assert_eq!(js::parse_diagnostics(NOT_A_ZIP).unwrap_err().code, "ZIP");
        }
    },
    /// 保存（`COMPAT-08`）：`bytes` = 原文档字节（TS `saveDocx` 从这里拿原字节），
    /// `blocks_json` / `options_json` = TS `SaveBlock[]` / `SaveOptions` 的字符串。
    /// 「全 original、顺序不变、无选项」时输出与输入逐字节相同（不变式 1）。
    save(bytes: &[u8], blocks_json: &str, options_json: &str) -> Vec<u8> => js::save {
        test save_rejects_garbage {
            assert_eq!(js::save(NOT_A_ZIP, "[]", "{}").unwrap_err().code, "ZIP");
        }
        test save_rejects_bad_blocks_json {
            let bytes = native_blank(None);
            assert_eq!(
                js::save(&bytes, "not json", "{}").unwrap_err().code,
                "BIND_BAD_ARGUMENT"
            );
        }
    },
    /// 空白模板（`SAVE-05`，TS `buildBlankDocx`）。`options_json` = `BlankDocxOptions`
    /// （`{ eastAsiaFont?: string }`）的 JSON 或 `null`；给了 `eastAsiaFont` 才写
    /// docDefaults 的 `w:eastAsia`。
    blank(options_json: Option<String>) -> Vec<u8> => js::blank {
        test blank_rejects_bad_options_json {
            assert_eq!(js::blank(Some("not json".into())).unwrap_err().code, "BIND_BAD_ARGUMENT");
        }
        test blank_east_asia_font_switches_doc_defaults {
            let none = js::blank(None).unwrap();
            assert_eq!(none, native_blank(None), "缺省输出与原生 blank_docx(None) 逐字节相同");
            let zh = js::blank(Some(r#"{"eastAsiaFont":"Microsoft YaHei"}"#.into())).unwrap();
            assert_eq!(zh, native_blank(Some("Microsoft YaHei")), "带 eastAsiaFont 的输出与原生逐字节相同");
            assert_ne!(none, zh, "两种选项必须产出不同的字节");
        }
    },
    /// 引擎版本 + rsWordParser 提交号（构建脚本嵌的 `RSWORD_GIT_SHA`）+ 协议版本的 JSON。
    version() -> String => core_version {
        test version_reports_protocol_and_commit {
            let v: serde_json::Value = serde_json::from_str(&core_version().unwrap()).unwrap();
            assert_eq!(v["protocol"], "compat/1");
            assert_eq!(v["version"], js::version());
            assert!(v["git"].is_string());
        }
    },
}

/// 非法输入的固定样本：连 zip 签名都没有的字节。
#[cfg(test)]
const NOT_A_ZIP: &[u8] = b"this is definitely not a zip archive";

/// 原生侧的空白模板字节（`blank_docx` 是绑定的逐字节对照物）。
#[cfg(test)]
fn native_blank(east_asia: Option<&str>) -> Vec<u8> {
    rsword::save::blank_docx(east_asia).unwrap()
}

/// [`js::ApiError`] → JS `Error`（带 `code` / `message` 两个属性）。
fn js_error(e: js::ApiError) -> JsValue {
    let err = js_sys::Error::new(&e.message);
    err.set_name("RswordError");
    let _ = js_sys::Reflect::set(&err, &JsValue::from_str("code"), &JsValue::from_str(&e.code));
    err.into()
}

/// `version` 的 JSON：`version` 引擎版本、`git` 构建时的提交号、`protocol` 这份绑定面的
/// 协议版本（`compat/1` = TS 兼容面；原生协议的版本由 `spec/21` BIND-08 另定）。
fn core_version() -> Result<String, js::ApiError> {
    Ok(serde_json::json!({
        "version": js::version(),
        "git": env!("RSWORD_GIT_SHA"),
        "protocol": "compat/1",
    })
    .to_string())
}

/// 有效 zip 但没有 `word/document.xml` 也没有 officeDocument 关系——`NOT_OOXML` 的路径。
/// 样例不进宏表：要现造一个 zip（测试用的字节，不值得提交二进制夹具）。
#[test]
fn parse_rejects_zip_without_main_part() {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    w.start_file("readme.txt", zip::write::SimpleFileOptions::default()).unwrap();
    use std::io::Write as _;
    w.write_all(b"not a wordprocessing package").unwrap();
    let bytes = w.finish().unwrap().into_inner();
    assert_eq!(js::parse(&bytes).unwrap_err().code, "NOT_OOXML");
}

/// 保存路径端到端：`original` 块原样回发 → 无编辑短路 → 输出与输入逐字节相同
/// （不变式 1；`blocks: []` 的语义是「清空文档」，不触发短路）。
#[test]
fn save_roundtrip_is_byte_identical() {
    let bytes = native_blank(None);
    let blocks = r#"[{"kind":"original","docxIndex":0}]"#;
    let out = js::save(&bytes, blocks, "{}").unwrap();
    assert_eq!(out, bytes, "无编辑保存必须逐字节相同");
}

/// 保存路径的坏 `options` JSON 同样是 `BIND_BAD_ARGUMENT`。
#[test]
fn save_rejects_bad_options_json() {
    let bytes = native_blank(None);
    assert_eq!(js::save(&bytes, "[]", "{").unwrap_err().code, "BIND_BAD_ARGUMENT");
}
