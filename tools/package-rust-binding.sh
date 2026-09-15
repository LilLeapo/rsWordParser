#!/usr/bin/env bash
# BIND-11：交付完整 crate 与原生 binding，在仓库外验证真实下游的读改存。
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/.." && pwd)
cd "$repo_root"
export RSWORD_COMMIT="${RSWORD_COMMIT:-$(git rev-parse HEAD)}"
cargo package --locked -p rsword
version=$(python3 -c 'import tomllib; print(tomllib.load(open("crates/rsword/Cargo.toml", "rb"))["package"]["version"])')

stage=$(mktemp -d "${TMPDIR:-/tmp}/rsword-rustbinding.XXXXXX")
trap 'rm -rf "$stage"' EXIT
bundle="$stage/rsword-rustbinding"
mkdir -p "$bundle/rsword" "$bundle/example/src/bin"
tar -xzf "target/package/rsword-$version.crate" -C "$bundle/rsword" --strip-components=1
cp tools/rust-binding/README.md "$bundle/README.md"
cp crates/rsword/examples/read.rs "$bundle/example/src/bin/read.rs"
cp crates/rsword/examples/agent.rs "$bundle/example/src/bin/edit.rs"
cat > "$bundle/example/Cargo.toml" <<'TOML'
[package]
name = "rsword-binding-example"
version = "0.1.0"
edition = "2024"
publish = false

[workspace]

[dependencies]
rsword = { path = "../rsword" }
serde_json = "1"
TOML

# 示例仅引用解包后的 crate；编译缓存留在仓库 target，发布包不包含构建缓存。
export CARGO_TARGET_DIR="$repo_root/target/rust-binding-check"
cd "$bundle/example"
cargo generate-lockfile
cargo build --locked --release --bins
cargo run --locked --release --bin read -- "$repo_root/corpus/synthetic/bidi__001.docx"
cargo run --locked --release --bin edit -- "$repo_root/corpus/synthetic/bidi__001.docx" "Rust binding 编辑验证"

mkdir -p "$repo_root/target/release-artifacts"
tar -czf "$repo_root/target/release-artifacts/rsword-rustbinding.tar.gz" -C "$stage" rsword-rustbinding
echo "Rust binding: target/release-artifacts/rsword-rustbinding.tar.gz"
