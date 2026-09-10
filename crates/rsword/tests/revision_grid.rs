//! SAVE-02 / EDIT-05：拒绝列修订保留当前存活列；推导失败不得提交非法网格。
mod common;
use rsword::edit::{BlockPos, EditContext, EditOp, EditSession, NewBlock, RevisionAuthor};
use rsword::semantic::props::Val;

fn tracked() -> EditContext {
    EditContext::default()
        .with_track_changes(Some(RevisionAuthor { author: "A".into(), date: None }))
}

#[test]
fn edit_03_reject_insert_preserves_current_column_widths() {
    for reject_all in [false, true] {
        let mut s = EditSession::open(&common::docx_with_body("<w:p/>")).unwrap();
        let plain = EditContext::default();
        s.apply(
            EditOp::InsertBlock {
                at: BlockPos::end(s.document().body.unwrap()),
                block: NewBlock::Table {
                    rows: 2,
                    cols: 3,
                    widths: Some(vec![1000, 2000, 3000]),
                    style: None,
                    header: false,
                },
            },
            &plain,
        )
        .unwrap();
        let table = s.document().tables().next().unwrap().node;
        s.apply(EditOp::InsertColumn { table, at: 1, width: 900 }, &tracked()).unwrap();
        s.apply(EditOp::InsertColumn { table, at: 3, width: 1700 }, &plain).unwrap();
        let rev = s
            .document()
            .revisions
            .entries()
            .iter()
            .find(|r| r.kind == rsword::model::RevKind::TableGridChange)
            .unwrap()
            .id;
        let op = if reject_all {
            EditOp::RejectAll { author: Some("A".into()) }
        } else {
            EditOp::RejectRevision { rev }
        };
        s.apply(op, &plain).unwrap();
        let t = s.document().tables().next().unwrap();
        assert_eq!(
            t.grid.iter().map(|c| *c.w.as_ref().and_then(Val::value).unwrap()).collect::<Vec<_>>(),
            [1000, 2000, 1700, 3000]
        );
        assert!(t.rows.iter().all(|r| r.cells.iter().map(|c| c.grid_span()).sum::<u32>() == 4));
        s.save().unwrap();
    }
}

#[test]
fn save_02_reject_inconsistent_grid_snapshot_is_atomic() {
    let body = r#"<w:tbl><w:tblGrid><w:gridCol w:w="1000"/><w:gridCol w:w="2000"/><w:tblGridChange w:id="1"><w:tblGrid><w:gridCol w:w="3000"/></w:tblGrid></w:tblGridChange></w:tblGrid><w:tr><w:tc><w:p/></w:tc><w:tc><w:p/></w:tc></w:tr></w:tbl>"#;
    let mut s = EditSession::open(&common::docx_with_body(body)).unwrap();
    let state = |s: &EditSession| {
        (
            s.clone().save().unwrap(),
            rsword::bind::native::document_json(
                s.package(),
                s.document(),
                rsword::bind::native::DocumentOpts { display: false },
            )
            .to_string(),
            rsword::bind::native::edit::edit_diagnostics_json(s).to_string(),
            format!("{:?}", s.dom()),
        )
    };
    let before = state(&s);
    let rev = s
        .document()
        .revisions
        .entries()
        .iter()
        .find(|r| r.kind == rsword::model::RevKind::TableGridChange)
        .unwrap()
        .id;
    let err = s.apply(EditOp::RejectRevision { rev }, &EditContext::default()).unwrap_err();
    assert!(matches!(
        err,
        rsword::Error::Edit { code: rsword::DiagCode::EditTableGridInconsistent, .. }
    ));
    assert_eq!(state(&s), before, "完整会话必须回滚");
}
