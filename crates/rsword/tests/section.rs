//! 节模型与节视图验收（`MOD-09` / `MOD-10` / `RES-10`，任务 5.2）。

mod common;

use rsword::model::{Document, HfKind, HfVariant, Revision, SectionOwner};
use rsword::package::Package;
use rsword::resolve::Resolver;
use rsword::resolve::section::HfSlot;

const R: &str = r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;

fn doc_of(body: &str) -> (Package, Document) {
    let bytes = common::docx_with_body(body);
    let mut pkg = Package::open(&bytes).expect("open");
    let doc = rsword::model::Document::rebuild(&mut pkg).expect("rebuild");
    (pkg, doc)
}

fn doc_with_settings(body: &str, settings: &str) -> (Package, Document) {
    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:settings xmlns:w="{W}">{settings}</w:settings>"#
    );
    let bytes = common::docx_with_parts(body, &[("word/settings.xml", &xml)]);
    let mut pkg = Package::open(&bytes).expect("open");
    let doc = rsword::model::Document::rebuild(&mut pkg).expect("rebuild");
    (pkg, doc)
}

/// `RES-10` 验收行：第二节没有 header 引用时继承第一节的。三个变体各自继承。
#[test]
fn res_10_second_section_inherits_header_reference() {
    let body = format!(
        concat!(
            // 第一节：分节段落，声明 default 与 first 两个页眉、一个 default 页脚
            r#"<w:p><w:pPr><w:sectPr {r}>"#,
            r#"<w:headerReference w:type="default" r:id="rIdH1"/>"#,
            r#"<w:headerReference w:type="first" r:id="rIdH1f"/>"#,
            r#"<w:footerReference w:type="default" r:id="rIdF1"/>"#,
            r#"<w:pgSz w:w="11906" w:h="16838"/></w:sectPr></w:pPr><w:r><w:t>一</w:t></w:r></w:p>"#,
            // 第二节：只声明 default 页眉，页脚与 first 页眉靠继承
            r#"<w:p><w:r><w:t>二</w:t></w:r></w:p>"#,
            r#"<w:sectPr {r}><w:headerReference w:type="default" r:id="rIdH2"/></w:sectPr>"#
        ),
        r = R
    );
    let (_pkg, doc) = doc_of(&body);
    assert_eq!(doc.sections.len(), 2);
    assert!(matches!(doc.sections[0].owner, SectionOwner::Paragraph(_)));
    assert_eq!(doc.sections[1].owner, SectionOwner::Body);

    let r = Resolver::new(&doc);
    let s0 = r.section(&doc.sections, 0).expect("节 0");
    let s1 = r.section(&doc.sections, 1).expect("节 1");

    // 第一节：自己声明的
    assert_eq!(s0.slot(HfKind::Header, HfVariant::Default), &HfSlot::Declared("rIdH1".into()));
    assert_eq!(s0.slot(HfKind::Header, HfVariant::Even), &HfSlot::Absent);

    // 第二节：default 自己声明，first 与 footer 继承第一节，even 两节都没有
    assert_eq!(s1.slot(HfKind::Header, HfVariant::Default), &HfSlot::Declared("rIdH2".into()));
    assert_eq!(
        s1.slot(HfKind::Header, HfVariant::First),
        &HfSlot::Inherited { from: 0, id: "rIdH1f".into() }
    );
    assert_eq!(
        s1.slot(HfKind::Footer, HfVariant::Default),
        &HfSlot::Inherited { from: 0, id: "rIdF1".into() }
    );
    assert_eq!(s1.slot(HfKind::Footer, HfVariant::Even), &HfSlot::Absent);
    // 「是不是本节自己的 part」是 SetHeaderFooter 改写 / 新建的分界
    assert!(s1.slot(HfKind::Header, HfVariant::Default).is_declared());
    assert!(!s1.slot(HfKind::Header, HfVariant::First).is_declared());
}

