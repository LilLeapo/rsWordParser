#!/usr/bin/env bash
# 只跑**一个**导出用例文件，产物写到临时目录，**不碰** corpus/。开发 *.export.test.ts 时用它验证；
# 全量重导（会清空并重建整个 corpus/synthetic 与 corpus/hostile）只能用 run.sh。
# 用法：tools/export-golden/try.sh <文件名或路径>.export.test.ts [输出目录] [vitest 额外参数…]
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
GENOFFICE="${GENOFFICE_DIR:-$HOME/code/genoffice}"
ENGINE="$GENOFFICE/packages/docx-engine"
VITEST="$GENOFFICE/node_modules/.bin/vitest"
FILE="$(basename "${1:?用法: try.sh <file.export.test.ts> [out-dir]}")"
OUT="${2:-$(mktemp -d "${TMPDIR:-/tmp}/export-golden.XXXXXX")}"
shift; [ $# -gt 0 ] && shift
# 每次调用一个独立的 stage 目录：多个人同时跑互不干扰（vitest.config.ts 的 include 是 export-golden.tmp*/）
STAGE="$ENGINE/export-golden.tmp.$$"

[ -f "$HERE/$FILE" ] || { echo "找不到 $HERE/$FILE" >&2; exit 1; }
[ -x "$VITEST" ] || { echo "vitest not installed in $GENOFFICE (run npm install there)" >&2; exit 1; }

mkdir -p "$OUT/hostile"
rm -rf "$STAGE"; mkdir -p "$STAGE"
cp "$HERE"/*.ts "$STAGE"/
trap 'rm -rf "$STAGE"' EXIT

status=0
( cd "$ENGINE" && EXPORT_GOLDEN_OUT="$OUT" EXPORT_GOLDEN_HOSTILE_OUT="$OUT/hostile" \
    "$VITEST" run --config "$(basename "$STAGE")/vitest.config.ts" "$(basename "$STAGE")/$FILE" "$@" ) || status=$?
rm -rf "$OUT/.hash"

echo "out: $OUT"
echo "docx: $(ls "$OUT"/*.docx 2>/dev/null | wc -l | tr -d ' ')  expected: $(ls "$OUT"/*.expected.json 2>/dev/null | wc -l | tr -d ' ')  error: $(ls "$OUT"/*.error.json 2>/dev/null | wc -l | tr -d ' ')  save: $(ls "$OUT"/*.save.*.json 2>/dev/null | wc -l | tr -d ' ')  hostile: $(ls "$OUT"/hostile 2>/dev/null | wc -l | tr -d ' ')"
exit "$status"
