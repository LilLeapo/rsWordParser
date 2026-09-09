//! `SetNoteContent` / `RemoveNote` / `SetSdtContent` / `RemoveSdtShell` / `SetMathTokens`
//! 与 `NewBlock::MathPara`（`EDIT-03`，`spec/18` 7.5）。
//!
//! 还有 TS `text-patch` 的两个场景：改批注 / 脚注里的文字，加粗与超链接原样留着——
//! 在本引擎里那就是 `InlinePos { part: 注释 part }` 上的 `InsertText` / `DeleteRange`。

mod common;

use rsword::edit::{EditContext, EditOp, EditSession, InlinePos, NewInline, NewRun};
use rsword::xml::{LocalName, NodeId, QName};

const FOOTNOTES: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
    r#"<w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>"#,
    r#"<w:footnote w:id="2"><w:p><w:r><w:footnoteRef/></w:r>"#,
    r#"<w:r><w:rPr><w:b/></w:rPr><w:t>粗体注</w:t></w:r>"#,
    r#"<w:hyperlink r:id="rIdX"><w:r><w:t>链接</w:t></w:r></w:hyperlink></w:p></w:footnote>"#,
    r#"</w:footnotes>"#,
);

fn doc_with_footnote() -> Vec<u8> {
    common::docx_with_parts(
        concat!(
            r#"<w:p><w:r><w:t>正文</w:t></w:r>"#,
            r#"<w:r><w:footnoteReference w:id="2"/></w:r>"#,
            r#"<w:r><w:t>尾巴</w:t></w:r></w:p>"#,
        ),
        &[("word/footnotes.xml", FOOTNOTES)],
    )
}

fn note_para(s: &EditSession) -> (rsword::package::PartId, NodeId) {
    let part = s.document().footnotes.part.expect("脚注 part");
    let note = s.document().footnotes.get("2").expect("脚注 2");
    (part, *note.paragraphs.first().expect("脚注段落"))
}

/// `SetNoteContent`：正文段落换掉，自引用标记 run 保住。
#[test]
fn edit_03_set_note_content() {
    let mut s = EditSession::open(&doc_with_footnote()).unwrap();
    s.apply(
        EditOp::SetNoteContent {
            endnote: false,
            id: "2".into(),
            content: vec![vec![NewRun::text("换过的脚注")]],
        },
        &EditContext::default(),
    )
    .expect("换内容");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/footnotes.xml",
        [
            ("count(//w:footnote[@w:id='2']//w:footnoteRef)", ["1"]),
            ("//w:footnote[@w:id='2']//w:t/text()", ["换过的脚注"]),
            ("count(//w:footnote[@w:type='separator'])", ["1"]),
        ]
    );
}

/// `RemoveNote`：条目与引用 run 都没了；引用 run 里只有引用 → 整 run 删。
#[test]
fn edit_03_remove_note() {
    let mut s = EditSession::open(&doc_with_footnote()).unwrap();
    s.apply(EditOp::RemoveNote { endnote: false, id: "2".into() }, &EditContext::default())
        .expect("删脚注");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [("count(//w:footnoteReference)", ["0"]), ("//w:p/w:r/w:t/text()", ["正文", "尾巴"]),]
    );
    common::xpath_asserts!(
        &out,
        "word/footnotes.xml",
        [
            ("count(//w:footnote[@w:id='2'])", ["0"]),
            ("count(//w:footnote[@w:type='separator'])", ["1"]),
        ]
    );
}

/// `DeleteRange` 覆盖引用原子 → 条目跟着走（`EDIT-03`）。
#[test]
fn edit_03_delete_range_over_note_ref_drops_the_entry() {
    let bytes = doc_with_footnote();
    let mut s = EditSession::open(&bytes).unwrap();
    let p = s.document().main.iter().find_map(|b| b.as_text().map(|t| t.node)).unwrap();
    // 正文 "正文" + 引用原子（1）+ "尾巴"：删 [2, 3) 正好盖住原子
    s.apply(
        EditOp::DeleteRange { from: InlinePos::new(p, 2), to: InlinePos::new(p, 3) },
        &EditContext::default(),
    )
    .expect("删引用");
    let out = s.save().unwrap();
    common::xpath_asserts!(&out, "word/document.xml", [("count(//w:footnoteReference)", ["0"])]);
    common::xpath_asserts!(&out, "word/footnotes.xml", [("count(//w:footnote[@w:id='2'])", ["0"])]);
}