/// `RES-10` 有效变体：`titlePg` / `evenAndOddHeaders` 定变体；选中的槽为空时**不回退** default。
#[test]
fn res_10_variant_for_page_has_no_fallback_to_default() {
    let body = format!(
        concat!(
            r#"<w:p><w:r><w:t>x</w:t></w:r></w:p>"#,
            r#"<w:sectPr {r}><w:headerReference w:type="default" r:id="rIdD"/>"#,
            r#"<w:headerReference w:type="even" r:id="rIdE"/><w:titlePg/></w:sectPr>"#
        ),
        r = R
    );
    let (_pkg, doc) = doc_with_settings(&body, "<w:evenAndOddHeaders/>");
    let r = Resolver::new(&doc);
    let s = r.section(&doc.sections, 0).expect("节");
    assert!(s.title_pg);
    assert!(s.even_and_odd);

    // 首页：titlePg 开着 → 用 first；没有 first 引用 → 首页没有页眉（不回退 default）
    assert_eq!(s.variant_for_page(true, false), HfVariant::First);
    assert_eq!(s.for_page(HfKind::Header, true, false), &HfSlot::Absent);
    // 偶数页：evenAndOddHeaders 开着 → 用 even
    assert_eq!(s.for_page(HfKind::Header, false, true).rel_id(), Some("rIdE"));
    // 其余页：default
    assert_eq!(s.for_page(HfKind::Header, false, false).rel_id(), Some("rIdD"));
    // 首页优先于偶数页
    assert_eq!(s.variant_for_page(true, true), HfVariant::First);

    // evenAndOddHeaders 关掉时偶数页也用 default
    let (_pkg2, doc2) = doc_of(&body);
    let r2 = Resolver::new(&doc2);
    let s2 = r2.section(&doc2.sections, 0).expect("节");
    assert!(!s2.even_and_odd);
    assert_eq!(s2.for_page(HfKind::Header, false, true).rel_id(), Some("rIdD"));
}

/// 一个 `w:sectPr` 都没有 → 一个隐式节覆盖全部块，几何全缺省（同 TS `DEFAULT_SECTION`）。
#[test]
fn mod_10_implicit_section_when_the_document_has_no_sect_pr() {
    let (_pkg, doc) = doc_of("<w:p><w:r><w:t>x</w:t></w:r></w:p><w:p/>");
    assert_eq!(doc.sections.len(), 1);
    let s = &doc.sections[0];
    assert_eq!(s.node, None);
    assert_eq!(s.owner, SectionOwner::Implicit);
    assert_eq!(s.block_range, 0..doc.main.len());
    assert_eq!(s.geom().page_width, rsword::model::section::DEFAULT_PAGE_WIDTH);
    assert_eq!(s.start_type(), rsword::semantic::props::SectType::NextPage);
    assert!(!s.title_pg());
    assert_eq!(s.hf_ref(HfKind::Header, HfVariant::Default), None);
    // 隐式节也能查到
    let r = Resolver::new(&doc);
    assert!(r.section(&doc.sections, 0).is_some());
    assert!(r.section(&doc.sections, 1).is_none());
}

/// `MOD-09`：`sectPr/sectPrChange` 的旧值快照挂在 `SectionInfo.revisions` 上。
#[test]
fn mod_09_sect_props_change_attaches_to_the_section() {
    let body = concat!(
        r#"<w:p/><w:sectPr><w:pgSz w:w="11906" w:h="16838"/>"#,
        r#"<w:sectPrChange w:id="7" w:author="甲" w:date="2026-09-05T00:00:00Z">"#,
        r#"<w:sectPr><w:pgSz w:w="12240" w:h="15840"/></w:sectPr></w:sectPrChange></w:sectPr>"#
    );
    let (_pkg, doc) = doc_of(body);
    let s = &doc.sections[0];
    assert_eq!(s.geom().page_width, 11906, "当前值");
    match s.revisions.as_slice() {
        [rsword::model::Revision::SectPropsChange { meta, old }] => {
            assert_eq!(meta.id.as_deref(), Some("7"));
            assert_eq!(meta.author.as_deref(), Some("甲"));
            assert_eq!(
                old.page_size.as_ref().and_then(|p| p.w.as_ref()),
                Some(&rsword::semantic::props::Val::Value(12240)),
                "旧值快照"
            );
        }
        other => panic!("期望一条 SectPropsChange，得到 {other:?}"),
    }
}

