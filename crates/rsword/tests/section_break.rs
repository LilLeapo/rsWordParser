//! 分节符的增删（`EDIT-03`，`spec/18` 7.6）。
//!
//! 形态对照件是 `fixtures/word-ops/{insert-next-page,delete-break}`：Word 自己做同一件事的
//! 前后两份（那里的 README 记着已复算的形态变化）。

mod common;

use common::fingerprint::{diff_str, fingerprint, fingerprint_main};
use rsword::edit::{EditContext, EditOp, EditSession};
use rsword::semantic::props::SectType;
use rsword::xml::NodeId;

fn nth_para(s: &EditSession, i: usize) -> NodeId {
    s.document().main.iter().filter_map(|b| b.as_text().map(|t| t.node)).nth(i).expect("段落")
}

const TWO_PARAS: &str = concat!(
    r#"<w:p><w:r><w:t>第一段</w:t></w:r></w:p>"#,
    r#"<w:p><w:r><w:t>第二段</w:t></w:r></w:p>"#,
    r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/>"#,
    r#"<w:pgMar w:top="1440" w:right="1800" w:bottom="1440" w:left="1800"/></w:sectPr>"#,
);

/// 单节文档在中段后插分节符 → 两节；第一节的 `sectPr` 是段落级、与 body 级 canon 相等。
#[test]
fn edit_03_insert_section_break() {
    let mut s = EditSession::open(&common::docx_with_body(TWO_PARAS)).unwrap();
    assert_eq!(s.document().sections.len(), 1);
    let p = nth_para(&s, 0);
    s.apply(
        EditOp::InsertSectionBreak { after: p, kind: SectType::NextPage },
        &EditContext::default(),
    )
    .expect("插分节符");
    assert_eq!(s.document().sections.len(), 2, "两节");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:sectPr)", ["2"]),
            ("count(//w:p[1]/w:pPr/w:sectPr)", ["1"]),
            // 缺省的 `nextPage` 不写 `w:type`（与真实 Word 一致）
            ("count(//w:sectPr/w:type)", ["0"]),
            // 页面设置克隆过去了
            ("//w:p[1]/w:pPr/w:sectPr/w:pgSz/@w:w", ["11906"]),
            ("//w:p[1]/w:pPr/w:sectPr/w:pgMar/@w:left", ["1800"]),
        ]
    );
}

/// 非缺省的断节方式写 `w:type`，而且写在**后**一节（原来的 body 级 `sectPr`）上。
#[test]
fn insert_section_break_writes_type_on_the_second_section() {
    let mut s = EditSession::open(&common::docx_with_body(TWO_PARAS)).unwrap();
    let p = nth_para(&s, 0);
    s.apply(
        EditOp::InsertSectionBreak { after: p, kind: SectType::Continuous },
        &EditContext::default(),
    )
    .expect("插连续分节符");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:p[1]/w:pPr/w:sectPr/w:type)", ["0"]),
            ("//w:body/w:sectPr/w:type/@w:val", ["continuous"]),
        ]
    );
}

/// 页眉引用跟着克隆：两节声明同一个 part，`RES-10` 的六槽有效值不变。
#[test]
fn insert_section_break_keeps_header_references() {
    let header = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
        r#"<w:p><w:r><w:t>页眉</w:t></w:r></w:p></w:hdr>"#,
    );
    let rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rIdH" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/>"#,
        r#"</Relationships>"#,
    );
    let body = concat!(
        r#"<w:p><w:r><w:t>第一段</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:t>第二段</w:t></w:r></w:p>"#,
        r#"<w:sectPr><w:headerReference xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" w:type="default" r:id="rIdH"/>"#,
        r#"<w:pgSz w:w="11906" w:h="16838"/></w:sectPr>"#,
    );
    let bytes = common::docx_with_parts(
        body,
        &[("word/header1.xml", header), ("word/_rels/document.xml.rels", rels)],
    );
    let mut s = EditSession::open(&bytes).unwrap();
    let p = nth_para(&s, 0);
    s.apply(
        EditOp::InsertSectionBreak { after: p, kind: SectType::NextPage },
        &EditContext::default(),
    )
    .expect("插分节符");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:sectPr/w:headerReference)", ["2"]),
            ("//w:sectPr/w:headerReference/@r:id", ["rIdH", "rIdH"]),
        ]
    );
    let re = EditSession::open(&out).unwrap();
    assert_eq!(re.document().sections.len(), 2);
}

