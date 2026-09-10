//! handoff §5.5 的变形关系。
//!
//! 变形只比较规范语义（文本、直接 bold/italic、段落 jc），不要求物理 XML 或字节恢复。

mod common;

use rsword::edit::{BlockAt, BlockPos, EditContext, EditOp, EditSession, InlinePos, Utf16Offset};
use rsword::model::{Inline, SegmentKind};
use rsword::semantic::props::{Change, Jc, ParaPropsPatch, RunPropsPatch, Val};

#[derive(Clone, Debug, PartialEq, Eq)]
struct SemanticPara {
    text: String,
    style: Vec<(Option<bool>, Option<bool>)>,
    jc: Option<Val<Jc>>,
}

fn semantic(doc: &EditSession) -> Vec<SemanticPara> {
    doc.document()
        .text_blocks()
        .map(|block| {
            let mut style = Vec::new();
            for inline in &block.inlines {
                if let Inline::Run(run) = inline {
                    for seg in &run.segments {
                        match &seg.kind {
                            SegmentKind::Text | SegmentKind::DelText => {
                                for _ in run.segment_text(seg).chars() {
                                    style.push((run.props.bold, run.props.italic));
                                }
                            }
                            SegmentKind::Ink
                            | SegmentKind::FootnoteRefMark
                            | SegmentKind::EndnoteRefMark => {}
                            _ if seg.utf16_len == 0 => {}
                            _ => style.push((run.props.bold, run.props.italic)),
                        }
                    }
                } else if matches!(inline, Inline::Field { .. } | Inline::Atom(_)) {
                    style.push((None, None));
                }
            }
            SemanticPara { text: block.text(), style, jc: block.props.jc.clone() }
        })
        .collect()
}

fn para_by_text(s: &EditSession, text: &str) -> rsword::xml::NodeId {
    s.document()
        .text_blocks()
        .find(|b| b.text() == text)
        .unwrap_or_else(|| panic!("paragraph {text:?} not found"))
        .node
}

fn pos(para: rsword::xml::NodeId, offset: u32) -> InlinePos {
    InlinePos { part: None, para, offset: Utf16Offset(offset) }
}

fn insert(s: &mut EditSession, text: &str, at: u32, inserted: &str, bold: bool) {
    let para = para_by_text(s, text);
    s.apply(
        EditOp::InsertText {
            at: pos(para, at),
            text: inserted.into(),
            props: Some(RunPropsPatch {
                bold: Change::Set(bold),
                italic: Change::Set(false),
                ..Default::default()
            }),
        },
        &EditContext::default(),
    )
    .unwrap();
}

fn body() -> &'static str {
    concat!(
        r#"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>A</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:rPr><w:i/></w:rPr><w:t>B</w:t></w:r></w:p>"#
    )
}

#[test]
fn edit_03_insert_then_delete_inserted_range_restores_semantics() {
    let mut s = EditSession::open(&common::docx_with_body(body())).unwrap();
    let before = semantic(&s);
    insert(&mut s, "A", 1, "😀X", true);
    let mid = semantic(&s);
    assert_ne!(before, mid);
    let para = para_by_text(&s, "A😀X");
    s.apply(EditOp::DeleteRange { from: pos(para, 1), to: pos(para, 4) }, &EditContext::default())
        .unwrap();
    assert_eq!(semantic(&s), before, "插后删的语义必须恢复");
    let reopened = EditSession::open(&s.save().unwrap()).unwrap();
    assert_eq!(semantic(&reopened), before);
}