/// `section_of`：任意节点 → 管辖它的节。节以自己的 `sectPr` 结束，所以「在我之后的第一个」。
#[test]
fn res_10_section_of_maps_nodes_to_sections() {
    let body = concat!(
        r#"<w:p><w:pPr><w:sectPr><w:pgSz w:w="1" w:h="1"/></w:sectPr></w:pPr><w:r><w:t>一</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:t>二</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:t>三</w:t></w:r></w:p>"#,
        r#"<w:sectPr><w:pgSz w:w="2" w:h="2"/></w:sectPr>"#
    );
    let (mut pkg, doc) = doc_of(body);
    let dom = pkg.dom(doc.main_part).unwrap().unwrap();
    assert_eq!(doc.sections.len(), 2);
    assert_eq!(doc.sections[0].block_range, 0..1);
    assert_eq!(doc.sections[1].block_range, 1..doc.main.len());

    let nodes: Vec<_> = doc.main.iter().map(|b| b.node()).collect();
    assert_eq!(doc.section_of(dom, nodes[0]), Some(0), "分节段落自己属于它结束的那节");
    assert_eq!(doc.section_of(dom, nodes[1]), Some(1));
    assert_eq!(doc.section_of(dom, nodes[2]), Some(1));
    // 几何跟着节走
    assert_eq!(doc.sections[0].geom().page_width, 1);
    assert_eq!(doc.sections[1].geom().page_width, 2);
}

/// 全语料：节的枚举与 TS `readSections` 一致（份数与每节的块区间连续覆盖）。
///
/// TS 的规则是「`originalXml` 里含 `<w:sectPr` 的块结束一个节，一个都没有就给一个缺省节」；
/// 我们按块的 `w:sectPr`（body 级或分节段落的 `pPr/sectPr`）算，两者在全语料上逐份相同。
#[test]
fn mod_10_section_count_matches_ts_read_sections_on_corpus() {
    let mut docs = 0usize;
    let mut sections = 0usize;
    let mut multi = 0usize;
    for path in common::docx_paths("synthetic") {
        let expected = path.with_extension("").to_string_lossy().to_string() + ".expected.json";
        let Ok(txt) = std::fs::read_to_string(&expected) else { continue };
        let j: serde_json::Value = serde_json::from_str(&txt).expect("expected.json");
        let ts = j["blocks"]
            .as_array()
            .map(|bs| {
                bs.iter()
                    .filter(|b| b["originalXml"].as_str().is_some_and(|x| x.contains("<w:sectPr")))
                    .count()
            })
            .unwrap_or(0)
            .max(1);
        let bytes = std::fs::read(&path).unwrap();
        let Ok(mut pkg) = Package::open(&bytes) else { continue };
        let Ok(doc) = rsword::model::Document::rebuild(&mut pkg) else { continue };
        docs += 1;
        sections += doc.sections.len();
        if doc.sections.len() > 1 {
            multi += 1;
        }
        assert_eq!(doc.sections.len(), ts, "{}: 节数与 TS 不一致", path.display());
        // 块区间连续覆盖 0..main.len()
        let mut prev = 0usize;
        for s in &doc.sections {
            assert_eq!(s.block_range.start, prev, "{}: 区间不连续", path.display());
            assert!(s.block_range.end >= s.block_range.start);
            prev = s.block_range.end;
        }
        assert_eq!(prev, doc.main.len(), "{}: 区间没覆盖到末尾", path.display());
    }
    eprintln!("section: {docs} 份文档、{sections} 个节、{multi} 份多节");
    assert!(docs > 500, "语料缺失？{docs}");
}
