//! `latexToOmml` 的移植（`spec/18` 7.5）：与 TS 的输出**逐字相等**。
//!
//! 对照件在 `fixtures/fieldgen/latex.json`（TS `src/math.ts` 的输出记录，见那里的 README）。

mod common;

use rsword::model::latex_to_omml;
#[cfg(feature = "compat-ts")]
use rsword::model::math_paragraph_xml;
#[cfg(feature = "compat-ts")]
use serde_json::Value;

#[cfg(feature = "compat-ts")]
fn fixture() -> Value {
    let path = common::repo_root().join("fixtures/fieldgen/latex.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("latex.json")).expect("JSON")
}

/// 42 条输入的 OMML 与 TS 逐字相等。
#[test]
#[cfg(feature = "compat-ts")]
fn latex_to_omml_matches_ts() {
    let fx = fixture();
    let omml = fx["omml"].as_object().expect("omml 段");
    assert!(omml.len() >= 40, "对照件只有 {} 条", omml.len());
    for (src, want) in omml {
        let got = latex_to_omml(src).unwrap_or_else(|e| panic!("{src:?}: {e}"));
        assert_eq!(&got, want.as_str().expect("字符串"), "{src:?}");
    }
}

/// 解析不了的输入两边都报错（措辞不比——那是 TS 的英文文案）。
#[test]
#[cfg(feature = "compat-ts")]
fn latex_errors_match_ts() {
    let fx = fixture();
    for (src, ts_msg) in fx["errors"].as_object().expect("errors 段") {
        let ts_failed = !ts_msg.as_str().unwrap_or_default().is_empty();
        let got = latex_to_omml(src);
        assert_eq!(got.is_err(), ts_failed, "{src:?}: TS={ts_msg:?} 我们={got:?}");
    }
}

/// `mathParagraphXml` 的三种对齐。
#[test]
#[cfg(feature = "compat-ts")]
fn math_paragraph_matches_ts() {
    let fx = fixture();
    let body = latex_to_omml("a^2").unwrap();
    for (align, want) in fx["paragraphs"].as_object().expect("paragraphs 段") {
        assert_eq!(&math_paragraph_xml(&body, align), want.as_str().expect("字符串"), "{align}");
    }
}

/// 深度 300 的输入 → `Err`，不爆栈（`spec/18` 风险 11：用户输入用递归 + 深度上限）。
#[test]
fn latex_depth_limit() {
    let deep = format!("{}x{}", "\\frac{".repeat(300), "}{1}".repeat(300));
    let err = latex_to_omml(&deep).expect_err("超过深度上限");
    assert!(
        matches!(&err, rsword::Error::Edit { code, .. }
            if *code == rsword::DiagCode::EditMathTooDeep),
        "{err}"
    );
    // 上限之内的照常算得出来
    let ok = format!("{}x{}", "\\frac{".repeat(100), "}{1}".repeat(100));
    assert!(latex_to_omml(&ok).is_ok());
}
