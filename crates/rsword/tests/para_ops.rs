//! `EDIT-03` 段落与字段操作（任务 2.9）：`SplitParagraph` / `MergeWithNext` 与 `SPAN-06`
//! 的拆分 / 合并两行规则。

mod common;

use rsword::diag::DiagCode;
use rsword::edit::{EditContext, EditOp, EditSession, InlinePos};
use rsword::error::Error;
use rsword::package::Package;
use rsword::span::RangeClass;
use rsword::xml::NodeId;

fn session(body: &str) -> EditSession {
    EditSession::open(&common::docx_with_body(body)).unwrap()
}

fn para(s: &EditSession, i: usize) -> NodeId {
    s.document().text_blocks().nth(i).unwrap().node
}

fn texts(s: &EditSession) -> Vec<String> {
    s.document().text_blocks().map(|b| b.text()).collect()
}

fn saved_xml(s: &mut EditSession) -> String {
    let bytes = s.save().unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    let main = pkg.main_part();
    pkg.dom(main).unwrap().unwrap().src().to_string()
}

fn code(e: &Error) -> Option<DiagCode> {
    match e {
        Error::Edit { code, .. } => Some(*code),
        _ => None,
    }
}

/// 段中间拆：先拆 run，后半搬进新段；`pPr` 字节克隆到新段（Word 语义）。
#[test]
fn edit_03_split_paragraph_in_the_middle_of_a_run() {
    let mut s = session(
        r#"<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>abcd</w:t></w:r></w:p>"#,
    );
    let p = para(&s, 0);
    let r = s
        .apply(EditOp::SplitParagraph { at: InlinePos::new(p, 2) }, &EditContext::default())
        .unwrap();
    assert!(r.structure_changed);
    assert_eq!(texts(&s), ["ab", "cd"]);
    let xml = saved_xml(&mut s);
    assert_eq!(xml.matches("<w:jc w:val=\"center\"/>").count(), 2, "pPr 克隆到新段: {xml}");
    assert_eq!(xml.matches("<w:b/>").count(), 2, "拆出的 run 保留格式: {xml}");
    assert!(xml.find("ab").unwrap() < xml.find("cd").unwrap());
}

/// 边界处拆：内容项整项搬走，不产生空 run。
#[test]
fn edit_03_split_paragraph_at_a_run_boundary() {
    let mut s = session(
        r#"<w:p><w:r><w:t>one</w:t></w:r><w:r><w:t>two</w:t></w:r></w:p><w:p><w:r><w:t>tail</w:t></w:r></w:p>"#,
    );
    let p = para(&s, 0);
    s.apply(EditOp::SplitParagraph { at: InlinePos::new(p, 3) }, &EditContext::default()).unwrap();
    assert_eq!(texts(&s), ["one", "two", "tail"]);
    let xml = saved_xml(&mut s);
    assert_eq!(xml.matches("<w:p>").count() + xml.matches("<w:p/>").count(), 3, "{xml}");
    assert!(!xml.contains("<w:t></w:t>") && !xml.contains("<w:t/>"), "没有空 w:t: {xml}");
}

/// `SPAN-06` 拆分行：书签起点在前半留下，终点跟到后段；空书签按 affinity 整体跟到后段。
#[test]
fn span_06_split_moves_anchors_to_the_tail_paragraph() {
    let mut s = session(
        r#"<w:p><w:bookmarkStart w:id="1" w:name="bm"/><w:r><w:t>ab</w:t></w:r>
           <w:r><w:t>cd</w:t></w:r><w:bookmarkEnd w:id="1"/></w:p>"#,
    );
    let p = para(&s, 0);
    s.apply(EditOp::SplitParagraph { at: InlinePos::new(p, 2) }, &EditContext::default()).unwrap();
    let tail = para(&s, 1);
    let idx = s.spans().unwrap();
    let bm = idx.find(RangeClass::Bookmark, "1").expect("书签还在");
    let (start, end) = (bm.start.unwrap(), bm.end.unwrap());
    assert_eq!((start.container, start.index), (p, 0), "起点留在前段");
    assert_eq!((end.container, end.index), (tail, 1), "终点跟到后段末尾");
    // 保存后标记物理上也在对应段里
    let xml = saved_xml(&mut s);
    let (a, b) = (xml.find("bookmarkStart").unwrap(), xml.find("bookmarkEnd").unwrap());
    let mid = xml.find("cd").unwrap();
    assert!(a < mid && mid < b, "起点在前、终点在后: {xml}");
}

