//! JS 绑定的语言无关那一半（`bind::js`，`spec/18` 7.10）。
//!
//! wasm 那层（`crates/rsword-js`）只做类型转换，所以这里比的就是绑定的全部行为：
//! `parse` 与 `compat_ts::parsed_doc` 逐字节相同、`save` 与原生 `save_blocks` 那条路字节相同、
//! `blank` 与 `save::blank_docx` 相同、错误映射成稳定的 `code`。

mod common;

use rsword::bind::compat_ts::{apply_save_blocks, parsed_doc};
use rsword::bind::js;
use rsword::edit::EditSession;
use rsword::package::Package;
use serde_json::Value;

/// `parse`：全语料上与 `compat_ts::parsed_doc` 的 JSON **逐字节相同**（`COMPAT-02`）。
#[test]
fn compat_02_js_parse_matches_the_native_projection() {
    let mut n = 0usize;
    for kind in ["synthetic", "real"] {
        for path in common::docx_paths(kind) {
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let native = Package::open(&bytes).and_then(|mut p| parsed_doc(&mut p));
            let via = js::parse(&bytes);
            match (native, via) {
                (Ok(v), Ok(text)) => {
                    assert_eq!(
                        serde_json::to_string(&v).unwrap(),
                        text,
                        "{}: 绑定的 JSON 与原生不同",
                        path.display()
                    );
                    n += 1;
                }
                (Err(_), Err(_)) => {}
                (a, b) => {
                    panic!("{}: 一边成功一边失败 {:?} / {:?}", path.display(), a.is_ok(), b.is_ok())
                }
            }
        }
    }
    assert!(n > 1000, "扫到的文档太少：{n}");
}

/// `save`：全部保存用例的结果与原生 `apply_save_blocks` + `save_with` **字节相同**（`COMPAT-08`）。
#[test]
fn compat_08_js_save_matches_the_native_path() {
    let dir = common::corpus_dir("synthetic");
    let mut n = 0usize;
    let mut refused = 0usize;
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().is_some_and(|f| f.to_string_lossy().contains(".save.")))
        .collect();
    files.sort();
    for path in &files {
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        let case: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let stem = file.split(".save.").next().unwrap();
        let Ok(bytes) = std::fs::read(dir.join(format!("{stem}.docx"))) else { continue };
        // 原生那条路
        let native = (|| {
            let mut s = EditSession::open(&bytes)?;
            let o = apply_save_blocks(&mut s, &case["blocks"], &case["options"])?;
            s.save_with(&o.save_options)
        })();
        // 绑定那条路：两个参数都是 JSON 文本
        let via = js::save(
            &bytes,
            &serde_json::to_string(&case["blocks"]).unwrap(),
            &serde_json::to_string(&case["options"]).unwrap(),
        );
        match (native, via) {
            (Ok(a), Ok(b)) => {
                assert_eq!(a, b, "{file}: 绑定与原生保存出的字节不同");
                n += 1;
            }
            (Err(a), Err(b)) => {
                // 两边都拒：错误码要对得上
                let code = match &a {
                    rsword::Error::Edit { code, .. } => code.as_str().to_string(),
                    other => format!("{other}"),
                };
                assert!(
                    code.contains(&b.code) || b.code == code,
                    "{file}: 错误码不同 原生={code} 绑定={}",
                    b.code
                );
                refused += 1;
            }
            (a, b) => panic!("{file}: 一边成功一边失败 {:?} / {:?}", a.is_ok(), b.is_ok()),
        }
    }
    eprintln!("js save: {n} 份字节相同、{refused} 份两边都拒");
    assert_eq!(n + refused, files.len(), "有用例没跑到");
    assert!(n > 200, "字节相同的太少：{n}");
}

/// `blank` 与 `version`，以及缺省参数（空串 / `null` 当缺省）。
#[test]
fn blank_version_and_default_arguments() {
    assert_eq!(js::blank(None).unwrap(), rsword::save::blank_docx(None).unwrap());
    assert_eq!(js::blank(Some("等线")).unwrap(), rsword::save::blank_docx(Some("等线")).unwrap());
    assert_eq!(js::version(), env!("CARGO_PKG_VERSION"));
    // `SaveOptions` 缺省成 `{}`；`finalBlocks` 是必填的（空的 `[]` 语义是"正文清空"，
    // 漏传参数不该悄悄走到那一步）
    let bytes = js::blank(None).unwrap();
    let e = js::save(&bytes, "", "{}").unwrap_err();
    assert_eq!(e.code, "JSON_PARSE", "{e:?}");
    let e = js::save(&bytes, "null", "").unwrap_err();
    assert_eq!(e.code, "JSON_PARSE", "{e:?}");
    // 真的传 `[]`：正文清空，与 TS `saveDocx(doc, [])` 同义
    let emptied = js::save(&bytes, "[]", "null").unwrap();
    assert_ne!(emptied, bytes, "空块表应当把正文清空");
    let doc: serde_json::Value = serde_json::from_str(&js::parse(&emptied).unwrap()).unwrap();
    assert!(
        doc["blocks"]
            .as_array()
            .is_some_and(|b| b.iter().all(|x| x["runs"].as_array().is_none_or(|r| r.is_empty()))),
        "清空之后不该还有文字: {}",
        doc["blocks"]
    );
}

/// 错误映射：`code` 稳定可依赖，`message` 只给人看。
#[test]
fn errors_carry_a_stable_code() {
    // 不是 zip
    let e = js::parse(b"not a docx").unwrap_err();
    assert!(!e.code.is_empty() && !e.message.is_empty(), "{e:?}");
    // JSON 不合法
    let bytes = js::blank(None).unwrap();
    let e = js::save(&bytes, "{", "{}").unwrap_err();
    assert_eq!(e.code, "JSON_PARSE", "{e:?}");
    let e = js::save(&bytes, "[]", "{").unwrap_err();
    assert_eq!(e.code, "JSON_PARSE", "{e:?}");
    // 编辑被拒 → 诊断码原样传出去
    let e = js::save(&bytes, "[]", r#"{"partXml": {"word/nope.xml": "<a/>"}}"#).unwrap_err();
    assert_eq!(e.code, "EDIT_TARGET_MISSING", "{e:?}");
}
