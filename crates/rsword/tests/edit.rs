//! 编辑会话公开 API（`EDIT-01/02/05`）：语料打开→默认保存字节相同、正文坐标流
//! 定位、公开低层 `MutationPlan::for_part` 的 validate → commit → save。

mod common;

use rsword::edit::{EditSession, InlinePos, Loc, MutationPlan, Utf16Offset};
use rsword::error::Error;
use rsword::package::Package;
use rsword::save::SaveOptions;
use rsword::xml::plan::{NodeEdit, Target};
use rsword::xml::{LocalName, NsId, QName};

#[test]
fn edit_01_session_no_edit_save_is_byte_identical_on_all_openable_corpus() {
    let mut opened = 0;
    for kind in ["synthetic", "hostile"] {
        for path in common::docx_paths(kind) {
            let bytes = std::fs::read(&path).unwrap();
            let Ok(mut session) = EditSession::open(&bytes) else { continue };
            assert!(!session.package().is_dirty(), "{}", path.display());
            assert_eq!(session.save(SaveOptions::default()).unwrap(), bytes, "{}", path.display());
            opened += 1;
        }
    }
    assert!(opened > 580, "expected the full exported corpus to open, got {opened}");
}

#[test]
fn edit_02_locate_first_text_block_coordinate_boundaries() {
    let path = common::docx_paths("synthetic")
        .into_iter()
        .find_map(|path| {
            let session = EditSession::open(&std::fs::read(&path).unwrap()).ok()?;
            let block = session.document().text_blocks().next()?;
            (block.utf16_len() > 0).then(|| (path, block.node, block.utf16_len()))
        })
        .expect("corpus has at least one non-empty text block");
    let (path, para, len) = path;
    let session = EditSession::open(&std::fs::read(&path).unwrap()).unwrap();
    assert!(matches!(session.locate(InlinePos::new(para, 0)).unwrap(), Loc::Boundary { .. }));
    assert!(matches!(
        session.locate(InlinePos::new(para, len)).unwrap(),
        Loc::Boundary { index }
        if index > 0
    ));
    let err = session.locate(InlinePos::new(para, len + 1)).unwrap_err();
    assert!(matches!(
        err,
        Error::EditPlan { code: rsword::diag::DiagCode::EditInvalidPosition, .. }
    ));
}

#[test]
fn edit_05_public_low_level_plan_goes_through_validate_commit_and_save() {
    let bytes = std::fs::read(common::docx_paths("synthetic").into_iter().next().unwrap()).unwrap();
    let mut session = EditSession::open(&bytes).unwrap();
    let main = session.package().main_part();
    let dom = session.package().part(main).dom().unwrap();
    let t = QName::w(LocalName::T);
    let target = dom.descendants(dom.root()).find(|&id| dom.is(id, t)).unwrap();
    let offset_para = session.document().text_blocks().next().map(|b| b.node).unwrap();
    let space = QName::new(NsId::Xml, LocalName::Space);

    let plan = MutationPlan::for_part(
        main,
        vec![NodeEdit::SetAttr {
            node: Target::Node(target),
            name: space,
            value: "preserve".into(),
        }],
    )
    .with_offset_delta(vec![(offset_para, Utf16Offset(0), 0)]);
    let result = session.commit(plan).unwrap();
    assert!(!result.dirty_nodes.is_empty());
    assert!(session.package().is_dirty());

    let saved = session.save(SaveOptions::default()).unwrap();
    assert_ne!(saved, bytes);
    let mut reopened = Package::open(&saved).unwrap();
    let main2 = reopened.main_part();
    let dom2 = reopened.dom(main2).unwrap().unwrap();
    let target2 = dom2.descendants(dom2.root()).find(|&id| dom2.is(id, t)).unwrap();
    assert_eq!(dom2.attr_value(target2, space).as_deref(), Some("preserve"));
}