/// 删掉刚插的分节符 → 指纹回到操作前。
#[test]
fn delete_section_break_round_trips() {
    let bytes = common::docx_with_body(TWO_PARAS);
    let before = fingerprint(&EditSession::open(&bytes).unwrap());
    let mut s = EditSession::open(&bytes).unwrap();
    let p = nth_para(&s, 0);
    s.apply(
        EditOp::InsertSectionBreak { after: p, kind: SectType::NextPage },
        &EditContext::default(),
    )
    .expect("插");
    let sect = s.document().sections[0].node.expect("段落级 sectPr");
    s.apply(EditOp::DeleteSectionBreak { sect }, &EditContext::default()).expect("删");
    assert_eq!(s.document().sections.len(), 1, "回到一节");
    let after = fingerprint(&s);
    if let Some((view, x, y)) = before.diff(&after) {
        panic!("插了又删没回到原样\n  视图 {view}\n  之前: {x}\n  之后: {y}");
    }
}

/// body 级的 `sectPr` 不能删。
#[test]
fn delete_body_level_section_is_refused() {
    let bytes = common::docx_with_body(TWO_PARAS);
    let mut s = EditSession::open(&bytes).unwrap();
    let sect = s.document().sections[0].node.expect("body 级 sectPr");
    let err = s
        .apply(EditOp::DeleteSectionBreak { sect }, &EditContext::default())
        .expect_err("body 级不能删");
    assert!(
        matches!(&err, rsword::Error::Edit { code, .. }
            if *code == rsword::DiagCode::EditBadPosition),
        "{err}"
    );
    assert_eq!(s.save().unwrap(), bytes, "EDIT-05：一个字节都没动");
}

/// 单元格里不能分节。
#[test]
fn insert_section_break_in_a_cell_is_refused() {
    let body = concat!(
        r#"<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/></w:tblPr>"#,
        r#"<w:tblGrid><w:gridCol w:w="4000"/></w:tblGrid>"#,
        r#"<w:tr><w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/></w:tcPr>"#,
        r#"<w:p><w:r><w:t>格里段</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        r#"<w:p><w:r><w:t>正文</w:t></w:r></w:p>"#,
        r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr>"#,
    );
    let bytes = common::docx_with_body(body);
    let mut s = EditSession::open(&bytes).unwrap();
    let cell_para = s
        .document()
        .blocks()
        .filter_map(|b| b.as_text().map(|t| t.node))
        .find(|&n| n != nth_para(&s, 0))
        .expect("格里的段落");
    let err = s
        .apply(
            EditOp::InsertSectionBreak { after: cell_para, kind: SectType::NextPage },
            &EditContext::default(),
        )
        .expect_err("单元格里不能分节");
    assert!(
        matches!(&err, rsword::Error::Edit { code, .. }
            if *code == rsword::DiagCode::EditBadPosition),
        "{err}"
    );
    assert_eq!(s.save().unwrap(), bytes, "EDIT-05：一个字节都没动");
}

