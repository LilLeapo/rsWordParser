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
    let r =
        s.apply(EditOp::MergeWithNext { part: None, para: p }, &EditContext::default()).unwrap();
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
    s.apply(EditOp::MergeWithNext { part: None, para: p }, &EditContext::default()).unwrap();
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
        .apply(EditOp::MergeWithNext { part: None, para: p }, &EditContext::default())
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

// ---- 字段操作（`FLD-09`–`FLD-12`，任务 2.9）----------------------------------------------------

use rsword::edit::{LinkDest, LinkRef, NewField, NewInline, NewRun};
use rsword::semantic::props::{Change, RunPropsPatch};
use rsword::span::FieldId;
use rsword::span::field::{FormData, Keyword, read_form_data};

fn field_id(s: &EditSession, keyword: &Keyword) -> FieldId {
    s.document()
        .fields
        .fields()
        .iter()
        .find(|f| f.keyword() == keyword)
        .unwrap_or_else(|| panic!("没有 {keyword:?} 字段"))
        .id
}

/// `FLD-12` 验收行：插入 SEQ 字段后保存 → begin / instrText / separate / end 顺序与 `xml:space`。
#[test]
fn fld_12_insert_field_emits_the_five_runs_in_order() {
    let mut s = session(r#"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>ab</w:t></w:r></w:p>"#);
    let p = para(&s, 0);
    s.apply(
        EditOp::InsertField {
            at: InlinePos::new(p, 2),
            field: NewField {
                instr: "SEQ Figure \\* ARABIC".into(),
                result: vec![NewInline::Run(NewRun::text("1"))],
                mark_dirty: false,
            },
        },
        &EditContext::default(),
    )
    .unwrap();
    let xml = saved_xml(&mut s);
    let at = |needle: &str| xml.find(needle).unwrap_or_else(|| panic!("{needle} 不在: {xml}"));
    let begin = at(r#"w:fldCharType="begin""#);
    let instr = at("<w:instrText");
    let sep = at(r#"w:fldCharType="separate""#);
    let end = at(r#"w:fldCharType="end""#);
    assert!(begin < instr && instr < sep && sep < end, "五组 run 顺序: {xml}");
    assert!(
        xml.contains(r#"<w:instrText xml:space="preserve"> SEQ Figure \* ARABIC </w:instrText>"#),
        "指令前后各一个空格且带 preserve: {xml}"
    );
    // 结构 run 继承插入点的格式
    assert!(xml[begin - 60..begin].contains("<w:b/>"), "begin run 继承 rPr: {xml}");
    // 模型：字段进索引，坐标流里是一个原子
    let f = s.document().fields.fields();
    assert_eq!(f.len(), 1);
    assert_eq!(*f[0].keyword(), Keyword::Seq);
    assert!(f[0].is_atomic());
    assert_eq!(s.document().text_blocks().next().unwrap().text(), "ab\u{FFFC}");
}

/// `FLD-07 Link` 验收行：`SetLinkTarget` 只改 instrText，字段形态与开关都留着。
#[test]
fn fld_07_set_link_target_only_rewrites_the_instruction() {
    let mut s = session(concat!(
        r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r>"#,
        r#"<w:r><w:instrText xml:space="preserve"> HYPERLINK "http://old/" \o "tip" </w:instrText></w:r>"#,
        r#"<w:r><w:fldChar w:fldCharType="separate"/></w:r>"#,
        r#"<w:r><w:rPr><w:color w:val="0000FF"/></w:rPr><w:t>点这里</w:t></w:r>"#,
        r#"<w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#
    ));
    let id = field_id(&s, &Keyword::Hyperlink);
    s.apply(
        EditOp::SetLinkTarget {
            link: LinkRef::Field(id),
            target: LinkDest::Url("https://new.example/".into()),
        },
        &EditContext::default(),
    )
    .unwrap();
    let xml = saved_xml(&mut s);
    assert!(xml.contains(r#" HYPERLINK "https://new.example/" \o "tip" "#), "{xml}");
    assert!(!xml.contains("http://old/"), "{xml}");
    assert_eq!(xml.matches("<w:fldChar").count(), 3, "还是字段形态: {xml}");
    assert!(
        xml.contains("点这里") && xml.contains(r#"<w:color w:val="0000FF"/>"#),
        "结果不动: {xml}"
    );
    // 重开后目标已更新
    let re = EditSession::open(&s.save().unwrap()).unwrap();
    let f = &re.document().fields.fields()[0];
    assert_eq!(f.instr.first_argument(), Some("https://new.example/"));
    assert_eq!(f.instr.switch('o'), Some("tip"));
}

/// `FLD-10`：`ToggleCheckbox` 改 `w:checked`（不存在则插入），`w:default` 不动。
#[test]
fn fld_10_toggle_checkbox() {
    let ff = concat!(
        r#"<w:ffData><w:name w:val="Check1"/><w:enabled/><w:calcOnExit w:val="0"/>"#,
        r#"<w:checkBox><w:sizeAuto/><w:default w:val="0"/></w:checkBox></w:ffData>"#
    );
    let body = format!(
        concat!(
            r#"<w:p><w:r><w:fldChar w:fldCharType="begin">{ff}</w:fldChar></w:r>"#,
            r#"<w:r><w:instrText xml:space="preserve"> FORMCHECKBOX </w:instrText></w:r>"#,
            r#"<w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#
        ),
        ff = ff
    );
    let mut s = session(&body);
    let id = field_id(&s, &Keyword::FormCheckBox);
    let ctx = EditContext::default();
    s.apply(EditOp::ToggleCheckbox { field: id }, &ctx).unwrap();
    let xml = saved_xml(&mut s);
    assert!(xml.contains(r#"<w:checked w:val="1"/>"#), "插入 checked: {xml}");
    assert!(xml.contains(r#"<w:default w:val="0"/>"#), "default 不动: {xml}");
    // 再切一次回到未选中
    let mut re = EditSession::open(&s.save().unwrap()).unwrap();
    let id = field_id(&re, &Keyword::FormCheckBox);
    re.apply(EditOp::ToggleCheckbox { field: id }, &ctx).unwrap();
    let xml = saved_xml(&mut re);
    assert!(xml.contains(r#"<w:checked w:val="0"/>"#), "{xml}");
    let dom_bytes = re.save().unwrap();
    let mut pkg = Package::open(&dom_bytes).unwrap();
    let main = pkg.main_part();
    let dom = pkg.dom(main).unwrap().unwrap();
    let f = rsword::span::FieldIndex::build(dom);
    let ff = f.fields()[0].ff_data;
    assert!(matches!(read_form_data(dom, ff), Some(FormData::CheckBox { checked: false, .. })));
}

/// `FLD-10`：`SetFormText` 改 FORMTEXT 的结果文字，格式沿用原结果 run。
#[test]
fn fld_10_set_form_text() {
    let mut s = session(concat!(
        r#"<w:p><w:r><w:fldChar w:fldCharType="begin"><w:ffData><w:name w:val="T1"/>"#,
        r#"<w:textInput><w:default w:val="旧值"/></w:textInput></w:ffData></w:fldChar></w:r>"#,
        r#"<w:r><w:instrText xml:space="preserve"> FORMTEXT </w:instrText></w:r>"#,
        r#"<w:r><w:fldChar w:fldCharType="separate"/></w:r>"#,
        r#"<w:r><w:rPr><w:i/></w:rPr><w:t>旧值</w:t></w:r>"#,
        r#"<w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#
    ));
    let id = field_id(&s, &Keyword::FormText);
    s.apply(EditOp::SetFormText { field: id, text: "新值".into() }, &EditContext::default())
        .unwrap();
    let xml = saved_xml(&mut s);
    assert!(xml.contains("新值") && !xml.contains("<w:t>旧值</w:t>"), "{xml}");
    assert!(xml.contains("<w:i/>"), "结果格式保留: {xml}");
    assert!(xml.contains(r#"<w:default w:val="旧值"/>"#), "ffData 的 default 不动: {xml}");
}

/// `FLD-06/07` 验收行：REF 结果两个 run 改格式后只重生成结果 run，结构 run 原字节。
#[test]
fn fld_07_set_field_result_props_only_touches_the_result() {
    let mut s = session(concat!(
        r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r>"#,
        r#"<w:r><w:instrText xml:space="preserve"> REF bm \h </w:instrText></w:r>"#,
        r#"<w:r><w:fldChar w:fldCharType="separate"/></w:r>"#,
        r#"<w:r><w:rPr><w:b/></w:rPr><w:t>粗</w:t></w:r><w:r><w:t>普通</w:t></w:r>"#,
        r#"<w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#
    ));
    let id = field_id(&s, &Keyword::Ref);
    s.apply(
        EditOp::SetFieldResultProps {
            field: id,
            patch: RunPropsPatch { italic: Change::Set(true), ..Default::default() },
        },
        &EditContext::default(),
    )
    .unwrap();
    let xml = saved_xml(&mut s);
    assert_eq!(xml.matches("<w:i/>").count(), 2, "两个结果 run 都加了斜体: {xml}");
    assert!(xml.contains("<w:b/>"), "原有的加粗保留: {xml}");
    assert!(
        xml.contains(r#"<w:instrText xml:space="preserve"> REF bm \h </w:instrText>"#),
        "指令原字节: {xml}"
    );
    assert_eq!(xml.matches("<w:fldChar").count(), 3);
}

/// `FLD-09`：`UpdateBlockField` 换掉 `separate..end`；`w:fldLock` 的字段拒绝（`FLD_LOCKED`）。
#[test]
fn fld_09_update_block_field_and_lock() {
    let toc = |lock: &str| {
        format!(
            concat!(
                r#"<w:p><w:r><w:fldChar w:fldCharType="begin"{lock}/></w:r>"#,
                r#"<w:r><w:instrText xml:space="preserve"> TOC \o "1-3" \h </w:instrText></w:r>"#,
                r#"<w:r><w:fldChar w:fldCharType="separate"/></w:r>"#,
                r#"<w:r><w:t>旧目录</w:t></w:r></w:p>"#,
                r#"<w:p><w:r><w:t>旧目录第二行</w:t></w:r>"#,
                r#"<w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#
            ),
            lock = lock
        )
    };
    // 带 fldLock：拒绝
    let mut locked = session(&toc(r#" w:fldLock="true""#));
    let id = field_id(&locked, &Keyword::Toc);
    let err = locked
        .apply(EditOp::UpdateBlockField { field: id, blocks: Vec::new() }, &EditContext::default())
        .expect_err("fldLock 要拒绝");
    assert_eq!(code(&err), Some(DiagCode::FldLocked));

    // 不带：换掉结果区
    let mut s = session(&toc(""));
    let id = field_id(&s, &Keyword::Toc);
    let block = rsword::edit::NewBlock::Paragraph {
        props: None,
        inlines: vec![NewInline::Run(NewRun::text("新目录"))],
    };
    let ctx = EditContext::default().with_mark_updated_fields_dirty(true);
    s.apply(EditOp::UpdateBlockField { field: id, blocks: vec![block] }, &ctx).unwrap();
    let xml = saved_xml(&mut s);
    assert!(xml.contains("新目录"), "{xml}");
    assert!(!xml.contains("旧目录"), "旧结果全删: {xml}");
    assert!(
        xml.contains(r#"<w:instrText xml:space="preserve"> TOC \o "1-3" \h </w:instrText>"#),
        "指令原字节: {xml}"
    );
    assert!(xml.contains(r#"w:dirty="true""#), "mark_updated_fields_dirty: {xml}");
    assert_eq!(xml.matches("<w:fldChar").count(), 3, "结构 run 都在: {xml}");
}

/// `w:hyperlink` 元素的目标：外部 URL 按 `EDIT-06` 分配关系并写 `r:id`，改成书签时
/// 换成 `w:anchor` 并去掉 `r:id`（两者互斥）。
#[test]
fn fld_07_set_link_target_on_a_hyperlink_element() {
    let mut s = session(
        r#"<w:p><w:hyperlink w:anchor="bm"><w:r><w:t>去书签</w:t></w:r></w:hyperlink></w:p>"#,
    );
    let dom = s.dom();
    let link = dom
        .descendants(dom.root())
        .find(|&n| dom.is(n, rsword::xml::QName::w(rsword::xml::LocalName::Hyperlink)))
        .unwrap();
    let ctx = EditContext::default();
    s.apply(
        EditOp::SetLinkTarget {
            link: LinkRef::Element(link),
            target: LinkDest::Url("https://example.org/".into()),
        },
        &ctx,
    )
    .unwrap();
    let xml = saved_xml(&mut s);
    assert!(xml.contains("r:id="), "写了关系 id: {xml}");
    assert!(!xml.contains("w:anchor="), "互斥属性去掉: {xml}");
    let rels = {
        let bytes = s.save().unwrap();
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut f = z.by_name("word/_rels/document.xml.rels").unwrap();
        let mut t = String::new();
        std::io::Read::read_to_string(&mut f, &mut t).unwrap();
        t
    };
    assert!(
        rels.contains("https://example.org/") && rels.contains(r#"TargetMode="External""#),
        "{rels}"
    );
    // 再改回书签
    s.apply(
        EditOp::SetLinkTarget {
            link: LinkRef::Element(link),
            target: LinkDest::Anchor("other".into()),
        },
        &ctx,
    )
    .unwrap();
    let xml = saved_xml(&mut s);
    assert!(xml.contains(r#"w:anchor="other""#) && !xml.contains("r:id="), "{xml}");
}
