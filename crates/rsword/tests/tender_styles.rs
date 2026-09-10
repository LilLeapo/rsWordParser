//! Tender-derived formatting regression. Synthetic content, not a Word-authored fixture.
mod common;

use rsword::edit::{EditContext, EditOp, EditSession, InlinePos};
use rsword::semantic::props::Val;

const BODY: &str = include_str!("../../../fixtures/tender/guangzhou-it-2025/document-body.xml");
const STYLES: &str = include_str!("../../../fixtures/tender/guangzhou-it-2025/styles.xml");

fn fixture() -> Vec<u8> {
    common::docx_with_parts(BODY, &[("word/styles.xml", STYLES)])
}

#[test]
fn tender_raw_indent_is_length_and_missing_spacing_is_not_zero() {
    let bytes = fixture();
    let mut session = EditSession::open(&bytes).unwrap();
    let para =
        session.document().paragraphs().find(|p| p.text().starts_with("TENDER-BODY")).unwrap();
    let ind = para.props.indent.as_ref().unwrap();
    assert_eq!(ind.first_line, Some(Val::Value(480)));
    assert_eq!(ind.first_line_chars, None);
    assert!(para.props.spacing.is_none());
    assert_eq!(session.save().unwrap(), bytes);
}

#[test]
fn tender_merged_cells_are_read_from_table_flow() {
    let session = EditSession::open(&fixture()).unwrap();
    let tables: Vec<_> = session.document().tables().collect();
    assert_eq!(tables.len(), 1);
    let table = tables[0];
    assert_eq!(table.cell(0, 0).unwrap().grid_span(), 2);
    assert!(!table.cell(1, 0).unwrap().is_vmerge_continue());
    assert!(table.cell(2, 0).unwrap().is_vmerge_continue());
    assert!(table.cell(1, 1).unwrap().text_blocks().any(|p| p.text() == "TENDER-CELL"));
    assert!(session.document().paragraphs().any(|p| p.text() == "TENDER-CELL"));
}

#[test]
fn tender_public_insert_preserves_paragraph_and_section_properties() {
    let mut session = EditSession::open(&fixture()).unwrap();
    let para =
        session.document().paragraphs().find(|p| p.text().starts_with("TENDER-BODY")).unwrap();
    let node = para.node;
    let props = para.props.clone();
    let section = session.document().sections.clone();
    session
        .apply(
            EditOp::InsertText {
                at: InlinePos::new(node, 0),
                text: "RESPONSE ".into(),
                props: None,
            },
            &EditContext::default(),
        )
        .unwrap();
    let saved = session.save().unwrap();
    let reopened = EditSession::open(&saved).unwrap();
    let para = reopened
        .document()
        .paragraphs()
        .find(|p| p.text().starts_with("RESPONSE TENDER-BODY"))
        .unwrap();
    assert_eq!(para.props, props);
    assert_eq!(reopened.document().sections.len(), section.len());
    assert_eq!(reopened.document().sections[0].props, section[0].props);
    assert!(reopened.document().paragraphs().any(|p| p.text() == "TENDER-CELL"));
}