/// 透明字段（HYPERLINK）横跨拆分点 → `Err(EDIT_SPLIT_FIELD)`，什么都不改。
#[test]
fn edit_03_split_across_a_transparent_field_is_refused() {
    let mut s = session(concat!(
        r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r>"#,
        r#"<w:r><w:instrText xml:space="preserve"> HYPERLINK "http://x/" </w:instrText></w:r>"#,
        r#"<w:r><w:fldChar w:fldCharType="separate"/></w:r>"#,
        r#"<w:r><w:t>ab</w:t></w:r><w:r><w:t>cd</w:t></w:r>"#,
        r#"<w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#
    ));
    let p = para(&s, 0);
    let before = texts(&s);
    let err = s
        .apply(EditOp::SplitParagraph { at: InlinePos::new(p, 2) }, &EditContext::default())
        .expect_err("透明字段不能跨段");
    assert_eq!(code(&err), Some(DiagCode::EditSplitField));
    assert_eq!(texts(&s), before, "失败不留半改状态");
    assert!(!s.package().is_dirty());
}

/// 合并：下一段内容接到本段末尾，下一段消失，保留**前**段的 `pPr`。
#[test]
fn edit_03_merge_with_next_keeps_the_first_paragraph_props() {
    let mut s = session(
        r#"<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:t>ab</w:t></w:r></w:p>
           <w:p><w:pPr><w:jc w:val="right"/></w:pPr><w:r><w:t>cd</w:t></w:r></w:p>"#,
    );
    let p = para(&s, 0);
    let r = s.apply(EditOp::MergeWithNext { para: p }, &EditContext::default()).unwrap();
    assert!(r.structure_changed);
    assert_eq!(texts(&s), ["abcd"]);
    let xml = saved_xml(&mut s);
    assert!(xml.contains(r#"<w:jc w:val="center"/>"#), "{xml}");
    assert!(!xml.contains(r#"<w:jc w:val="right"/>"#), "后段的 pPr 丢弃: {xml}");
}

/// `SPAN-06` 合并行：后段里的锚点整体搬到前段，下标加上前段原有内容项数。
#[test]
fn span_06_merge_relocates_anchors_from_the_second_paragraph() {
    let mut s = session(
        r#"<w:p><w:r><w:t>ab</w:t></w:r><w:r><w:t>cd</w:t></w:r></w:p>
           <w:p><w:bookmarkStart w:id="1" w:name="bm"/><w:r><w:t>ef</w:t></w:r><w:bookmarkEnd w:id="1"/></w:p>"#,
    );
    let p = para(&s, 0);
    s.apply(EditOp::MergeWithNext { para: p }, &EditContext::default()).unwrap();
    assert_eq!(texts(&s), ["abcdef"]);
    let idx = s.spans().unwrap();
    let bm = idx.find(RangeClass::Bookmark, "1").expect("书签还在");
    let (start, end) = (bm.start.unwrap(), bm.end.unwrap());
    assert_eq!((start.container, start.index), (p, 2), "0 + 前段的 2 个内容项");
    assert_eq!((end.container, end.index), (p, 3));
    let xml = saved_xml(&mut s);
    assert_eq!(xml.matches("<w:p>").count(), 1, "只剩一段: {xml}");
    let (a, b) = (xml.find("bookmarkStart").unwrap(), xml.find("bookmarkEnd").unwrap());
    assert!(
        xml.find("cd").unwrap() < a && a < xml.find("ef").unwrap() && b > xml.find("ef").unwrap()
    );
}

/// 合并的边界情形：最后一段没有下一段 → `Err`；下一个块不是段落 → `Err`。
#[test]
fn edit_03_merge_with_next_needs_a_following_paragraph() {
    let mut s = session(
        r#"<w:p><w:r><w:t>ab</w:t></w:r></w:p>
           <w:tbl><w:tr><w:tc><w:p><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
    );
    let p = para(&s, 0);
    let err = s
        .apply(EditOp::MergeWithNext { para: p }, &EditContext::default())
        .expect_err("下一个块是表格");
    assert_eq!(code(&err), Some(DiagCode::EditBadPosition));
}

// ---- 书签（`EDIT-03` AddBookmark / RemoveBookmark，任务 2.9）-----------------------------------

/// `EDIT-06`：书签 `w:id` 取全 part 最大值 + 1；名字重复要拒。
#[test]
fn edit_06_add_bookmark_allocates_ids_and_refuses_duplicate_names() {
    let mut s = session(
        r#"<w:p><w:bookmarkStart w:id="7" w:name="old"/><w:r><w:t>abcd</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p>"#,
    );
    let p = para(&s, 0);
    let ctx = EditContext::default();
    s.apply(
        EditOp::AddBookmark {
            name: "fresh".into(),
            from: InlinePos::new(p, 1),
            to: InlinePos::new(p, 3),
        },
        &ctx,
    )
    .unwrap();
    let idx = s.spans().unwrap();
    let bm = idx.find(RangeClass::Bookmark, "8").expect("id 取 7 + 1");
    assert_eq!(bm.kind.bookmark_name(), Some("fresh"));
    assert!(bm.is_paired() && !bm.is_collapsed());
    // 名字重复
    let err = s
        .apply(
            EditOp::AddBookmark {
                name: "fresh".into(),
                from: InlinePos::new(p, 0),
                to: InlinePos::new(p, 1),
            },
            &ctx,
        )
        .expect_err("名字重复");
    assert_eq!(code(&err), Some(DiagCode::EditBadPosition));
    // 保存：标记落在正确位置，原书签不动
    let xml = saved_xml(&mut s);
    assert!(xml.contains(r#"<w:bookmarkStart w:id="8" w:name="fresh"/>"#), "{xml}");
    assert!(xml.contains(r#"<w:bookmarkStart w:id="7" w:name="old"/>"#), "原书签原字节: {xml}");
    let (s8, e8) = (
        xml.find(r#"w:id="8" w:name="fresh""#).unwrap(),
        xml.rfind(r#"<w:bookmarkEnd w:id="8"/>"#).unwrap(),
    );
    assert!(s8 < xml.find(">bc<").unwrap_or(s8 + 1), "起点在 bc 之前: {xml}");
    assert!(s8 < e8);
    // 重开后仍是完整的一对
    let mut re = EditSession::open(&s.save().unwrap()).unwrap();
    let bm = re.spans().unwrap().find(RangeClass::Bookmark, "8").expect("重开还在");
    assert!(bm.is_paired());
}

/// 空区间的书签：两端同位置，`SPAN-02` 例外让它们同向。
#[test]
fn edit_03_add_collapsed_bookmark() {
    let mut s = session(r#"<w:p><w:r><w:t>ab</w:t></w:r></w:p>"#);
    let p = para(&s, 0);
    s.apply(
        EditOp::AddBookmark {
            name: "here".into(),
            from: InlinePos::new(p, 2),
            to: InlinePos::new(p, 2),
        },
        &EditContext::default(),
    )
    .unwrap();
    let idx = s.spans().unwrap();
    let bm = idx.find(RangeClass::Bookmark, "1").unwrap();
    assert!(bm.is_collapsed());
    assert_eq!(bm.start.unwrap().affinity, bm.end.unwrap().affinity);
    let xml = saved_xml(&mut s);
    let (a, b) = (xml.find("bookmarkStart").unwrap(), xml.find("bookmarkEnd").unwrap());
    assert!(a < b, "起点在终点之前（不是反序的一对）: {xml}");
}

/// `RemoveBookmark`：标记删掉、索引作废，正文文字不动；没有该名字要拒。
#[test]
fn edit_03_remove_bookmark() {
    let mut s = session(
        r#"<w:p><w:bookmarkStart w:id="1" w:name="one"/><w:r><w:t>ab</w:t></w:r><w:bookmarkEnd w:id="1"/>
           <w:bookmarkStart w:id="2" w:name="two"/><w:r><w:t>cd</w:t></w:r><w:bookmarkEnd w:id="2"/></w:p>"#,
    );
    let ctx = EditContext::default();
    s.apply(EditOp::RemoveBookmark { name: "one".into() }, &ctx).unwrap();
    let idx = s.spans().unwrap();
    assert!(idx.find(RangeClass::Bookmark, "1").is_none());
    assert!(idx.find(RangeClass::Bookmark, "2").is_some());
    let err =
        s.apply(EditOp::RemoveBookmark { name: "nope".into() }, &ctx).expect_err("没有这个书签");
    assert_eq!(code(&err), Some(DiagCode::EditBadPosition));
    let xml = saved_xml(&mut s);
    assert!(!xml.contains(r#"w:name="one""#), "{xml}");
    assert!(xml.contains(r#"w:name="two""#), "另一个不动: {xml}");
    assert!(xml.contains("ab") && xml.contains("cd"), "正文不动: {xml}");
}