/// TS `text-patch` 场景：改脚注里的文字，加粗 run 与超链接原样保留。
#[test]
fn text_patch_footnote_keeps_bold_and_hyperlink() {
    let mut s = EditSession::open(&doc_with_footnote()).unwrap();
    let (part, para) = note_para(&s);
    // 脚注坐标流："粗体注" + "链接"；在 "粗" 之后插字
    s.apply(
        EditOp::InsertText {
            at: InlinePos::in_part(part, para, 1), text: "×".into(), props: None
        },
        &EditContext::default(),
    )
    .expect("插字");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/footnotes.xml",
        [
            ("count(//w:footnote[@w:id='2']//w:rPr/w:b)", ["1"]),
            ("count(//w:footnote[@w:id='2']//w:hyperlink)", ["1"]),
            ("//w:footnote[@w:id='2']//w:hyperlink//w:t/text()", ["链接"]),
        ]
    );
    let re = EditSession::open(&out).unwrap();
    assert!(re.document().footnotes.get("2").expect("还在").text.contains('×'), "字插进去了");
}

/// TS `text-patch` 场景：改批注里的文字，加粗原样保留。
#[test]
fn text_patch_comment_keeps_bold() {
    let comments = concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
        r#"<w:comment w:id="1" w:author="甲"><w:p>"#,
        r#"<w:r><w:rPr><w:b/></w:rPr><w:t>批注文字</w:t></w:r></w:p></w:comment>"#,
        r#"</w:comments>"#,
    );
    let bytes = common::docx_with_parts(
        r#"<w:p><w:commentRangeStart w:id="1"/><w:r><w:t>正文</w:t></w:r><w:commentRangeEnd w:id="1"/><w:r><w:commentReference w:id="1"/></w:r></w:p>"#,
        &[("word/comments.xml", comments)],
    );
    let mut s = EditSession::open(&bytes).unwrap();
    let part = s.document().comments.part.expect("批注 part");
    let para = *s
        .document()
        .comments
        .items
        .iter()
        .find(|c| c.id == "1")
        .expect("批注 1")
        .blocks
        .first()
        .map(|b| b.node())
        .as_ref()
        .expect("批注段落");
    s.apply(
        EditOp::InsertText {
            at: InlinePos::in_part(part, para, 2),
            text: "新".into(),
            props: None,
        },
        &EditContext::default(),
    )
    .expect("插字");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/comments.xml",
        [("count(//w:comment[@w:id='1']//w:rPr/w:b)", ["1"]),]
    );
    let re = EditSession::open(&out).unwrap();
    let c = re.document().comments.items.iter().find(|c| c.id == "1").expect("批注还在");
    assert_eq!(c.text, "批注新文字");
}

// ---- 内容控件 -----------------------------------------------------------------------------------

fn sdt_doc(extra_pr: &str) -> Vec<u8> {
    common::docx_with_body(&format!(
        concat!(
            r#"<w:p><w:sdt><w:sdtPr><w:alias w:val="控件"/>{extra}</w:sdtPr>"#,
            r#"<w:sdtContent><w:r><w:rPr><w:i/></w:rPr><w:t>控件内容</w:t></w:r></w:sdtContent>"#,
            r#"</w:sdt><w:r><w:t>外面</w:t></w:r></w:p>"#
        ),
        extra = extra_pr
    ))
}

fn first_sdt(s: &EditSession) -> NodeId {
    let dom = s.package().part(s.document().main_part).dom().expect("主 part");
    dom.descendants(dom.root()).find(|&n| dom.is(n, QName::w(LocalName::Sdt))).expect("有 w:sdt")
}

#[test]
fn edit_03_set_sdt_content() {
    let mut s = EditSession::open(&sdt_doc("")).unwrap();
    let sdt = first_sdt(&s);
    s.apply(
        EditOp::SetSdtContent { sdt, inlines: vec![NewInline::Run(NewRun::text("新内容"))] },
        &EditContext::default(),
    )
    .expect("换内容");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("//w:sdtContent//w:t/text()", ["新内容"]),
            ("count(//w:sdt/w:sdtPr/w:alias)", ["1"]),
            ("//w:p/w:r/w:t/text()", ["外面"]),
        ]
    );
}

