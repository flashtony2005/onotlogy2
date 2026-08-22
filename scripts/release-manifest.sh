#!/usr/bin/env bash
set -euo pipefail

# Generate a release evidence manifest without mutating the repository.  A
# dirty worktree is deliberately represented as a candidate, never as a
# release, so this command cannot silently bless an uncommitted deployment.
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${OAIP_BIN:-${ROOT_DIR}/target/release/oaip}"
OUT="${OAIP_RELEASE_MANIFEST:-${ROOT_DIR}/target/release-manifest.json}"
IMAGE="${OAIP_IMAGE:-openaip-os:0.1.0}"
TAG="${OAIP_RELEASE_TAG:-}"

if [[ ! -x "${BIN}" ]]; then
  echo "release binary is not executable: ${BIN}" >&2
  echo "run: cargo build --release --bin oaip" >&2
  exit 2
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "python3 is required to write the release manifest" >&2
  exit 2
fi

export OAIP_MANIFEST_ROOT="${ROOT_DIR}"
export OAIP_MANIFEST_BIN="${BIN}"
export OAIP_MANIFEST_OUT="${OUT}"
export OAIP_MANIFEST_IMAGE="${IMAGE}"
export OAIP_MANIFEST_TAG="${TAG}"

python3 <<'PY'
import hashlib
import json
import os
import re
import subprocess
from datetime import datetime, timezone
from pathlib import Path

root = Path(os.environ["OAIP_MANIFEST_ROOT"])
binary = Path(os.environ["OAIP_MANIFEST_BIN"])
output = Path(os.environ["OAIP_MANIFEST_OUT"])

def run(*args: str) -> str:
    return subprocess.check_output(args, cwd=root, text=True).strip()

version = run(str(binary), "--version").split()[1]
schema_text = (root / "crates/oaip-storage/src/schema.rs").read_text()
schema_match = re.search(r"SCHEMA_VERSION: i64 = (\d+)", schema_text)
if schema_match is None:
    raise SystemExit("cannot determine SCHEMA_VERSION from crates/oaip-storage/src/schema.rs")

def git_dirty() -> bool:
    return bool(run("git", "status", "--porcelain"))

dirty = git_dirty()
tag = os.environ["OAIP_MANIFEST_TAG"] or None
commit = run("git", "rev-parse", "HEAD")
try:
    commit_time = run("git", "show", "-s", "--format=%cI", "HEAD")
except subprocess.CalledProcessError:
    commit_time = None

digest = hashlib.sha256(binary.read_bytes()).hexdigest()
manifest = {
    "format": "openaip.release-manifest.v1",
    "status": "release" if not dirty and tag else "candidate",
    "version": version,
    "release_tag": tag,
    "git": {
        "commit": commit,
        "commit_time": commit_time,
        "worktree_dirty": dirty,
    },
    "toolchain": "nightly-2026-07-17",
    "schema_version": int(schema_match.group(1)),
    "artifact": {
        "name": "oaip",
        "path": str(binary.relative_to(root)) if binary.is_relative_to(root) else str(binary),
        "size_bytes": binary.stat().st_size,
        "sha256": digest,
    },
    "container": {
        "image": os.environ["OAIP_MANIFEST_IMAGE"],
        "dockerfile": "Dockerfile",
        "compose_file": "docker-compose.yml",
    },
    "generated_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
}

output.parent.mkdir(parents=True, exist_ok=True)
output.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
print(f"release manifest written: {output}")
print(f"status={manifest['status']} version={version} schema={manifest['schema_version']}")
print(f"binary_sha256={digest}")
PY
