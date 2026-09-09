//! 保存校验与 Strict 编辑（任务 1.14 第一批，`SAVE-02/03`，M1 门第三条）：
//! `extra__strict-minimal` 改字后根命名空间仍为 Strict，新写的 `ST_OnOff` 为 `true/false`，
//! 改过的 `w:t` 带 `xml:space="preserve"`，其他 run 原字节不动；乱序的新子元素在调试构建下让 `save` 失败。

mod common;

use rsword::diag::{DiagCode, ValidationOrigin};
use rsword::package::{Package, PackageFlavor, PartFlavor};
use rsword::semantic::props::{Change, RunPropsPatch, plan_apply_run_props};
use rsword::xml::{LocalName, QName, xpath_strings};

fn strict_doc() -> Vec<u8> {
    let path = common::corpus_dir("synthetic").join("extra__strict-minimal.docx");
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn save_03_strict_document_stays_strict_after_edit() {
    let mut pkg = Package::open(&strict_doc()).unwrap();
    assert_eq!(pkg.flavor(), PackageFlavor::Strict);
    let main = pkg.main_part();
    let flavor = pkg.flavor_of(main);
    assert_eq!(flavor, PartFlavor::Strict);
    let dom = pkg.dom_mut(main).unwrap().unwrap();
    // 第一个 w:r：改字 + 显式关闭加粗
    let run =
        dom.descendants(dom.root()).find(|&n| dom.is(n, QName::w(LocalName::R))).expect("a run");
    let t = dom
        .children(run)
        .iter()
        .copied()
        .find(|&c| dom.is(c, QName::w(LocalName::T)))
        .expect("w:t");
    let text_node = dom.children(t)[0];
    let original_text = dom.text(text_node).unwrap().into_owned();
    let other_runs: Vec<String> = dom
        .descendants(dom.root())
        .filter(|&n| dom.is(n, QName::w(LocalName::R)) && n != run)
        .map(|n| dom.lex_str(&dom.node(n).lex.as_ref().unwrap().range).to_string())
        .collect();
    dom.set_text(text_node, format!("{original_text}!"));
    let rpr = dom.children(run).iter().copied().find(|&c| dom.is(c, QName::w(LocalName::RPr)));
    let patch = RunPropsPatch { bold: Change::Set(false), ..Default::default() };
    let edits = plan_apply_run_props(dom, run, rpr, &patch, flavor);
    dom.apply_edits(&edits);

    let saved = pkg.save().unwrap();
    let mut again = Package::open(&saved).unwrap();
    assert_eq!(again.flavor(), PackageFlavor::Strict, "改字后仍为 Strict");
    let main2 = again.main_part();
    let dom2 = again.dom(main2).unwrap().unwrap();
    let xml = dom2.src().to_string();
    assert!(
        xml.contains("http://purl.oclc.org/ooxml/wordprocessingml/main"),
        "根命名空间仍是 Strict URI"
    );
    assert!(!xml.contains("schemas.openxmlformats.org/wordprocessingml/2006/main"));
    assert_eq!(
        xpath_strings(dom2, "count(//w:b[@w:val='false'])").unwrap(),
        ["1"],
        "Strict 的 ST_OnOff 写 false"
    );
    assert_eq!(xpath_strings(dom2, "count(//w:b[@w:val='0'])").unwrap(), ["0"]);
    assert_eq!(xpath_strings(dom2, "//w:r[1]/w:t/text()").unwrap(), [format!("{original_text}!")]);
    assert_eq!(
        xpath_strings(dom2, "count(//w:r[1]/w:t[@xml:space='preserve'])").unwrap(),
        ["1"],
        "改过的 w:t 带 preserve"
    );
    for bytes in &other_runs {
        assert!(xml.contains(bytes), "未改的 run 原字节应原样出现: {bytes}");
    }
    assert!(!again.diagnostics().iter().any(|d| d.code == DiagCode::SaveInvariant));
}

#[test]
fn save_02_misordered_new_child_fails_save_in_debug_builds() {
    let mut pkg = Package::open(&strict_doc()).unwrap();
    let main = pkg.main_part();
    let dom = pkg.dom_mut(main).unwrap().unwrap();
    let run = dom.descendants(dom.root()).find(|&n| dom.is(n, QName::w(LocalName::R))).unwrap();
    let rpr = match dom.children(run).iter().copied().find(|&c| dom.is(c, QName::w(LocalName::RPr)))
    {
        Some(r) => r,
        None => {
            let r = dom.new_element(QName::w(LocalName::RPr));
            dom.insert_child(run, 0, r);
            r
        }
    };
    let sz = dom.new_element(QName::w(LocalName::Sz));
    dom.set_attr(sz, QName::w(LocalName::Val), "24");
    dom.append_child(rpr, sz);
    let b = dom.new_element(QName::w(LocalName::B));
    dom.append_child(rpr, b); // 绕过 plan_apply，w:b 落在 w:sz 之后
    let result = pkg.save();
    if cfg!(debug_assertions) {
        let err = result.expect_err("调试构建下乱序应报错");
        let msg = err.to_string();
        assert!(msg.contains("PROP-05"), "{msg}");
    } else {
        result.unwrap();
        assert!(pkg.diagnostics().iter().any(|d| d.code == DiagCode::SaveInvariant
            && d.origin == ValidationOrigin::EngineInvariantViolation));
    }
}
