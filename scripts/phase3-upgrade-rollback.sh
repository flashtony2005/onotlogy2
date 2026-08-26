#!/usr/bin/env bash
set -euo pipefail

# S3d rehearsal: exercise a real schema upgrade, a real snapshot, and a
# binary rollback against the pre-upgrade snapshot.  The old binary must be
# supplied explicitly; silently using the candidate binary would not test a
# rollback.
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CANDIDATE_BIN="${OAIP_CANDIDATE_BIN:-${ROOT_DIR}/target/release/oaip}"
BASE_BIN="${OAIP_BASE_BIN:-}"
BASE_REF="${OAIP_BASE_REF:-}"
WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/oaip-phase3-rehearsal.XXXXXX")"
DB="${WORK_DIR}/v1.db"
V1_SNAPSHOT="${WORK_DIR}/v1-pre-upgrade.db"
V2_SNAPSHOT="${WORK_DIR}/v2-post-upgrade.db"
V2_RESTORED="${WORK_DIR}/v2-restored.db"
ROLLBACK_DB="${WORK_DIR}/rollback.db"
LOG_DIR="${WORK_DIR}/logs"
mkdir -p "${LOG_DIR}"

cleanup() {
  rm -rf "${WORK_DIR}"
}
trap cleanup EXIT

copy_database() {
  local source="$1"
  local destination="$2"
  cp "${source}" "${destination}"
  for suffix in -wal -shm; do
    if [[ -f "${source}${suffix}" ]]; then
      cp "${source}${suffix}" "${destination}${suffix}"
    else
      rm -f "${destination}${suffix}"
    fi
  done
}

sha256_file() {
  local file="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${file}" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${file}" | awk '{print $1}'
  else
    echo "sha256sum or shasum is required for release evidence" >&2
    exit 2
  fi
}

schema_version() {
  sqlite3 "$1" "SELECT value FROM wb_meta WHERE key='schema_version'"
}

if [[ -z "${BASE_BIN}" && -n "${BASE_REF}" ]]; then
  BASE_SOURCE="${WORK_DIR}/base-source"
  mkdir -p "${BASE_SOURCE}"
  git archive "${BASE_REF}" | tar -x -C "${BASE_SOURCE}"
  # A global Cargo target-dir must not redirect the isolated base build into
  # the candidate's release directory.
  (cd "${BASE_SOURCE}" && CARGO_TARGET_DIR="${BASE_SOURCE}/target" cargo build --release --bin oaip >"${LOG_DIR}/base-build.log" 2>&1)
  BASE_BIN="${BASE_SOURCE}/target/release/oaip"
fi

if [[ -z "${BASE_BIN}" ]]; then
  echo "OAIP_BASE_BIN or OAIP_BASE_REF is required; refusing a fake rollback test" >&2
  exit 2
fi
for binary in "${BASE_BIN}" "${CANDIDATE_BIN}"; do
  if [[ ! -x "${binary}" ]]; then
    echo "binary is not executable: ${binary}" >&2
    exit 2
  fi
done

# V1: create data with the previous release and preserve the stopped-database
# rollback anchor before the new binary is allowed to migrate it.
"${BASE_BIN}" --db "${DB}" run >"${LOG_DIR}/v1-seed.log" 2>&1
copy_database "${DB}" "${V1_SNAPSHOT}"
V1_SCHEMA="$(schema_version "${V1_SNAPSHOT}")"
"${BASE_BIN}" --db "${DB}" object list Customer >"${LOG_DIR}/v1-verify.log" 2>&1
grep -q 001 "${LOG_DIR}/v1-verify.log"
grep -q 002 "${LOG_DIR}/v1-verify.log"

# V1 -> V2: opening the same database with the candidate performs the real
# in-place schema migration and then records a post-upgrade audit fact.
"${CANDIDATE_BIN}" --db "${DB}" object list Customer >"${LOG_DIR}/upgrade.log" 2>&1
grep -q 001 "${LOG_DIR}/upgrade.log"
"${CANDIDATE_BIN}" --db "${DB}" event emit phase3_upgrade_verified 001 >"${LOG_DIR}/upgrade-event.log" 2>&1
V2_SCHEMA="$(schema_version "${DB}")"
"${CANDIDATE_BIN}" --db "${DB}" backup "${V2_SNAPSHOT}" >"${LOG_DIR}/v2-backup.log" 2>&1

# Validate the production backup path independently from the binary rollback.
"${CANDIDATE_BIN}" --db "${V2_RESTORED}" restore "${V2_SNAPSHOT}" >"${LOG_DIR}/v2-restore.log" 2>&1
"${CANDIDATE_BIN}" --db "${V2_RESTORED}" event list >"${LOG_DIR}/v2-restored-verify.log" 2>&1
grep -q phase3_upgrade_verified "${LOG_DIR}/v2-restored-verify.log"

# A pre-upgrade binary must reject a newer post-upgrade schema; rollback is not
# allowed to pretend that incompatible data is readable. A patch release can
# keep the schema unchanged, making the old binary a valid direct rollback
# target after the new process has stopped.
if "${BASE_BIN}" --db "${DB}" object list Customer >"${LOG_DIR}/downgrade-rejected.log" 2>&1; then
  if [[ "${V1_SCHEMA}" != "${V2_SCHEMA}" ]]; then
    echo "older binary unexpectedly opened schema ${V2_SCHEMA} from ${V1_SCHEMA}" >&2
    exit 1
  fi
  grep -q 001 "${LOG_DIR}/downgrade-rejected.log"
  grep -q 002 "${LOG_DIR}/downgrade-rejected.log"
else
  grep -q WB2101 "${LOG_DIR}/downgrade-rejected.log"
fi

# V2 -> V1 rollback: restore the pre-upgrade snapshot, then prove the old
# binary can read the original objects and audit history.
restore_started="$(date +%s)"
copy_database "${V1_SNAPSHOT}" "${ROLLBACK_DB}"
"${BASE_BIN}" --db "${ROLLBACK_DB}" object list Customer >"${LOG_DIR}/rollback-verify.log" 2>&1
"${BASE_BIN}" --db "${ROLLBACK_DB}" event list >"${LOG_DIR}/rollback-events.log" 2>&1
grep -q 001 "${LOG_DIR}/rollback-verify.log"
grep -q 002 "${LOG_DIR}/rollback-verify.log"
grep -q ObjectCreated "${LOG_DIR}/rollback-events.log"
rto_seconds=$(( $(date +%s) - restore_started ))

echo "phase3 upgrade/rollback rehearsal passed"
echo "  base binary:      ${BASE_BIN}"
echo "  candidate binary: ${CANDIDATE_BIN}"
echo "  base version:     $("${BASE_BIN}" --version | awk '{print $2}')"
echo "  candidate version: $("${CANDIDATE_BIN}" --version | awk '{print $2}')"
echo "  schema migration:  ${V1_SCHEMA} -> ${V2_SCHEMA}"
echo "  base sha256:      $(sha256_file "${BASE_BIN}")"
echo "  candidate sha256: $(sha256_file "${CANDIDATE_BIN}")"
echo "  rollback RTO:     ${rto_seconds}s (local stopped-database measurement)"
echo "  downgrade guard:   WB2101 expected"
