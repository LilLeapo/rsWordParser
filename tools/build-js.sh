#!/usr/bin/env bash
# 构建 rsword-js 的 wasm 产物（M8′ 8.0②）：wasm32 + wasm-release → wasm-bindgen glue
# 落 `crates/rsword-js/pkg/`，给 `tools/js-parity/` 与 `diff-parse --via-js` 用。
#
# 用法: tools/build-js.sh
#
# 做的事：
#   1. 以仓库当前提交构建 wasm32（wasm-release profile，提交号经 RSWORD_COMMIT 嵌进 version()）；
#   2. wasm-bindgen --target web 生成 glue（wasm-opt -Oz 有则再压，见 TOOLS.md）。
# 产物不进 git（.gitignore）；wasm-bindgen-cli 必须与 Cargo.lock 的 wasm-bindgen 同版本。
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/.." && pwd)
sha=$(git -C "$repo_root" rev-parse --short=12 HEAD)

cd "$repo_root/crates/rsword-js"
RSWORD_COMMIT="$sha" cargo build --target wasm32-unknown-unknown --profile wasm-release "$@"
wasm_out=../../target/wasm32-unknown-unknown/wasm-release/rsword_js.wasm
mkdir -p pkg
wasm-bindgen --target web --out-dir pkg "$wasm_out"
if command -v wasm-opt >/dev/null 2>&1; then
    wasm-opt -Oz pkg/rsword_js_bg.wasm -o pkg/rsword_js_bg.opt.wasm
    mv pkg/rsword_js_bg.opt.wasm pkg/rsword_js_bg.wasm
else
    echo "build-js: 未装 wasm-opt（binaryen），跳过 -Oz（可选，见 TOOLS.md）" >&2
fi
echo "build-js: $sha -> crates/rsword-js/pkg ($(du -h pkg/rsword_js_bg.wasm | cut -f1))"