#[test]
fn edit_03_sdt_locked_and_bound_are_refused() {
    for (pr, want) in [
        (r#"<w:lock w:val="contentLocked"/>"#, rsword::DiagCode::EditSdtLocked),
        (
            r#"<w:dataBinding w:xpath="/root/a" w:storeItemID="{X}"/>"#,
            rsword::DiagCode::EditSdtBound,
        ),
    ] {
        let bytes = sdt_doc(pr);
        let mut s = EditSession::open(&bytes).unwrap();
        let sdt = first_sdt(&s);
        let err = s
            .apply(
                EditOp::SetSdtContent { sdt, inlines: vec![NewInline::Run(NewRun::text("x"))] },
                &EditContext::default(),
            )
            .expect_err("锁定 / 绑定的控件不给改");
        assert!(matches!(&err, rsword::Error::Edit { code, .. } if *code == want), "{pr}: {err}");
        assert_eq!(s.save().unwrap(), bytes, "EDIT-05：被拒后一个字节都没动");
    }
}

/// `RemoveSdtShell`：内容留下，`w:sdt` 消失（Word 的「删除内容控件」）。
#[test]
fn edit_03_remove_sdt_shell() {
    let mut s = EditSession::open(&sdt_doc("")).unwrap();
    let sdt = first_sdt(&s);
    s.apply(EditOp::RemoveSdtShell { sdt }, &EditContext::default()).expect("删壳");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:sdt)", ["0"]),
            ("//w:p/w:r/w:t/text()", ["控件内容", "外面"]),
            ("count(//w:p/w:r/w:rPr/w:i)", ["1"]),
        ]
    );
}

// ---- 公式 ---------------------------------------------------------------------------------------

const MATH_DOC: &str = concat!(
    r#"<w:p><m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">"#,
    r#"<m:r><m:t>a</m:t></m:r><m:r><m:t>+</m:t></m:r><m:r><m:t>b</m:t></m:r>"#,
    r#"</m:oMath></w:p>"#,
);

fn first_math(s: &EditSession) -> NodeId {
    let dom = s.package().part(s.document().main_part).dom().expect("主 part");
    dom.descendants(dom.root())
        .find(|&n| dom.is(n, QName::new(rsword::xml::NsId::M, LocalName::OMath)))
        .expect("有 m:oMath")
}

#[test]
fn edit_03_set_math_tokens() {
    let bytes = common::docx_with_body(MATH_DOC);
    let mut s = EditSession::open(&bytes).unwrap();
    let math = first_math(&s);
    s.apply(
        EditOp::SetMathTokens { math, tokens: vec!["x".into(), "-".into(), "y".into()] },
        &EditContext::default(),
    )
    .expect("改 token");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [("//m:oMath/m:r/m:t/text()", ["x", "-", "y"])]
    );

    // 个数不等 → `EDIT_MATH_TOKEN_COUNT`，状态不变
    let mut s = EditSession::open(&bytes).unwrap();
    let math = first_math(&s);
    let err = s
        .apply(EditOp::SetMathTokens { math, tokens: vec!["x".into()] }, &EditContext::default())
        .expect_err("个数不等");
    assert!(
        matches!(&err, rsword::Error::Edit { code, .. }
            if *code == rsword::DiagCode::EditMathTokenCount),
        "{err}"
    );
    assert_eq!(s.save().unwrap(), bytes, "EDIT-05：被拒后一个字节都没动");
}

/// `NewBlock::MathPara`：独立公式段（TS `mathParagraphXml` 的形态）。
#[test]
fn new_block_math_para() {
    let mut s =
        EditSession::open(&common::docx_with_body("<w:p><w:r><w:t>前</w:t></w:r></w:p>")).unwrap();
    let p = s.document().main.iter().find_map(|b| b.as_text().map(|t| t.node)).unwrap();
    s.apply(
        EditOp::InsertBlock {
            at: rsword::edit::BlockPos::after(p),
            block: rsword::edit::NewBlock::MathPara {
                omml: rsword::edit::NewMath::Latex("a^2".into()),
                align: "center".into(),
            },
        },
        &EditContext::default(),
    )
    .expect("插公式段");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:body/w:p)", ["2"]),
            ("count(//m:oMathPara/m:oMath/m:sSup)", ["1"]),
            ("//m:oMathPara/m:oMathParaPr/m:jc/@m:val", ["center"]),
        ]
    );
}
