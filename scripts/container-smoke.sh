#!/usr/bin/env bash
set -euo pipefail

# S3a/S3b container smoke: build the production Dockerfile, start the real
# image, probe the public operational endpoints, then stop and remove it.
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${OAIP_IMAGE:-openaip-os:0.1.0}"
PORT="${OAIP_CONTAINER_SMOKE_PORT:-18081}"
NAME="oaip-container-smoke-$$"
VOLUME="oaip-container-smoke-$$"
CONTAINER=""

cleanup() {
  if [[ -n "${CONTAINER}" ]]; then
    docker rm --force "${CONTAINER}" >/dev/null 2>&1 || true
  fi
  docker volume rm "${VOLUME}" >/dev/null 2>&1 || true
}
trap cleanup EXIT

if ! command -v docker >/dev/null 2>&1; then
  echo "Docker is required for container smoke; install/start Docker and retry" >&2
  exit 2
fi
if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required for container smoke; install it and retry" >&2
  exit 2
fi
if ! docker info >/dev/null 2>&1; then
  echo "Docker daemon is unavailable; start Docker and retry" >&2
  exit 2
fi

docker build --tag "${IMAGE}" "${ROOT_DIR}"
docker volume create "${VOLUME}" >/dev/null
CONTAINER="${NAME}"
docker run --detach \
  --name "${CONTAINER}" \
  --publish "127.0.0.1:${PORT}:8080" \
  --mount "type=volume,source=${VOLUME},destination=/var/lib/oaip" \
  --env WB_BOOTSTRAP_ADMIN_USER=container-smoke-admin \
  --env WB_BOOTSTRAP_ADMIN_PWD=container-smoke-password \
  "${IMAGE}" >/dev/null

runtime_user="$(docker inspect --format '{{.Config.User}}' "${CONTAINER}")"
if [[ "${runtime_user}" != "oaip" ]]; then
  echo "container must run as non-root user oaip, got: ${runtime_user:-empty}" >&2
  exit 1
fi
mounted_volume="$(docker inspect --format '{{range .Mounts}}{{if eq .Destination "/var/lib/oaip"}}{{.Name}}{{end}}{{end}}' "${CONTAINER}")"
if [[ "${mounted_volume}" != "${VOLUME}" ]]; then
  echo "container did not mount the expected persistent volume: ${mounted_volume:-empty}" >&2
  exit 1
fi

ready=0
for _ in $(seq 1 60); do
  if curl --fail --silent "http://127.0.0.1:${PORT}/readyz" >/dev/null; then
    ready=1
    break
  fi
  sleep 1
done
if [[ "${ready}" -ne 1 ]]; then
  echo "container did not become ready within 60 seconds" >&2
  docker logs "${CONTAINER}" >&2 || true
  exit 1
fi

curl --fail --silent "http://127.0.0.1:${PORT}/healthz" | grep --fixed-strings --quiet '"status":"ok"'
curl --fail --silent "http://127.0.0.1:${PORT}/readyz" | grep --fixed-strings --quiet '"status":"ready"'
curl --fail --silent "http://127.0.0.1:${PORT}/metrics" | grep --fixed-strings --quiet 'oaip_ready 1'

health=""
for _ in $(seq 1 30); do
  health="$(docker inspect --format '{{.State.Health.Status}}' "${CONTAINER}" 2>/dev/null || true)"
  if [[ "${health}" == "healthy" ]]; then
    break
  fi
  sleep 1
done
if [[ "${health}" != "healthy" ]]; then
  echo "container did not report healthy status: ${health:-unknown}" >&2
  docker logs "${CONTAINER}" >&2 || true
  exit 1
fi

docker stop --time 15 "${CONTAINER}" >/dev/null
echo "container smoke test passed: ${IMAGE}"
