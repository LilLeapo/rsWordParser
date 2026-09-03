//! 语料发现（`TEST-01` 布局）。集成测试共用。

use std::path::{Path, PathBuf};

/// 仓库根目录（`crates/rsword/../..`）。
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repo root")
}

pub fn corpus_dir(kind: &str) -> PathBuf {
    repo_root().join("corpus").join(kind)
}

/// `corpus/<kind>/*.docx`，按文件名排序，保证测试输出稳定。
pub fn docx_paths(kind: &str) -> Vec<PathBuf> {
    let dir = corpus_dir(kind);
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut v: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "docx"))
        .collect();
    v.sort();
    v
}
