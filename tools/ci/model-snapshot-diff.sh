#!/usr/bin/env bash
# TEST-10：快照大批变化提示，不替代逐份快照比较与提交说明。
set -euo pipefail
base=${1:?base commit required}
head=${2:-HEAD}
threshold=${3:-20}
count=$(git diff --name-only "$base" "$head" -- ':(glob)corpus/**/*.model.json' | wc -l | tr -d ' ')
message="Model snapshots changed: $count (warning threshold: $threshold). Explain the reason and affected documents in the commit message."
echo "$message"
if [ "$count" -gt "$threshold" ]; then
  echo "::warning title=Large model snapshot diff::$message"
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    printf '## Model snapshot review required\n\n%s\n' "$message" >> "$GITHUB_STEP_SUMMARY"
  fi
fi
