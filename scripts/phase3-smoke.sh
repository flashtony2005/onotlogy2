#!/usr/bin/env bash
set -euo pipefail

# S3e smoke test: start a real release binary, verify probes, then stop it cleanly.
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${OAIP_BIN:-${ROOT_DIR}/target/release/oaip}"
WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/oaip-phase3.XXXXXX")"
DB="${WORK_DIR}/openaip.db"
BACKUP="${WORK_DIR}/openaip-backup.db"
RESTORED="${WORK_DIR}/openaip-restored.db"
LOG="${WORK_DIR}/server.log"
PORT="${OAIP_SMOKE_PORT:-18080}"
PID=""
cleanup() {
  if [[ -n "${PID}" ]] && kill -0 "${PID}" 2>/dev/null; then
    kill -TERM "${PID}" 2>/dev/null || true
    wait "${PID}" 2>/dev/null || true
  fi
  rm -rf "${WORK_DIR}"
}
trap cleanup EXIT

if [[ ! -x "${BIN}" ]]; then
  # Keep the artifact at the documented path even when the developer's Cargo
  # config sets a global target-dir.
  cargo build --release --target-dir "${ROOT_DIR}/target" --bin oaip
fi
if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required for deployment smoke; install it and retry" >&2
  exit 2
fi

export WB_BOOTSTRAP_ADMIN_USER="${WB_BOOTSTRAP_ADMIN_USER:-smoke-admin}"
export WB_BOOTSTRAP_ADMIN_PWD="${WB_BOOTSTRAP_ADMIN_PWD:-smoke-password-change-me}"
"${BIN}" --db "${DB}" serve --host 127.0.0.1 --port "${PORT}" --static-dir "${ROOT_DIR}/ui/dist" --odl "${ROOT_DIR}/examples/crm.odl" >"${LOG}" 2>&1 &
PID=$!

ready=0
for _ in $(seq 1 60); do
  if curl --connect-timeout 1 --max-time 5 --fail --silent \
    "http://127.0.0.1:${PORT}/readyz" >/dev/null; then
    ready=1
    break
  fi
  if ! kill -0 "${PID}" 2>/dev/null; then
    break
  fi
  sleep 0.25
done
if [[ "${ready}" -ne 1 ]]; then
  echo "release binary did not become ready within 15 seconds" >&2
  cat "${LOG}" >&2 || true
  exit 1
fi
curl --connect-timeout 1 --max-time 5 --fail --silent \
  "http://127.0.0.1:${PORT}/healthz" | grep --fixed-strings --quiet '"status":"ok"'
curl --connect-timeout 1 --max-time 5 --fail --silent \
  "http://127.0.0.1:${PORT}/readyz" | grep --fixed-strings --quiet '"status":"ready"'
curl --connect-timeout 1 --max-time 5 --fail --silent \
  "http://127.0.0.1:${PORT}/metrics" | grep --fixed-strings --quiet 'oaip_ready 1'
kill -TERM "${PID}"
wait "${PID}"

# Exercise the operational backup/restore commands against the same database
# that served the HTTP probes.  The service is stopped before the snapshot,
# matching the documented single-process local-first recovery contract.
"${BIN}" --db "${DB}" backup "${BACKUP}" >"${WORK_DIR}/backup.log" 2>&1
"${BIN}" --db "${RESTORED}" restore "${BACKUP}" >"${WORK_DIR}/restore.log" 2>&1
"${BIN}" --db "${RESTORED}" event list >"${WORK_DIR}/restored-events.log" 2>&1
test -s "${BACKUP}"
grep --fixed-strings --quiet '✅ 恢复完成' "${WORK_DIR}/restore.log"

echo "phase3 smoke test passed"
