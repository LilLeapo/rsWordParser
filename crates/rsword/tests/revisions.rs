//! 修订索引与修订相关病态输入（`MOD-09`、`TEST-09`；`spec/18` 7.0 / 7.1）。

mod common;

use rsword::edit::EditSession;

/// `TEST-09` 三条：解析成功、局部降级（无引擎不变式违规）、无编辑保存字节相同。
#[test]
fn test_09_hostile_revision_documents() {
    for name in [
        "rev-nested-wrappers.docx",
        "rev-move-unpaired.docx",
        "rev-change-empty.docx",
        "rev-del-with-t.docx",
        "sectpr-in-cell.docx",
        "drawing-anchor-no-extent.docx",
    ] {
        let bytes = std::fs::read(common::corpus_dir("hostile").join(name)).unwrap();
        let mut s = EditSession::open(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!s.document().main.is_empty(), "{name}: 投影出块");
        assert!(
            !s.diagnostics()
                .iter()
                .any(|d| d.origin == rsword::ValidationOrigin::EngineInvariantViolation),
            "{name}: {:?}",
            s.diagnostics()
        );
        assert_eq!(s.save().unwrap(), bytes, "{name}: 无编辑保存字节相同");
    }
}
