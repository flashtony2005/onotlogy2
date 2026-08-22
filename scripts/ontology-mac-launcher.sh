#!/usr/bin/env bash
set -euo pipefail

# Ontology Mac 本地运行壳：只使用本地 libSQL/SQLite 文件，不连接 Turso 云端。
CONTENTS_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RESOURCES="${CONTENTS_ROOT}/Resources"
BIN="${RESOURCES}/oaip"
DATA_DIR="${ONTOLOGY_DATA_DIR:-${HOME}/Library/Application Support/Ontology}"
DB="${ONTOLOGY_DB:-${DATA_DIR}/ontology.db}"
HOST="${ONTOLOGY_HOST:-127.0.0.1}"
PORT="${ONTOLOGY_PORT:-18420}"
DOMAIN_PACK="${ONTOLOGY_DOMAIN_PACK:-}"
DOMAIN_PACKS_DIR="${ONTOLOGY_DOMAIN_PACKS_DIR:-${RESOURCES}/domain-packs}"
LOG_DIR="${DATA_DIR}/logs"
PID_FILE="${DATA_DIR}/ontology.pid"
CREDENTIALS_FILE="${DATA_DIR}/first-run-credentials.txt"

mkdir -p "${DATA_DIR}" "${LOG_DIR}"
chmod 700 "${DATA_DIR}"

if [[ ! -x "${BIN}" ]]; then
    echo "Ontology runtime not found: ${BIN}" >&2
    exit 1
fi
if [[ ! -f "${RESOURCES}/ui-dist/index.html" ]]; then
    echo "Ontology UI not found: ${RESOURCES}/ui-dist/index.html" >&2
    exit 1
fi

# Mac 本地版明确禁止通过环境变量误接入 Turso/libSQL 远程地址。
unset TURSO_AUTH_TOKEN || true
unset TURSO_DATABASE_URL || true
unset LIBSQL_URL || true

if [[ -n "${DOMAIN_PACK}" && "${DOMAIN_PACK}" != /* ]]; then
    DOMAIN_PACK="${RESOURCES}/${DOMAIN_PACK}"
fi
if [[ -n "${DOMAIN_PACK}" && ! -d "${DOMAIN_PACK}" ]]; then
    echo "Ontology domain pack not found: ${DOMAIN_PACK}" >&2
    exit 1
fi

if [[ -f "${PID_FILE}" ]]; then
    OLD_PID="$(cat "${PID_FILE}" 2>/dev/null || true)"
    if [[ -n "${OLD_PID}" ]] && kill -0 "${OLD_PID}" 2>/dev/null; then
        open "http://${HOST}:${PORT}/"
        exit 0
    fi
    rm -f "${PID_FILE}"
fi

# 空库首启时生成一次本地管理员凭据；不会把固定默认密码写进程序。
if [[ ! -f "${CREDENTIALS_FILE}" ]]; then
    PASSWORD="$(/usr/bin/uuidgen | tr -d '-')"
    umask 077
    {
        echo "username=admin"
        echo "password=${PASSWORD}"
        echo "created_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
        echo "请首次登录后立即修改密码。"
    } > "${CREDENTIALS_FILE}"
else
    PASSWORD="$(/usr/bin/grep '^password=' "${CREDENTIALS_FILE}" | /usr/bin/cut -d= -f2-)"
fi

export WB_BOOTSTRAP_ADMIN_USER="admin"
export WB_BOOTSTRAP_ADMIN_PWD="${PASSWORD}"
# 业务包管理 API 从 App Resources 发现所有业务包，而不是依赖启动时选中的单个包。
export ONTOLOGY_DOMAIN_PACKS_DIR="${DOMAIN_PACKS_DIR}"

# 将 PID 写入数据目录，便于第二次从 Finder 启动时复用已运行服务。
if [[ -n "${DOMAIN_PACK}" ]]; then
    SEMANTICS_ARGS=(--domain-pack "${DOMAIN_PACK}")
else
    SEMANTICS_ARGS=(--odl "${RESOURCES}/ontology.odl")
fi

(
    exec "${BIN}" --db "${DB}" serve --host "${HOST}" --port "${PORT}" \
        --static-dir "${RESOURCES}/ui-dist" "${SEMANTICS_ARGS[@]}"
) > "${LOG_DIR}/ontology.log" 2>&1 &
SERVER_PID=$!
echo "${SERVER_PID}" > "${PID_FILE}"

cleanup() {
    rm -f "${PID_FILE}"
}
trap cleanup EXIT INT TERM

for _ in $(seq 1 80); do
    if /usr/bin/curl --silent --fail "http://${HOST}:${PORT}/readyz" >/dev/null 2>&1; then
        open "http://${HOST}:${PORT}/"
        wait "${SERVER_PID}"
        exit $?
    fi
    if ! kill -0 "${SERVER_PID}" 2>/dev/null; then
        echo "Ontology failed to start. See ${LOG_DIR}/ontology.log" >&2
        exit 1
    fi
    sleep 0.25
done

echo "Ontology did not become ready. See ${LOG_DIR}/ontology.log" >&2
kill "${SERVER_PID}" 2>/dev/null || true
exit 1
