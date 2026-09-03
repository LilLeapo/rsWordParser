//! `TEST-01` 语料布局就位；`TEST-04`/`SAVE-01` 往返门在任务 0.11 之后接到 `docx_paths("synthetic")` 上。

mod common;

#[test]
fn test_01_corpus_layout_exists() {
    for kind in ["synthetic", "real", "hostile"] {
        let dir = common::corpus_dir(kind);
        assert!(dir.is_dir(), "missing corpus dir {}", dir.display());
    }
    assert!(common::repo_root().join("fixtures/resolve").is_dir(), "missing fixtures/resolve");
    // 语料可能尚未导出（任务 0.1）；有则必须成对出现 expected.json。
    for docx in common::docx_paths("synthetic") {
        let expected = docx.with_extension("expected.json");
        let error = docx.with_extension("error.json");
        assert!(
            expected.is_file() || error.is_file(),
            "{} has neither .expected.json nor .error.json",
            docx.display()
        );
    }
}
