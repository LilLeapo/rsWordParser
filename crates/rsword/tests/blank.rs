//! 空白文档模板（`SAVE-05`，`spec/18` 7.8）：与 TS `buildBlankDocx` 的六个 part 逐字节相同。
//!
//! 对照件 `fixtures/fieldgen/blank.json` 由 `tools/export-golden/blankgen.export.test.ts` 导出。

mod common;

use rsword::edit::EditSession;
#[cfg(feature = "compat-ts")]
use rsword::edit::{BlockAt, BlockPos, EditContext, EditOp, NewBlock, NewInline, NewRun};
#[cfg(feature = "compat-ts")]
use rsword::xml::{LocalName, QName};

#[cfg(feature = "compat-ts")]
fn fixture() -> serde_json::Value {
    let path = common::repo_root().join("fixtures/fieldgen/blank.json");
    serde_json::from_slice(&std::fs::read(&path).expect("blank.json")).expect("JSON")
}

/// 六个 part 与 TS 的输出逐字节相同（不给 `w:eastAsia` 与给了各一遍）。
#[test]
#[cfg(feature = "compat-ts")]
fn blank_parts_match_the_ts_template_byte_for_byte() {
    let want = fixture();
    for (key, font) in [("default", None), ("eastAsia", Some("等线"))] {
        let bytes = rsword::save::blank_docx(font).expect("blank");
        let want = want[key].as_object().expect("part 表");
        let got: Vec<(&str, String)> = rsword::save::blank::blank_parts(font);
        assert_eq!(got.len(), want.len(), "{key}: part 数");
        for (name, xml) in &got {
            let w = want[*name].as_str().unwrap_or_else(|| panic!("{key}: TS 里没有 {name}"));
            assert_eq!(xml, w, "{key}: {name} 与 TS 不同");
            // 落进 zip 之后也一样
            assert_eq!(
                String::from_utf8_lossy(&common::part_bytes(&bytes, name)),
                *w,
                "{key}: {name} 在包里"
            );
        }
    }
    assert_eq!(rsword::save::BLANK_BULLET_NUM_ID, want["bulletNumId"].as_str().unwrap());
    assert_eq!(rsword::save::BLANK_ORDERED_NUM_ID, want["orderedNumId"].as_str().unwrap());
}

/// 打开就能用：一个可见段落，标准样式都在，未编辑保存字节不变（不变式 1）。
#[test]
fn blank_opens_to_one_paragraph_with_the_standard_styles() {
    let bytes = rsword::save::blank_docx(None).expect("blank");
    let mut s = EditSession::blank(None).expect("blank session");
    assert_eq!(s.document().text_blocks().count(), 1, "一个段落");
    let styles = s.document().styles.as_ref().expect("styles.xml");
    for id in
        ["Normal", "Heading1", "Heading2", "Heading3", "Heading6", "ListParagraph", "TOC1", "TOC9"]
    {
        assert!(styles.get(id).is_some(), "样式 {id} 应当在");
    }
    assert_eq!(s.save().expect("save"), bytes, "没编辑就一个字节不动");
    assert!(s.diagnostics().is_empty(), "打开空白模板不该有诊断: {:?}", s.diagnostics());
}

/// TS `blank-template` 第二场景：在空白模板上生成三级标题、正文与两种列表，
/// 保存后重解析，类型 / 级别 / 列表都对。
#[test]
#[cfg(feature = "compat-ts")]
fn generated_content_survives_a_save_and_reparse() {
    let mut s = EditSession::blank(None).expect("blank session");
    let dom = s.package().part(s.document().main_part).dom().expect("主 part");
    let mut at =
        dom.descendants(dom.root()).find(|&n| dom.is(n, QName::w(LocalName::P))).expect("空段");
    let ppr = |style: &str| {
        rsword::xml::NewElement::new(QName::w(LocalName::PPr)).with_child(
            rsword::xml::NewElement::new(QName::w(LocalName::PStyle))
                .with_attr(QName::w(LocalName::Val), style),
        )
    };
    let list_ppr = |num_id: &str| {
        let mut e = rsword::xml::NewElement::new(QName::w(LocalName::PPr));
        e.push_child(
            rsword::xml::NewElement::new(QName::w(LocalName::PStyle))
                .with_attr(QName::w(LocalName::Val), "ListParagraph"),
        );
        let mut num = rsword::xml::NewElement::new(QName::w(LocalName::NumPr));
        num.push_child(
            rsword::xml::NewElement::new(QName::w(LocalName::Ilvl))
                .with_attr(QName::w(LocalName::Val), "0"),
        );
        num.push_child(
            rsword::xml::NewElement::new(QName::w(LocalName::NumId))
                .with_attr(QName::w(LocalName::Val), num_id),
        );
        e.push_child(num);
        e
    };
    let blocks = [
        (Some(ppr("Heading1")), "生成的标题"),
        (None, "正文段落"),
        (Some(list_ppr(rsword::save::BLANK_BULLET_NUM_ID)), "无序项"),
        (Some(list_ppr(rsword::save::BLANK_ORDERED_NUM_ID)), "有序项"),
    ];
    for (props, text) in blocks {
        let r = s
            .apply(
                EditOp::InsertBlock {
                    at: BlockPos { part: None, at: BlockAt::After(at) },
                    block: NewBlock::Paragraph {
                        props,
                        inlines: vec![NewInline::Run(NewRun::text(text))],
                    },
                },
                &EditContext::default(),
            )
            .expect("插入");
        at = r.created.iter().flatten().copied().next().expect("新段");
    }
    let out = s.save().expect("save");
    let mut pkg = rsword::package::Package::open(&out).expect("reopen");
    let json = rsword::bind::compat_ts::parsed_doc(&mut pkg).expect("parsed_doc");
    let blocks = json["blocks"].as_array().expect("blocks");
    let visible: Vec<&serde_json::Value> =
        blocks.iter().filter(|b| b["hidden"] != serde_json::Value::Bool(true)).collect();
    let types: Vec<&str> = visible.iter().filter_map(|b| b["type"].as_str()).collect();
    assert_eq!(types, ["paragraph", "heading", "paragraph", "listItem", "listItem"]);
    assert_eq!(visible[1]["level"], 1);
    assert_eq!(visible[1]["runs"][0]["text"], "生成的标题");
    assert_eq!(visible[3]["list"]["kind"], "bullet");
    assert_eq!(visible[4]["list"]["kind"], "ordered");
}
