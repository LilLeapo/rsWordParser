//! `EDIT-03 InsertAtom`（`spec/18` 7.5）：五种原子，每种一条 XPath + 坐标流长度 1。

mod common;

use rsword::edit::{EditContext, EditOp, EditSession, InlinePos, NewAtom, NewMath, NewRun};
use rsword::model::inline::BreakKind;
use rsword::xml::NodeId;

const BODY: &str = "<w:p><w:r><w:t>前后</w:t></w:r></w:p>";

fn open() -> EditSession {
    EditSession::open(&common::docx_with_body(BODY)).expect("open")
}

fn para(s: &EditSession) -> NodeId {
    s.document().main.iter().find_map(|b| b.as_text().map(|t| t.node)).expect("段落")
}

/// 插一个原子：返回保存后的字节，并断言坐标流恰好长 1（`EDIT-02`）。
fn insert(atom: NewAtom) -> Vec<u8> {
    let mut s = open();
    let p = para(&s);
    let before = s.text_block(p).expect("段落").text().encode_utf16().count();
    s.apply(EditOp::InsertAtom { at: InlinePos::new(p, 1), atom }, &EditContext::default())
        .expect("插原子");
    let after = s.text_block(para(&s)).expect("段落").text().encode_utf16().count();
    assert_eq!(after, before + 1, "原子在坐标流里恰好占 1 个 UTF-16 单位");
    s.save().expect("保存")
}

#[test]
fn edit_02_break_atom() {
    let out = insert(NewAtom::Break { kind: BreakKind::Page, clear: None });
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [("count(//w:p/w:r/w:br)", ["1"]), ("//w:p/w:r/w:br/@w:type", ["page"]),]
    );
    let out = insert(NewAtom::Break { kind: BreakKind::TextWrapping, clear: Some("all".into()) });
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [("count(//w:br[@w:type])", ["0"]), ("//w:p/w:r/w:br/@w:clear", ["all"]),]
    );
}

#[test]
fn edit_02_symbol_atom() {
    let out = insert(NewAtom::Symbol { font: "Wingdings".into(), code: 0xF0FC });
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [("//w:p/w:r/w:sym/@w:font", ["Wingdings"]), ("//w:p/w:r/w:sym/@w:char", ["F0FC"]),]
    );
}

#[test]
fn edit_02_math_atom_from_omml_and_latex() {
    let out = insert(NewAtom::Math(NewMath::Omml(r#"<m:r><m:t>x</m:t></m:r>"#.into())));
    common::xpath_asserts!(&out, "word/document.xml", [("count(//w:p/m:oMath)", ["1"])]);
    let out = insert(NewAtom::Math(NewMath::Latex("a^2".into())));
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:p/m:oMath/m:sSup)", ["1"]),
            ("//w:p/m:oMath/m:sSup/m:sup/m:r/m:t/text()", ["2"]),
        ]
    );
}

/// 随文图片：run 里一个 `wp:inline`，不另起段落。
#[test]
fn edit_02_image_atom_is_inline() {
    let png = common::b64(common::PNG_1X1);
    let mut s = open();
    let p = para(&s);
    s.apply(
        EditOp::InsertAtom {
            at: InlinePos::new(p, 1),
            atom: NewAtom::Image(rsword::edit::NewImage {
                bytes: png,
                mime: "image/png".into(),
                extent_emu: (914400, 914400),
                align: None,
                wrap: None,
                pos_offset_emu: None,
                z_order: None,
                rot_deg: None,
                flip_h: false,
                flip_v: false,
                para_spacing: None,
            }),
        },
        &EditContext::default(),
    )
    .expect("插图片");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:body/w:p)", ["1"]),
            ("count(//w:p/w:r/w:drawing/wp:inline)", ["1"]),
            ("count(//w:p/w:r/w:drawing/wp:anchor)", ["0"]),
        ]
    );
}

