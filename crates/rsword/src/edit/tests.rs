use std::io::{Cursor, Write};

use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use super::*;
use crate::diag::{DiagCode, ValidationOrigin};
use crate::package::Package;
use crate::xml::plan::{NodeEdit, Target};
use crate::xml::{Dirty, LocalName, NsId, QName};

pub(super) fn minimal_docx(body_xml: &str) -> Vec<u8> {
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
</Types>";
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
</Relationships>";
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body_xml}</w:body></w:document>"
    );

    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", rels.as_bytes()),
        ("word/document.xml", document.as_bytes()),
    ] {
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        writer.start_file(name, opts).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn first_t(session: &EditSession) -> (crate::package::PartId, crate::xml::NodeId) {
    let main = session.package().main_part();
    let dom = session.package().part(main).dom().unwrap();
    let t = QName::w(LocalName::T);
    let node = dom.descendants(dom.root()).find(|&id| dom.is(id, t)).unwrap();
    (main, node)
}

fn first_paragraph(session: &EditSession) -> crate::xml::NodeId {
    session.document().text_blocks().next().unwrap().node
}

#[test]
fn edit_01_open_save_no_edit_is_byte_identical() {
    let bytes = minimal_docx("<w:p><w:r><w:t>hello</w:t></w:r></w:p>");
    let mut session = EditSession::open(&bytes).unwrap();
    assert_eq!(session.document().main_part, session.package().main_part());
    assert_eq!(session.document().text_blocks().count(), 1);
    assert!(!session.package().is_dirty());
    assert_eq!(session.save(SaveOptions::default()).unwrap(), bytes);
}

#[test]
fn edit_02_utf16_offsets_and_surrogate_boundaries() {
    let bytes = minimal_docx("<w:p><w:r><w:t xml:space=\"preserve\">A😀B</w:t></w:r></w:p>");
    let session = EditSession::open(&bytes).unwrap();
    let block = session.document().text_blocks().next().unwrap();
    assert_eq!(block.utf16_len(), 4);
    let para = block.node;

    assert!(matches!(session.locate(InlinePos::new(para, 0)).unwrap(), Loc::Boundary { index: 0 }));
    assert!(matches!(
        session.locate(InlinePos::new(para, 1)).unwrap(),
        Loc::InText { byte_offset: 1, .. }
    ));
    let err = session.locate(InlinePos::new(para, 2)).unwrap_err();
    assert!(matches!(err, Error::EditPlan { code: DiagCode::EditSplitSurrogate, .. }));
    assert!(matches!(
        session.locate(InlinePos::new(para, 3)).unwrap(),
        Loc::InText { byte_offset: 5, .. }
    ));
    assert!(matches!(session.locate(InlinePos::new(para, 4)).unwrap(), Loc::Boundary { index: 1 }));
}

#[test]
fn edit_02_atomic_inline_occupies_one_utf16_unit() {
    let bytes = minimal_docx(
        "<w:p><w:r><w:t>x</w:t></w:r>\
         <w:r><w:br w:type=\"page\"/></w:r>\
         <w:r><w:t>y</w:t></w:r></w:p>",
    );
    let session = EditSession::open(&bytes).unwrap();
    let block = session.document().text_blocks().next().unwrap();
    assert_eq!(block.utf16_len(), 3, "text + page atom + text");
    let before = session.locate(InlinePos::new(block.node, 1)).unwrap();
    let after = session.locate(InlinePos::new(block.node, 2)).unwrap();
    match (before, after) {
        (Loc::Boundary { index: a }, Loc::Boundary { index: b }) => assert_eq!(b - a, 1),
        other => panic!("expected two boundaries around the atom, got {other:?}"),
    }
}

#[test]
fn edit_02_rejects_non_text_paragraph_and_out_of_range_offset() {
    let bytes = minimal_docx("<w:p><w:pPr><w:sectPr/></w:pPr></w:p>");
    let session = EditSession::open(&bytes).unwrap();
    let mut block_nodes = session.document().main.iter().map(|b| b.node());
    let para = block_nodes.next().unwrap();
    let err = session.locate(InlinePos::new(para, 0)).unwrap_err();
    assert!(matches!(err, Error::EditPlan { code: DiagCode::EditInvalidPosition, .. }));

    let bytes = minimal_docx("<w:p><w:r><w:t>abc</w:t></w:r></w:p>");
    let session = EditSession::open(&bytes).unwrap();
    let para = session.document().text_blocks().next().unwrap().node;
    let err = session.locate(InlinePos::new(para, 4)).unwrap_err();
    assert!(matches!(err, Error::EditPlan { code: DiagCode::EditInvalidPosition, .. }));
}

#[test]
fn edit_05_commit_plans_apply_rebuild_and_report_offset_delta() {
    let bytes = minimal_docx("<w:p><w:r><w:t>abc</w:t></w:r></w:p>");
    let mut session = EditSession::open(&bytes).unwrap();
    let para = first_paragraph(&session);
    let (main, t) = first_t(&session);
    let space = QName::new(NsId::Xml, LocalName::Space);

    let plan = MutationPlan::for_part(
        main,
        vec![NodeEdit::SetAttr { node: Target::Node(t), name: space, value: "preserve".into() }],
    )
    .with_offset_delta(vec![(para, Utf16Offset(1), 3)]);
    plan.validate(&session).unwrap();
    let result = session.commit(plan).unwrap();

    assert_eq!(result.offset_delta, vec![(para, Utf16Offset(1), 3)]);
    assert!(session.package().is_dirty());
    assert_eq!(
        session.package().part(main).dom().unwrap().attr_value(t, space).as_deref(),
        Some("preserve")
    );
    assert_eq!(session.document().text_blocks().count(), 1);

    let saved = session.save(SaveOptions::default()).unwrap();
    assert_ne!(saved, bytes);
    let mut reopened = Package::open(&saved).unwrap();
    let xml = reopened.read_bytes(reopened.main_part()).unwrap();
    assert!(String::from_utf8(xml).unwrap().contains("xml:space=\"preserve\""));
}

#[test]
fn edit_05_failed_batch_validates_all_before_any_commit() {
    let bytes = minimal_docx("<w:p><w:r><w:t>abc</w:t></w:r></w:p>");
    let mut session = EditSession::open(&bytes).unwrap();
    let before_model = session.document().clone();
    let (main, t) = first_t(&session);
    let space = QName::new(NsId::Xml, LocalName::Space);
    let valid = MutationPlan::for_part(
        main,
        vec![NodeEdit::SetAttr { node: Target::Node(t), name: space, value: "preserve".into() }],
    );
    let invalid = MutationPlan::for_part(
        main,
        vec![NodeEdit::SetAttr {
            node: Target::Node(NodeId(999_999)),
            name: space,
            value: "preserve".into(),
        }],
    );

    let err = session.apply_plans(vec![valid.clone(), invalid]).unwrap_err();
    assert!(matches!(err, Error::EditPlan { code: DiagCode::EditInvalidPlan, .. }));
    assert!(!session.package().is_dirty(), "failed batch must not modify DOM");
    assert_eq!(session.document(), &before_model);
    assert_eq!(session.package().part(main).dom().unwrap().attr_value(t, space).as_deref(), None);

    // 同一批中的第一个 plan 单独提交可成功，证明失败确实发生在第二个 plan 的验证阶段。
    session.commit(valid).unwrap();
    assert_eq!(
        session.package().part(main).dom().unwrap().attr_value(t, space).as_deref(),
        Some("preserve")
    );
}

#[test]
fn edit_01_unsupported_ops_do_not_touch_the_document() {
    use crate::model::Block;
    let bytes = minimal_docx("<w:p><w:r><w:t>abc</w:t></w:r></w:p>");
    let mut session = EditSession::open(&bytes).unwrap();
    let para = session.document().main.iter().find_map(Block::as_text).unwrap().node;
    let before_model = session.document().clone();

    let err = session
        .apply(
            EditOp::DeleteRange { from: InlinePos::new(para, 0), to: InlinePos::new(para, 1) },
            &EditContext::default(),
        )
        .unwrap_err();
    assert!(matches!(err, Error::EditUnsupported { operation: "DeleteRange" }));
    assert!(!session.package().is_dirty());
    assert_eq!(session.document(), &before_model);
}

#[test]
fn edit_02_diagnostics_keep_validation_origin_constants() {
    assert_eq!(
        crate::error::Error::EditPlan { code: DiagCode::EditInvalidPlan, message: String::new() }
            .to_string(),
        "edit rejected (EDIT_INVALID_PLAN): "
    );
    assert_eq!(
        ValidationOrigin::EngineInvariantViolation,
        crate::diag::ValidationOrigin::EngineInvariantViolation
    );

    let dom_session = EditSession::open(&minimal_docx("<w:p/>")).unwrap();
    let dom = dom_session.package().part(dom_session.package().main_part()).dom().unwrap();
    let p = dom.descendants(dom.root()).find(|&n| dom.is(n, QName::w(LocalName::P))).unwrap();
    assert_eq!(dom.node(p).dirty, Dirty::Clean);
}
