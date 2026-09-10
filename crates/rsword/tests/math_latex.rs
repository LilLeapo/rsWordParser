//! `latexToOmml` 的移植（`spec/18` 7.5）：与 TS 的输出**逐字相等**。
//!
//! 对照件在 `fixtures/fieldgen/latex.json`（TS `src/math.ts` 的输出记录，见那里的 README）。

mod common;

use rsword::xml::dom::{Latex, Omml};
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
        let got = Omml::try_from(Latex::from(src.as_str()))
            .map(String::from)
            .unwrap_or_else(|e| panic!("{src:?}: {e}"));
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
        let got = Omml::try_from(Latex::from(src.as_str())).map(String::from);
        assert_eq!(got.is_err(), ts_failed, "{src:?}: TS={ts_msg:?} 我们={got:?}");
    }
}

/// `mathParagraphXml` 的三种对齐。
#[test]
#[cfg(feature = "compat-ts")]
fn math_paragraph_matches_ts() {
    let fx = fixture();
    let body = Omml::try_from(Latex::from("a^2")).unwrap();
    for (align, want) in fx["paragraphs"].as_object().expect("paragraphs 段") {
        assert_eq!(&body.paragraph(align), want.as_str().expect("字符串"), "{align}");
    }
}

/// 深度 300 的输入 → `Err`，不爆栈（`spec/18` 风险 11：用户输入用递归 + 深度上限）。
#[test]
fn latex_depth_limit() {
    let deep = format!("{}x{}", "\\frac{".repeat(300), "}{1}".repeat(300));
    let err =
        Omml::try_from(Latex::from(deep.as_str())).map(String::from).expect_err("超过深度上限");
    assert!(
        matches!(&err, rsword::Error::Edit { code, .. }
            if *code == rsword::DiagCode::EditMathTooDeep),
        "{err}"
    );
    // 上限之内的照常算得出来
    let ok = format!("{}x{}", "\\frac{".repeat(100), "}{1}".repeat(100));
    assert!(Omml::try_from(Latex::from(ok.as_str())).map(String::from).is_ok());
}

/// 多字节字符的上下标边界与根次数子解析器保持字符语义。
#[test]
fn latex_borrowed_unicode_boundaries() {
    let prefix = Omml::try_from(Latex::from("α")).map(String::from).unwrap();
    let suffix = Omml::try_from(Latex::from("𝑥^2")).map(String::from).unwrap();
    assert_eq!(
        Omml::try_from(Latex::from("α𝑥^2")).map(String::from).unwrap(),
        format!("{prefix}{suffix}")
    );
    assert_eq!(
        Omml::try_from(Latex::from("α𝑥_2")).map(String::from).unwrap(),
        format!("{prefix}{}", Omml::try_from(Latex::from("𝑥_2")).map(String::from).unwrap())
    );
    let degree = Omml::try_from(Latex::from("α^2")).map(String::from).unwrap();
    let body = Omml::try_from(Latex::from("𝑥")).map(String::from).unwrap();
    assert_eq!(
        Omml::try_from(Latex::from(r"\sqrt[α^2]{𝑥}")).map(String::from).unwrap(),
        format!("<m:rad><m:deg>{degree}</m:deg><m:e>{body}</m:e></m:rad>")
    );
    assert!(Omml::try_from(Latex::from(r"\sqrt[α")).map(String::from).is_err());
    assert!(Omml::try_from(Latex::from(r"\left(α\righ")).map(String::from).is_err());
    assert!(Omml::try_from(Latex::from(r"\left(α\right)")).map(String::from).is_ok());
}