/// 脚注引用：新建条目（`w:id` 按 `EDIT-06`）+ 引用 run；part 不存在按 `SAVE-05` 建。
#[test]
fn edit_02_note_ref_atom_creates_the_entry() {
    let mut s = open();
    let p = para(&s);
    s.apply(
        EditOp::InsertAtom {
            at: InlinePos::new(p, 1),
            atom: NewAtom::NoteRef {
                endnote: false,
                content: vec![vec![NewRun::text("脚注正文")]],
            },
        },
        &EditContext::default(),
    )
    .expect("插脚注");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:p/w:r/w:footnoteReference)", ["1"]),
            ("//w:p/w:r/w:footnoteReference/@w:id", ["1"]),
        ]
    );
    common::xpath_asserts!(
        &out,
        "word/footnotes.xml",
        [
            ("count(//w:footnotes/w:footnote[@w:id='1'])", ["1"]),
            ("//w:footnote[@w:id='1']//w:t/text()", ["脚注正文"]),
        ]
    );
    // 重解析：模型认得这条脚注
    let re = EditSession::open(&out).unwrap();
    assert!(re.document().footnotes.get("1").is_some(), "脚注在模型里");
}

/// 追踪时原子进 `w:ins`。
#[test]
fn tracked_atom_goes_into_ins() {
    let mut s = open();
    let p = para(&s);
    let ctx = EditContext {
        track_changes: Some(rsword::edit::RevisionAuthor {
            author: "甲".into(),
            date: Some("2026-01-01T00:00:00Z".into()),
        }),
        ..Default::default()
    };
    s.apply(
        EditOp::InsertAtom {
            at: InlinePos::new(p, 1),
            atom: NewAtom::Break { kind: BreakKind::Page, clear: None },
        },
        &ctx,
    )
    .expect("插分页符");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [("count(//w:p/w:ins/w:r/w:br)", ["1"]), ("//w:p/w:ins/@w:author", ["甲"]),]
    );
}

// ---- 跨段 `DeleteRange`（`spec/18` 7.5）--------------------------------------------------------

const THREE: &str = concat!(
    "<w:p><w:r><w:t>第一段</w:t></w:r></w:p>",
    "<w:p><w:r><w:t>第二段</w:t></w:r></w:p>",
    "<w:p><w:r><w:t>第三段</w:t></w:r></w:p>",
);

fn nth(s: &EditSession, i: usize) -> NodeId {
    s.document().main.iter().filter_map(|b| b.as_text().map(|t| t.node)).nth(i).expect("段落")
}

/// 三段文档从第 1 段中删到第 3 段中 → 剩一段。
#[test]
fn edit_03_cross_paragraph_delete() {
    let mut s = EditSession::open(&common::docx_with_body(THREE)).unwrap();
    let (a, c) = (nth(&s, 0), nth(&s, 2));
    s.apply(
        EditOp::DeleteRange { from: InlinePos::new(a, 1), to: InlinePos::new(c, 2) },
        &EditContext::default(),
    )
    .expect("跨段删除");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [("count(//w:body/w:p)", ["1"]), ("//w:p/w:r/w:t/text()", ["第", "段"]),]
    );
}

/// 书签端点按 `SPAN-06` 落到删除点。
#[test]
fn cross_paragraph_delete_collapses_bookmarks() {
    let body = concat!(
        r#"<w:p><w:r><w:t>第一段</w:t></w:r></w:p>"#,
        r#"<w:p><w:bookmarkStart w:id="1" w:name="mark"/><w:r><w:t>第二段</w:t></w:r>"#,
        r#"<w:bookmarkEnd w:id="1"/></w:p>"#,
        r#"<w:p><w:r><w:t>第三段</w:t></w:r></w:p>"#,
    );
    let mut s = EditSession::open(&common::docx_with_body(body)).unwrap();
    let (a, c) = (nth(&s, 0), nth(&s, 2));
    s.apply(
        EditOp::DeleteRange { from: InlinePos::new(a, 1), to: InlinePos::new(c, 2) },
        &EditContext::default(),
    )
    .expect("跨段删除");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:body/w:p)", ["1"]),
            // `SPAN-07`：两端都落进删除区的书签折叠留在删除点
            ("count(//w:bookmarkStart)", ["1"]),
            ("count(//w:bookmarkEnd)", ["1"]),
        ]
    );
}