#[test]
fn edit_03_repeated_property_set_is_idempotent() {
    let mut s = EditSession::open(&common::docx_with_body(body())).unwrap();
    let para = para_by_text(&s, "A");
    let patch =
        RunPropsPatch { bold: Change::Set(false), italic: Change::Set(true), ..Default::default() };
    s.apply(
        EditOp::SetRunProps { from: pos(para, 0), to: pos(para, 1), patch: patch.clone() },
        &EditContext::default(),
    )
    .unwrap();
    let first = s.save().unwrap();
    let after_first = semantic(&s);
    s.apply(
        EditOp::SetRunProps { from: pos(para, 0), to: pos(para, 1), patch },
        &EditContext::default(),
    )
    .unwrap();
    assert_eq!(semantic(&s), after_first);
    assert_eq!(s.save().unwrap(), first, "相同属性重复设置不应改写字节");
}

#[test]
fn test_07_save_reopen_between_ops_matches_uninterrupted_sequence() {
    let mut direct = EditSession::open(&common::docx_with_body(body())).unwrap();
    insert(&mut direct, "A", 1, "1", true);
    let p = para_by_text(&direct, "A1");
    direct
        .apply(
            EditOp::SetParaProps {
                part: None,
                para: p,
                patch: ParaPropsPatch {
                    jc: Change::Set(Val::Value(Jc::Center)),
                    ..Default::default()
                },
            },
            &EditContext::default(),
        )
        .unwrap();

    let mut reopened = EditSession::open(&common::docx_with_body(body())).unwrap();
    insert(&mut reopened, "A", 1, "1", true);
    reopened = EditSession::open(&reopened.save().unwrap()).unwrap();
    let p = para_by_text(&reopened, "A1");
    reopened
        .apply(
            EditOp::SetParaProps {
                part: None,
                para: p,
                patch: ParaPropsPatch {
                    jc: Change::Set(Val::Value(Jc::Center)),
                    ..Default::default()
                },
            },
            &EditContext::default(),
        )
        .unwrap();
    assert_eq!(semantic(&direct), semantic(&reopened));
}

#[test]
fn span_06_edit_independent_paragraphs_then_swap_matches_swap_then_edit() {
    let mut edit_then_swap = EditSession::open(&common::docx_with_body(body())).unwrap();
    insert(&mut edit_then_swap, "A", 1, "1", true);
    insert(&mut edit_then_swap, "B", 1, "2", false);
    let a = para_by_text(&edit_then_swap, "A1");
    let b = para_by_text(&edit_then_swap, "B2");
    edit_then_swap
        .apply(
            EditOp::MoveBlock { from: None, node: a, to: BlockPos::main(BlockAt::After(b)) },
            &EditContext::default(),
        )
        .unwrap();

    let mut swap_then_edit = EditSession::open(&common::docx_with_body(body())).unwrap();
    let a = para_by_text(&swap_then_edit, "A");
    let b = para_by_text(&swap_then_edit, "B");
    swap_then_edit
        .apply(
            EditOp::MoveBlock { from: None, node: a, to: BlockPos::main(BlockAt::After(b)) },
            &EditContext::default(),
        )
        .unwrap();
    insert(&mut swap_then_edit, "A", 1, "1", true);
    insert(&mut swap_then_edit, "B", 1, "2", false);
    assert_eq!(semantic(&edit_then_swap), semantic(&swap_then_edit));
}

#[test]
fn save_06_unrelated_opaque_part_is_preserved_and_body_semantics_unchanged() {
    let opaque = concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        r#"<opaque xmlns="urn:test"><payload>原样保留 &amp; 重复</payload><payload>原样保留 &amp; 重复</payload></opaque>"#
    );
    let with_part = common::docx_with_parts(body(), &[("customXml/item9.xml", opaque)]);
    let baseline = common::docx_with_body(body());
    let mut with_part_session = EditSession::open(&with_part).unwrap();
    let mut baseline_session = EditSession::open(&baseline).unwrap();
    insert(&mut with_part_session, "A", 1, "😀", true);
    insert(&mut baseline_session, "A", 1, "😀", true);
    assert_eq!(semantic(&with_part_session), semantic(&baseline_session));
    let saved = with_part_session.save().unwrap();
    assert_eq!(common::part_bytes(&saved, "customXml/item9.xml"), opaque.as_bytes());
}
