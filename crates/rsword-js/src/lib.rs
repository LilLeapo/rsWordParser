//! `rsword` 的 JS 绑定（`spec/18` 7.10）：wasm-bindgen，跑在渲染进程里，与 TS `docx-engine`
//! 同一个位置。M8「编辑器切换到 Rust 引擎」的入口。
//!
//! 这里只做**类型转换**：真正的实现在 `rsword::bind::js`，`diff-parse --via js` 走的是同一份，
//! 所以绑定层的 JSON 形态与错误映射能在原生构建里对着全语料比，不用起 node。
//!
//! ```js
//! import init, { parse, save, blank, version } from './rsword_js.js'
//! await init()
//! const doc = JSON.parse(parse(bytes))            // ParsedDoc
//! const out = save(bytes, JSON.stringify(blocks), JSON.stringify(options))
//! ```
//!
//! 错误抛成 JS `Error`，带 `code`（`EDIT_BAD_POSITION` 一类，稳定可依赖）与 `message`。
//!
//! 构建：`cargo build -p rsword-js --release --target wasm32-unknown-unknown`，
//! 再 `wasm-bindgen --target web --out-dir pkg target/wasm32-unknown-unknown/release/rsword_js.wasm`。

use rsword::bind::js;
use wasm_bindgen::prelude::*;

/// [`js::ApiError`] → JS `Error`（带 `code` / `message` 两个属性）。
fn throw(e: js::ApiError) -> JsValue {
    let err = js_sys::Error::new(&e.message);
    err.set_name("RswordError");
    let _ = js_sys::Reflect::set(&err, &JsValue::from_str("code"), &JsValue::from_str(&e.code));
    err.into()
}

/// 引擎版本。
#[wasm_bindgen]
pub fn version() -> String {
    js::version().to_string()
}

/// `parseDocx`：docx 字节 → `ParsedDoc` JSON 文本。
#[wasm_bindgen]
pub fn parse(bytes: &[u8]) -> Result<String, JsValue> {
    js::parse(bytes).map_err(throw)
}

/// `saveDocx`：源字节 + `finalBlocks` JSON + `SaveOptions` JSON → 新的 docx 字节。
#[wasm_bindgen]
pub fn save(bytes: &[u8], blocks_json: &str, options_json: &str) -> Result<Vec<u8>, JsValue> {
    js::save(bytes, blocks_json, options_json).map_err(throw)
}

/// `buildBlankDocx`：一份最小可用的空白文档。`east_asia_font` 给 `docDefaults` 的 `w:eastAsia`。
#[wasm_bindgen]
pub fn blank(east_asia_font: Option<String>) -> Result<Vec<u8>, JsValue> {
    js::blank(east_asia_font.as_deref()).map_err(throw)
}