/// 追踪版：三段仍在、中段带 `w:del`、首段的段落标记带 `w:del`。
#[test]
fn tracked_cross_paragraph_delete_keeps_everything() {
    let mut s = EditSession::open(&common::docx_with_body(THREE)).unwrap();
    let (a, c) = (nth(&s, 0), nth(&s, 2));
    let ctx = EditContext {
        track_changes: Some(rsword::edit::RevisionAuthor {
            author: "甲".into(),
            date: Some("2026-01-01T00:00:00Z".into()),
        }),
        ..Default::default()
    };
    s.apply(EditOp::DeleteRange { from: InlinePos::new(a, 1), to: InlinePos::new(c, 2) }, &ctx)
        .expect("追踪跨段删除");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [
            ("count(//w:body/w:p)", ["3"]),
            ("count(//w:p[2]/w:del/w:r/w:delText)", ["1"]),
            ("//w:p[2]/w:del/w:r/w:delText/text()", ["第二段"]),
            ("count(//w:p[1]/w:pPr/w:rPr/w:del)", ["1"]),
            ("count(//w:p[2]/w:pPr/w:rPr/w:del)", ["1"]),
        ]
    );
    // 接受之后与不追踪做一遍相同
    s.apply(EditOp::AcceptAll { author: None }, &EditContext::default()).expect("AcceptAll");
    let out = s.save().unwrap();
    common::xpath_asserts!(
        &out,
        "word/document.xml",
        [("count(//w:body/w:p)", ["1"]), ("//w:p/w:r/w:t/text()", ["第", "段"]),]
    );
}

/// 两端不在同一个内容容器里 → `EDIT_CROSS_CONTAINER`，状态不变。
#[test]
fn cross_container_delete_is_refused() {
    let body = concat!(
        r#"<w:p><w:r><w:t>正文段</w:t></w:r></w:p>"#,
        r#"<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/></w:tblPr>"#,
        r#"<w:tblGrid><w:gridCol w:w="4000"/></w:tblGrid>"#,
        r#"<w:tr><w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/></w:tcPr>"#,
        r#"<w:p><w:r><w:t>格里段</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
    );
    let bytes = common::docx_with_body(body);
    let mut s = EditSession::open(&bytes).unwrap();
    let outer = nth(&s, 0);
    let inner = s
        .document()
        .blocks()
        .filter_map(|b| b.as_text().map(|t| t.node))
        .find(|&n| n != outer)
        .expect("格里的段落");
    let err = s
        .apply(
            EditOp::DeleteRange { from: InlinePos::new(outer, 1), to: InlinePos::new(inner, 1) },
            &EditContext::default(),
        )
        .expect_err("跨容器");
    assert!(
        matches!(&err, rsword::Error::Edit { code, .. }
            if *code == rsword::DiagCode::EditCrossContainer),
        "{err}"
    );
    assert_eq!(s.save().unwrap(), bytes, "EDIT-05：被拒后一个字节都没动");
}

/// `SPAN-10` 的另一半：书签起点落在 `instrText` 内 → 保存后标记在 `fldChar begin` run 之前。
#[test]
fn span_10_endpoint_snaps_out_of_the_field_atom() {
    let body = concat!(
        r#"<w:p><w:r><w:t>前</w:t></w:r>"#,
        r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r>"#,
        r#"<w:bookmarkStart w:id="1" w:name="inside"/>"#,
        r#"<w:r><w:instrText xml:space="preserve"> PAGE </w:instrText></w:r>"#,
        r#"<w:r><w:fldChar w:fldCharType="separate"/></w:r>"#,
        r#"<w:r><w:t>1</w:t></w:r>"#,
        r#"<w:r><w:fldChar w:fldCharType="end"/></w:r>"#,
        r#"<w:bookmarkEnd w:id="1"/>"#,
        r#"<w:r><w:t>后</w:t></w:r></w:p>"#,
    );
    let mut s = EditSession::open(&common::docx_with_body(body)).unwrap();
    let p = nth(&s, 0);
    // 改这一段（让容器变脏，`SPAN-09` 才会物化标记）
    s.apply(
        EditOp::InsertText { at: InlinePos::new(p, 0), text: "X".into(), props: None },
        &EditContext::default(),
    )
    .expect("插字");
    let out = s.save().unwrap();
    common::xpath_asserts!(&out, "word/document.xml", [("count(//w:bookmarkStart)", ["1"])]);
    // 起点移到了字段原子**之前**：`w:bookmarkStart` 出现在第一个 `fldChar begin` 之前
    let xml = part_text(&out, "word/document.xml");
    let mark = xml.find("<w:bookmarkStart").expect("有 bookmarkStart");
    let begin = xml.find(r#"w:fldCharType="begin""#).expect("有 fldChar begin");
    assert!(mark < begin, "书签起点应在字段之前：\n{xml}");
}

/// zip 里一个 part 的文本。
fn part_text(docx: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(docx)).expect("zip");
    let mut f = zip.by_name(name).expect(name);
    let mut out = String::new();
    std::io::Read::read_to_string(&mut f, &mut out).expect("utf8");
    out
}
