//! TEST-06/10：自有协议快照与全语料不变式 1；绝不更新 TS 期望值。
mod common;

use rsword::bind::native::SessionTable;
use std::collections::BTreeSet;

const UNOPENABLE: [&str; 4] = [
    "hostile/xml-unbalanced-main.docx",
    "hostile/zip-part-too-large.docx",
    "hostile/zip-too-many-parts.docx",
    "hostile/zip-total-too-large.docx",
];

fn corpus() -> Vec<std::path::PathBuf> {
    let paths: Vec<_> =
        ["synthetic", "real", "hostile"].into_iter().flat_map(common::docx_paths).collect();
    assert_eq!(paths.len(), 1103, "全语料数量漂移");
    paths
}

#[test]
fn test_10_full_corpus_model_snapshots_and_no_edit_save_identity() {
    let update = std::env::var("RSWORD_UPDATE_MODEL_SNAPSHOTS").as_deref() == Ok("1");
    let root = common::repo_root().join("corpus");
    let mut refused = BTreeSet::new();
    let mut projected = 0;
    let mut expected_files = BTreeSet::new();
    let mut table = SessionTable::default();
    for path in corpus() {
        let name = path.strip_prefix(&root).unwrap().to_str().unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let snapshot = path.with_extension("model.json");
        let id = match table.open(&bytes, None) {
            Ok(id) => {
                assert!(!UNOPENABLE.contains(&name), "{name}: 必须拒绝的文档却打开成功");
                id
            }
            Err(error) => {
                assert!(UNOPENABLE.contains(&name), "{name}: 意外拒绝 {error}");
                assert!(!snapshot.exists(), "{name}: 拒绝文档不得伪造模型快照");
                refused.insert(name.to_owned());
                continue;
            }
        };
        // 8.3 已覆盖 synthetic + real；在此正式命名，并仅扩展到 hostile。
        assert_eq!(table.save(&id, None).unwrap(), bytes, "{name}: 无编辑保存改变原字节");
        let actual = table.document(&id, Some(r#"{"display":false}"#)).unwrap() + "\n";
        if update {
            std::fs::write(&snapshot, &actual).unwrap();
        }
        let expected = std::fs::read_to_string(&snapshot).unwrap_or_else(|e| {
            panic!("{}: {e}; 显式设置 RSWORD_UPDATE_MODEL_SNAPSHOTS=1 生成", snapshot.display())
        });
        // 比字节而非 Value，键序与可观察序列也属于快照。
        assert!(
            actual == expected,
            "{}: 模型快照漂移（实际 {} B，期望 {} B）",
            snapshot.display(),
            actual.len(),
            expected.len()
        );
        expected_files.insert(snapshot);
        table.close(&id);
        projected += 1;
    }
    assert_eq!(refused, UNOPENABLE.into_iter().map(str::to_owned).collect());
    assert_eq!(projected, 1099);
    let mut found = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.file_name().unwrap().to_string_lossy().ends_with(".model.json") {
                found.insert(path);
            }
        }
    }
    assert_eq!(found, expected_files, "模型快照文件集合漂移：缺失或孤儿快照");
    eprintln!(
        "TEST-10: {projected} model snapshots + no-edit byte identities; {} named open refusals",
        refused.len()
    );
}
