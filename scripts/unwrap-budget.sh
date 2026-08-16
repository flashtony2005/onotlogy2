#!/bin/sh
# M6 unwrap 预算门禁：src/ 中 `.unwrap()` 数量不得超过预算（只降不升）。
# 预算文件：scripts/unwrap-budget.txt（手工递减；每清一批 unwrap 同步下调）。
set -e
BUDGET_FILE="$(dirname "$0")/unwrap-budget.txt"
BUDGET="$(cat "$BUDGET_FILE")"
COUNT="$(grep -rn '\.unwrap()' src/ --include='*.rs' | wc -l | tr -d ' ')"
if [ "$COUNT" -gt "$BUDGET" ]; then
  echo "❌ unwrap 预算超限：当前 $COUNT > 预算 $BUDGET（请清理后下调 scripts/unwrap-budget.txt）"
  exit 1
fi
echo "✅ unwrap 预算内：$COUNT ≤ $BUDGET"
