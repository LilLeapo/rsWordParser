//! 构建脚本（M8′ 8.0②）：把 rsWordParser 提交号嵌进 `version()`。
//!
//! `RSWORD_COMMIT` 环境变量优先（`tools/build-js.sh` 构建时设置）；没有就问 git；
//! 都没有（如纯 tarball 构建）落 `unknown`。

use std::process::Command;

fn main() {
    let sha = std::env::var("RSWORD_COMMIT")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(12).collect::<String>())
        .or_else(|| {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            Command::new("git")
                .args(["rev-parse", "--short=12", "HEAD"])
                .current_dir(&root)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=RSWORD_GIT_SHA={sha}");
    println!("cargo:rerun-if-env-changed=RSWORD_COMMIT");
}