/// 真实 Word 的对照件：删掉两节之间的分节符，留下的是**后**一节的页面设置。
#[test]
fn word_ops_delete_break_matches_word() {
    let dir = common::repo_root().join("fixtures/word-ops/delete-break");
    let before = std::fs::read(dir.join("before.docx")).expect("before.docx");
    let after = std::fs::read(dir.join("after.docx")).expect("after.docx");
    let mut s = EditSession::open(&before).unwrap();
    // 段落级的那个 `sectPr` 就是两节之间的分节符
    let sect = s
        .document()
        .sections
        .iter()
        .filter_map(|x| x.node)
        .find(|&n| {
            let dom = s.package().part(s.document().main_part).dom().expect("主 part");
            dom.parent(n)
                .is_some_and(|p| dom.is(p, rsword::xml::QName::w(rsword::xml::LocalName::PPr)))
        })
        .expect("段落级 sectPr");
    s.apply(EditOp::DeleteSectionBreak { sect }, &EditContext::default()).expect("删分节符");
    assert_eq!(s.document().sections.len(), 1, "剩一节");
    // 只比主 part：Word 另存时顺手补了 `footnotes.xml` / `endnotes.xml`（`before.docx` 里没有），
    // 那是它的保存行为，与"删分节符"这件事无关
    let want = fingerprint_main(&EditSession::open(&after).unwrap());
    let got = fingerprint_main(&s);
    if let Some((x, y)) = diff_str(&want.accept, &got.accept) {
        panic!("与 Word 的 after.docx 不同\n  Word: {x}\n  我们: {y}");
    }
}

// ---- 跨 part 的 `MoveBlock`（`XML-12` 规则 E′，`spec/18` 7.6）----------------------------------

/// 正文段落搬到页眉：两个 part 各自脏、前缀按页眉 part 重解析、其他 part 的字节不动、
/// 保存良构且 `SAVE-02` 没有前缀未绑定。
#[test]
fn xml_12_move_block_across_parts() {
    let header = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
        r#"<w:p><w:r><w:t>页眉原文</w:t></w:r></w:p></w:hdr>"#,
    );
    let rels = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rIdH" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/>"#,
        r#"</Relationships>"#,
    );
    let body = concat!(
        r#"<w:p><w:bookmarkStart w:id="1" w:name="搬走的"/><w:r><w:t>要搬的段</w:t></w:r>"#,
        r#"<w:bookmarkEnd w:id="1"/></w:p>"#,
        r#"<w:p><w:r><w:t>留下的段</w:t></w:r></w:p>"#,
        r#"<w:sectPr><w:headerReference xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" w:type="default" r:id="rIdH"/>"#,
        r#"<w:pgSz w:w="11906" w:h="16838"/></w:sectPr>"#,
    );
    let bytes = common::docx_with_parts(
        body,
        &[("word/header1.xml", header), ("word/_rels/document.xml.rels", rels)],
    );
    let mut s = EditSession::open(&bytes).unwrap();
    let hf = *s.document().hf_parts.keys().next().expect("页眉 part");
    let hdr_root = s.package().part(hf).dom().expect("页眉 DOM").root();
    let moved = nth_para(&s, 0);
    s.apply(
        EditOp::MoveBlock {
            from: None,
            node: moved,
            to: rsword::edit::BlockPos::in_part(hf, rsword::edit::BlockAt::End(hdr_root)),
        },
        &EditContext::default(),
    )
    .expect("跨 part 搬块");
    assert!(
        !s.diagnostics()
            .iter()
            .any(|d| d.origin == rsword::ValidationOrigin::EngineInvariantViolation),
        "{:?}",
        s.diagnostics()
    );
    let out = s.save().expect("保存");
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:body/w:p)", ["1"]),
            ("//w:body/w:p/w:r/w:t/text()", ["留下的段"]),
            // 搬走的段落连它的书签一起离开了主 part（`SPAN-07`）
            ("count(//w:bookmarkStart)", ["0"]),
        ]
    );
    common::xpath_asserts!(
        &out,
        "word/header1.xml",
        [
            ("count(//w:hdr/w:p)", ["2"]),
            ("//w:hdr/w:p/w:r/w:t/text()", ["页眉原文", "要搬的段"]),
            ("//w:hdr//w:bookmarkStart/@w:name", ["搬走的"]),
        ]
    );
    // 其他 part 一个字节都没动
    let orig = common::part_bytes(&bytes, "word/styles.xml");
    assert_eq!(orig, common::part_bytes(&out, "word/styles.xml"), "未编辑的 part 原字节");
    // 重解析：两个 part 都读得回来
    let re = EditSession::open(&out).unwrap();
    assert_eq!(re.document().main.iter().filter(|b| b.as_text().is_some()).count(), 1);
}
