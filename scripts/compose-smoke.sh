#!/usr/bin/env bash
set -euo pipefail

# S3a/S3b Compose smoke: validate the real Compose file, build the image,
# start the service with a temporary project/volume, probe it, and stop it.
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${OAIP_COMPOSE_SMOKE_PORT:-18082}"
PROJECT="oaip-compose-smoke-$$"
WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/oaip-compose-smoke.XXXXXX")"
ENV_FILE="${WORK_DIR}/.env"
COMPOSE_ACTIVE=0

cleanup() {
  if [[ "${COMPOSE_ACTIVE}" -eq 1 ]]; then
    (
      cd "${ROOT_DIR}"
      docker compose \
        --project-name "${PROJECT}" \
        --env-file "${ENV_FILE}" \
        down --volumes --remove-orphans
    ) >/dev/null 2>&1 || true
  fi
  rm -rf "${WORK_DIR}"
}
trap cleanup EXIT

if ! command -v docker >/dev/null 2>&1; then
  echo "Docker is required for Compose smoke; install/start Docker and retry" >&2
  exit 2
fi
if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required for Compose smoke; install it and retry" >&2
  exit 2
fi
if ! docker info >/dev/null 2>&1; then
  echo "Docker daemon is unavailable; start Docker and retry" >&2
  exit 2
fi
if ! docker compose version >/dev/null 2>&1; then
  echo "Docker Compose v2 is required for Compose smoke; install it and retry" >&2
  exit 2
fi

cat >"${ENV_FILE}" <<ENV
OAIP_HTTP_BIND=127.0.0.1
OAIP_HTTP_PORT=${PORT}
WB_BOOTSTRAP_ADMIN_USER=compose-smoke-admin
WB_BOOTSTRAP_ADMIN_PWD=compose-smoke-password
ENV

compose() {
  docker compose \
    --project-name "${PROJECT}" \
    --env-file "${ENV_FILE}" \
    "$@"
}

COMPOSE_ACTIVE=1
(
  cd "${ROOT_DIR}"
  compose config --quiet
  compose up --build --detach
)

ready=0
for _ in $(seq 1 60); do
  if curl --connect-timeout 1 --max-time 5 --fail --silent \
    "http://127.0.0.1:${PORT}/readyz" >/dev/null; then
    ready=1
    break
  fi
  sleep 1
done
if [[ "${ready}" -ne 1 ]]; then
  echo "Compose service did not become ready within 60 seconds" >&2
  (cd "${ROOT_DIR}" && compose logs --no-color) >&2 || true
  exit 1
fi

curl --connect-timeout 1 --max-time 5 --fail --silent \
  "http://127.0.0.1:${PORT}/healthz" \
  | grep --fixed-strings --quiet '"status":"ok"'
curl --connect-timeout 1 --max-time 5 --fail --silent \
  "http://127.0.0.1:${PORT}/readyz" \
  | grep --fixed-strings --quiet '"status":"ready"'
curl --connect-timeout 1 --max-time 5 --fail --silent \
  "http://127.0.0.1:${PORT}/metrics" \
  | grep --fixed-strings --quiet 'oaip_ready 1'

container_id="$(cd "${ROOT_DIR}" && compose ps -q app)"
runtime_user="$(docker inspect --format '{{.Config.User}}' "${container_id}")"
if [[ "${runtime_user}" != "oaip" ]]; then
  echo "Compose container must run as non-root user oaip, got: ${runtime_user:-empty}" >&2
  exit 1
fi
mount_type="$(docker inspect --format '{{range .Mounts}}{{if eq .Destination "/var/lib/oaip"}}{{.Type}}{{end}}{{end}}' "${container_id}")"
if [[ "${mount_type}" != "volume" ]]; then
  echo "Compose container did not mount /var/lib/oaip as a named volume: ${mount_type:-empty}" >&2
  exit 1
fi
health=""
for _ in $(seq 1 30); do
  health="$(docker inspect --format '{{.State.Health.Status}}' "${container_id}" 2>/dev/null || true)"
  if [[ "${health}" == "healthy" ]]; then
    break
  fi
  sleep 1
done
if [[ "${health}" != "healthy" ]]; then
  echo "Compose container did not report healthy status: ${health:-unknown}" >&2
  (cd "${ROOT_DIR}" && compose logs --no-color) >&2 || true
  exit 1
fi

(cd "${ROOT_DIR}" && compose stop --timeout 15) >/dev/null
echo "Compose smoke test passed: project=${PROJECT} port=${PORT}"
