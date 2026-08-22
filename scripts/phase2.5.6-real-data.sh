#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DATA_ROOT="${ONTOLOGY_LOTTERY_SOURCE_ROOT:-/Users/lushaohui/project/test灯}"
DB_PATH="${1:-/tmp/ontology-phase-2-5-6-real.db}"
PACK_PATH="${ROOT_DIR}/domain-packs/lottery-analysis"

if [[ ! -f "${DATA_ROOT}/dlt.csv" || ! -f "${DATA_ROOT}/double.csv" ]]; then
  echo "缺少真实数据：${DATA_ROOT}/dlt.csv 或 ${DATA_ROOT}/double.csv" >&2
  exit 2
fi

rm -f "${DB_PATH}"
export ONTOLOGY_LOTTERY_SOURCE_ROOT="${DATA_ROOT}"

run_oaip() {
  cargo run --quiet -- --db "${DB_PATH}" "$@"
}

run_oaip domain-pack install "${PACK_PATH}"

for game in dlt double; do
  run_oaip lottery import --game "${game}" "${DATA_ROOT}/${game}.csv"
  run_oaip lottery analyze --game "${game}" --window 50
  run_oaip lottery backtest --game "${game}" --train-window 50 --test-count 30
  run_oaip lottery import --game "${game}" "${DATA_ROOT}/${game}.csv" | tee "/tmp/ontology-phase-2-5-6-${game}-repeat.log"
done

echo "真实数据验证完成：${DB_PATH}"
