#!/usr/bin/env bash
# BIND-11：隔离 Cargo feature 统一，真实下游默认构建必须能编辑且无法导入 compat_ts。
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
probe=$(mktemp -d "${TMPDIR:-/tmp}/rsword-native-default.XXXXXX")
trap 'rm -rf "$probe"' EXIT
mkdir "$probe/src"
cp "$root/Cargo.lock" "$probe/Cargo.lock"
cat > "$probe/Cargo.toml" <<TOML
[package]
name = "rsword-native-default-probe"
version = "0.0.0"
edition = "2024"
[workspace]
[features]
forbidden-compat = []
forbidden-ts-shape = []
[dependencies]
rsword = { path = "$root/crates/rsword" }
TOML
cat > "$probe/src/main.rs" <<'RS'
#[cfg(feature = "forbidden-compat")]
use rsword::bind::compat_ts;
#[cfg(feature = "forbidden-ts-shape")]
fn forbidden_ts_shape() {
    let _ = rsword::span::field::generate::toc::TocOptions {
        ts_shape: true, ..Default::default()
    };
}
fn main() {
    let bytes = rsword::EditSession::blank(None).unwrap().save().unwrap();
    let core = rsword::EditSession::open(&bytes).unwrap();
    let para = core.document().paragraphs().next().unwrap().node.0;
    let mut table = rsword::bind::native::SessionTable::default();
    let id = table.open(&bytes, None).unwrap();
    table.document(&id, None).unwrap();
    let op = format!(r#"{{"op":"insertText","at":{{"para":{para},"offset":0}},"text":"native default"}}"#);
    table.apply(&id, &op, None).unwrap();
    let saved = table.save(&id, None).unwrap();
    let reopened = table.open(&saved, None).unwrap();
    assert!(table.document(&reopened, None).unwrap().contains("native default"));
}
RS
export CARGO_TARGET_DIR="$root/target/native-default-probe"
cargo run --manifest-path "$probe/Cargo.toml" --quiet
for probe_case in forbidden-compat:E0432 forbidden-ts-shape:E0560; do
    feature=${probe_case%:*}
    expected=${probe_case#*:}
    if cargo check --manifest-path "$probe/Cargo.toml" --features "$feature" > "$probe/rejected.log" 2>&1; then
        echo "BIND-11 failed: default downstream accepts $feature" >&2
        exit 1
    fi
    # grep -F，不用 rg：GitHub 的 ubuntu-latest 没装 ripgrep。rg 缺失时
    # `! rg -q` 恒为真，这道负向门就从「验证拒绝理由」退化成「永远报错」——
    # 本地有 rg 所以一直看不见。用 POSIX 工具消掉这个依赖。
    if ! grep -qF "error[$expected]" "$probe/rejected.log"; then
        cat "$probe/rejected.log" >&2
        exit 1
    fi
done
echo 'BIND-11: default lifecycle passed; compat_ts E0432 and TocOptions.ts_shape E0560 verified'
