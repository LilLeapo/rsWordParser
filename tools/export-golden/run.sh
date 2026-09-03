#!/usr/bin/env bash
# TEST-02 语料导出。用法：tools/export-golden/run.sh [vitest 过滤参数…]
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
GENOFFICE="${GENOFFICE_DIR:-$HOME/code/genoffice}"
ENGINE="$GENOFFICE/packages/docx-engine"
STAGE="$ENGINE/export-golden.tmp"
OUT="$REPO/corpus/synthetic"
HOSTILE="$REPO/corpus/hostile"
VITEST="$GENOFFICE/node_modules/.bin/vitest"

[ -f "$ENGINE/tests/helpers/build-docx.ts" ] || { echo "genoffice docx-engine not found at $ENGINE (set GENOFFICE_DIR)"; exit 1; }
[ -x "$VITEST" ] || { echo "vitest not installed in $GENOFFICE (run npm install there)"; exit 1; }

mkdir -p "$OUT" "$HOSTILE"
# 只清理本工具产生的文件类型
find "$OUT" -maxdepth 1 -type f \( -name '*.docx' -o -name '*.json' -o -name '*.jsonl' \) -delete
rm -rf "$OUT/.hash"
find "$HOSTILE" -maxdepth 1 -type f \( -name '*.docx' -o -name '*.zip' -o -name '*.json' \) -delete

COMMIT="$(git -C "$GENOFFICE" rev-parse --short HEAD)"
DIRTY="$(git -C "$GENOFFICE" status --porcelain | grep -v 'export-golden.tmp' | wc -l | tr -d ' ')"
printf '{"genoffice_commit":"%s","genoffice_dirty_files":%s,"exported_at":"%s"}\n' \
  "$COMMIT" "$DIRTY" "$(date -u +%FT%TZ)" > "$OUT/manifest.jsonl"

rm -rf "$STAGE"; mkdir -p "$STAGE"
cp "$HERE"/*.ts "$STAGE"/
trap 'rm -rf "$STAGE"' EXIT

status=0
( cd "$ENGINE" && EXPORT_GOLDEN_OUT="$OUT" EXPORT_GOLDEN_HOSTILE_OUT="$HOSTILE" \
    "$VITEST" run --config export-golden.tmp/vitest.config.ts "$@" ) || status=$?
rm -rf "$OUT/.hash"

echo "synthetic: $(ls "$OUT"/*.docx 2>/dev/null | wc -l | tr -d ' ') docx, $(ls "$OUT"/*.save.*.json 2>/dev/null | wc -l | tr -d ' ') save.json"
echo "hostile:   $(ls "$HOSTILE"/*.docx 2>/dev/null | wc -l | tr -d ' ') files"
if [ "$status" -ne 0 ]; then
  echo "WARNING: vitest exited with $status (some genoffice tests failed under the recording wrappers); corpus still written" >&2
fi
exit "$status"
