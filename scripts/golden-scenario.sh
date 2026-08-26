#!/usr/bin/env bash
# 黄金场景回归：ODL → dsh/process → 审批 → 审计/世界/WS → 演进 → 重启恢复。
#
# 真实执行器约定：核心黄金场景默认使用确定性 /bin/sh process executor，
# 不依赖本机 dsh 登录态；如需验证特定外部 agent，显式设置 OAIP_DSH_COMMAND。
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
OAIP_BIN=${OAIP_BIN:-"$ROOT/target/debug/oaip"}
TMP=$(mktemp -d "${TMPDIR:-/tmp}/oaip-golden.XXXXXX")
DB="$TMP/world.db"
ODL_V1="$TMP/golden-v1.odl"
ODL_V2="$TMP/golden-v2.odl"
SERVER_LOG="$TMP/server.log"
MARKER="$TMP/dsh-marker"
OBJECT_REF="Project:golden-project"
PORT=${OAIP_GOLDEN_PORT:-$((18000 + ($$ % 1000)))}
BASE="http://127.0.0.1:$PORT"
SERVER_PID=""
TOKEN=""

cleanup() {
  if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill -TERM "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  rm -rf "$TMP"
}
trap cleanup EXIT

if [[ ! -x "$OAIP_BIN" ]]; then
  echo "golden: binary not found: $OAIP_BIN" >&2
  echo "golden: run 'cargo build --bin oaip' first" >&2
  exit 1
fi

if [[ -z "${OAIP_DSH_COMMAND:-}" ]]; then
  echo "golden: using deterministic process executor; set OAIP_DSH_COMMAND to test an external agent" >&2
  DSH_SHIM="$TMP/process-executor"
  cat >"$DSH_SHIM" <<'SHIM'
#!/bin/sh
set -eu

if [ "$#" -lt 3 ] || [ "$1" != "--profile" ] || [ "$2" != "headless" ]; then
  echo "dsh shim: expected --profile headless <task>" >&2
  exit 64
fi
shift 2

# The golden scenario deliberately uses a natural-language task so the shim
# exercises the same final command boundary as dsh without pretending to be an
# LLM. Extract only the command inside the explicit golden contract.
if [ "$#" -ne 1 ]; then
  echo "dsh shim: expected exactly one task argument" >&2
  exit 64
fi
task=$1
prefix='Run exactly this shell command: '
suffix='; then report completion.'
case "$task" in
  "$prefix"*"$suffix")
    command=${task#"$prefix"}
    command=${command%"$suffix"}
    if [ -z "$command" ]; then
      echo "dsh shim: empty shell command" >&2
      exit 64
    fi
    exec /bin/sh -c "$command"
    ;;
  *)
    echo "dsh shim: unsupported task contract" >&2
    exit 64
    ;;
esac
SHIM
  chmod +x "$DSH_SHIM"
  export OAIP_DSH_COMMAND="$DSH_SHIM"
else
  echo "golden: using explicit external executor" >&2
fi

cat >"$ODL_V1" <<'ODL'
# Golden domain: Project -> Task -> AgentRun.
object Project:
  properties:
    name: string
    status: string
  actions:
    create_task:
      creates:
        - type Task
          data:
            title: "Golden task"
            status: "Pending"
            project: self
          link: has_task

object Task:
  properties:
    title: string
    status: string
    project: string
  actions:
    complete:
      updates:
        - field: status
          value: "Completed"
      policy:
        precondition:
          - field: self.status
            op: eq
            value: "Pending"

object AgentRun:
  properties:
    goal: string
    status: string
ODL

cat >"$ODL_V2" <<'ODL'
# Golden domain v2: add Project.owner and a second completion constraint.
object Project:
  properties:
    name: string
    status: string
    owner: string
  actions:
    create_task:
      creates:
        - type Task
          data:
            title: "Golden task"
            status: "Pending"
            project: self
          link: has_task

object Task:
  properties:
    title: string
    status: string
    project: string
  actions:
    complete:
      updates:
        - field: status
          value: "Completed"
      policy:
        precondition:
          - field: self.status
            op: eq
            value: "Pending"
          - field: self.status
            op: ne
            value: "Blocked"

object AgentRun:
  properties:
    goal: string
    status: string
ODL

start_server() {
  env WB_BOOTSTRAP_ADMIN_USER=admin WB_BOOTSTRAP_ADMIN_PWD=admin-pass \
    "$OAIP_BIN" --db "$DB" serve --host 127.0.0.1 --port "$PORT" --odl "$1" \
    >"$SERVER_LOG" 2>&1 &
  SERVER_PID=$!
  for _ in $(seq 1 100); do
    if curl -fsS "$BASE/" >/dev/null 2>&1; then
      break
    fi
    sleep 0.1
  done
  if ! kill -0 "$SERVER_PID" 2>/dev/null; then
    cat "$SERVER_LOG" >&2
    return 1
  fi
  TOKEN=$(python3 - "$BASE" <<'PY'
import json, sys, urllib.request
base = sys.argv[1]
req = urllib.request.Request(
    base + "/api/v1/wb/auth/login",
    data=json.dumps({"username": "admin", "password": "admin-pass"}).encode(),
    headers={"Content-Type": "application/json"},
    method="POST",
)
with urllib.request.urlopen(req, timeout=5) as r:
    body = json.load(r)
if body.get("code") != 0:
    raise SystemExit(f"login failed: {body}")
print(body["data"]["token"])
PY
)
}

stop_server() {
  if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill -TERM "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  SERVER_PID=""
}

check_views() {
  python3 - "$BASE" "$TOKEN" "$OBJECT_REF" <<'PY'
import json, sys, urllib.request
base, token, object_ref = sys.argv[1:]

def post(path, payload):
    req = urllib.request.Request(
        base + path,
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json", "Authorization": "Bearer " + token},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=10) as r:
        body = json.load(r)
    if body.get("code") != 0:
        raise SystemExit(f"{path} failed: {body}")
    return body["data"]

world = post("/api/v1/facade/world/query", {
    "objectRef": object_ref, "predicate": "fabric.status", "limit": 100,
})
if not any(r.get("target") == "completed" for r in world.get("relations", [])):
    raise SystemExit(f"world projection missing completed: {world}")
audit = post("/api/v1/facade/audit/query", {"eventType": "fabric.", "limit": 200})
types = {e.get("eventType") for e in audit.get("events", [])}
for required in ("fabric.task_requested", "fabric.task_completed"):
    if required not in types:
        raise SystemExit(f"audit missing {required}: {types}")
print(f"views: world completed; audit events={audit.get('total', 0)}")
PY
}

run_dsh() {
  local label=$1
  local task="Run exactly this shell command: printf DSH_${label}_OK > $MARKER; then report completion."
  "$OAIP_BIN" --db "$DB" facade fabric submit \
    --session "golden-$label" --sender dsh --intent agent_task \
    --payload "$(python3 -c 'import json,sys; print(json.dumps({"object":sys.argv[1],"task":sys.argv[2]}))' "$OBJECT_REF" "$task")" \
    --execution-context "$(python3 -c 'import json,os,sys; print(json.dumps({"workspace":{"root":sys.argv[1],"mode":"isolated"},"env":{"HOME":os.environ.get("HOME","/"),"PATH":os.environ.get("PATH","")},"timeout_ms":60000}))' "$TMP")" \
    --timeout 60
  test "$(cat "$MARKER")" = "DSH_${label}_OK"
}

run_approved_shell() {
  python3 - "$BASE" "$TOKEN" "$OBJECT_REF" "$TMP" <<'PY'
import base64, json, os, socket, struct, sys, time, urllib.parse, urllib.request
base, token, object_ref, workspace_root = sys.argv[1:]
u = urllib.parse.urlparse(base)

# Minimal stdlib WebSocket client: the server sends unmasked text frames.
s = socket.create_connection((u.hostname, u.port), timeout=10)
key = base64.b64encode(os.urandom(16)).decode()
path = "/api/v1/wb/agent/dsh/stream?token=" + urllib.parse.quote(token)
s.sendall((
    f"GET {path} HTTP/1.1\r\nHost: {u.netloc}\r\n"
    f"Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\n"
    "Sec-WebSocket-Version: 13\r\n\r\n"
).encode())
headers = b""
while b"\r\n\r\n" not in headers:
    chunk = s.recv(4096)
    if not chunk:
        raise SystemExit("websocket closed during handshake")
    headers += chunk
if b" 101 " not in headers.split(b"\r\n", 1)[0]:
    raise SystemExit(f"websocket handshake failed: {headers[:200]!r}")


def recv_exact(n):
    out = b""
    while len(out) < n:
        chunk = s.recv(n - len(out))
        if not chunk:
            raise EOFError
        out += chunk
    return out


def recv_text():
    first, second = recv_exact(2)
    opcode = first & 0x0f
    length = second & 0x7f
    if length == 126:
        length = struct.unpack("!H", recv_exact(2))[0]
    elif length == 127:
        length = struct.unpack("!Q", recv_exact(8))[0]
    masked = second & 0x80
    mask = recv_exact(4) if masked else b""
    payload = bytearray(recv_exact(length))
    if masked:
        for i in range(length):
            payload[i] ^= mask[i % 4]
    if opcode == 8:
        raise EOFError
    if opcode != 1:
        return None
    return json.loads(payload.decode())


def post(path, payload):
    req = urllib.request.Request(
        base + path,
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json", "Authorization": "Bearer " + token},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=15) as r:
        body = json.load(r)
    if body.get("code") != 0:
        raise SystemExit(f"{path} failed: {body}")
    return body["data"]

submitted = post("/api/v1/facade/fabric/submit", {
    "sessionId": "golden-approval",
    "sender": "dsh",
    "intent": "run_shell",
    "payload": {"object": object_ref, "args": ["-c", "printf APPROVED_OK"]},
    "executionContext": {"workspace": {"root": workspace_root, "mode": "isolated"}},
})
result = submitted["result"]
if result["payload"].get("status") != "pending":
    raise SystemExit(f"high-risk submission did not suspend: {submitted}")
run_id = result["correlation_id"]
resolved = post(f"/api/v1/wb/agent/run/{run_id}/approval/{run_id}", {"decision": "approve"})
if resolved.get("status") != "completed" or resolved.get("sender") != "shell":
    raise SystemExit(f"approval did not complete through shell executor: {resolved}")

seen = set()
deadline = time.time() + 10
while time.time() < deadline:
    s.settimeout(max(0.1, deadline - time.time()))
    try:
        event = recv_text()
    except (socket.timeout, EOFError):
        break
    if event:
        seen.add(event.get("eventType"))
        if event.get("eventType") == "fabric.task_completed":
            break
s.close()
for required in ("fabric.approval_requested", "fabric.task_completed"):
    if required not in seen:
        raise SystemExit(f"websocket missing {required}: {sorted(seen)}")
print(f"approval: completed by {resolved['sender']}; ws events={sorted(seen)}")
PY
}

# GS-1: compile an actual three-object ODL with an action, relation effect, and constraint.
(cd "$TMP" && "$OAIP_BIN" compile "$ODL_V1")
test -s "$TMP/gen/ontology.json"
echo "GS-1 PASS: ODL v1 compiled"

start_server "$ODL_V1"
run_dsh "V1"
check_views
echo "GS-2 PASS: dsh real process completed a world-affecting task"

run_approved_shell
check_views
echo "GS-3 PASS: approval, append-only audit, world query, and WebSocket all observed"

# GS-4: evolve the ODL, rebuild, restart against the same database, and rerun the chain.
stop_server
(cd "$TMP" && "$OAIP_BIN" compile "$ODL_V2")
test -s "$TMP/gen/ontology.json"
start_server "$ODL_V2"
run_dsh "V2"
run_approved_shell
check_views
echo "GS-4 PASS: evolved ODL compiled and the execution/governance chain still works"

# GS-5: persist a pending fabric approval, restart the process, rehydrate the
# session/approval projection, then approve and execute the original request.
stop_server
start_server "$ODL_V2"
python3 - "$BASE" "$TOKEN" "$OBJECT_REF" "$MARKER" "$TMP" <<'PY'
import json, sys, time, urllib.request
base, token, object_ref, marker, workspace_root = sys.argv[1:]
headers = {"Content-Type": "application/json", "Authorization": "Bearer " + token}

def call(method, path, payload=None):
    data = None if payload is None else json.dumps(payload).encode()
    req = urllib.request.Request(base + path, data=data, headers=headers, method=method)
    with urllib.request.urlopen(req, timeout=15) as r:
        body = json.load(r)
    if body.get("code") != 0:
        raise SystemExit(f"{path} failed: {body}")
    return body["data"]

submitted = call("POST", "/api/v1/facade/fabric/submit", {
    "sessionId": "golden-restart-approval",
    "sender": "dsh",
    "intent": "run_shell",
    "payload": {
        "object": object_ref,
        "command": f"printf RESTART_APPROVED_OK > {marker}",
    },
    "executionContext": {"workspace": {"root": workspace_root, "mode": "isolated"}},
})
result = submitted["result"]
if result["payload"].get("status") != "pending":
    raise SystemExit(f"restart probe did not suspend: {submitted}")
run_id = result["correlation_id"]

# Kill/restart is the boundary under test; the same DB must recreate the pending entry.
# The shell parent owns the process, so terminate it through the exported marker file.
print(f"GS-5 pending before restart: {run_id}")
PY
stop_server
start_server "$ODL_V2"
python3 - "$BASE" "$TOKEN" "$OBJECT_REF" "$MARKER" <<'PY'
import json, sys, urllib.request
base, token, object_ref, marker = sys.argv[1:]
# The run id is recovered from the durable audit stream, not a process-local file.
headers = {"Content-Type": "application/json", "Authorization": "Bearer " + token}

def post(path, payload):
    req = urllib.request.Request(base + path, data=json.dumps(payload).encode(), headers=headers, method="POST")
    with urllib.request.urlopen(req, timeout=15) as r:
        body = json.load(r)
    if body.get("code") != 0:
        raise SystemExit(f"{path} failed: {body}")
    return body["data"]

def get(path):
    req = urllib.request.Request(base + path, headers={"Authorization": "Bearer " + token}, method="GET")
    with urllib.request.urlopen(req, timeout=15) as r:
        body = json.load(r)
    if body.get("code") != 0:
        raise SystemExit(f"{path} failed: {body}")
    return body["data"]

audit = post("/api/v1/facade/audit/query", {"eventType": "fabric.approval_requested", "limit": 100})
rows = [e for e in audit["events"] if e.get("payload", {}).get("session_id") == "golden-restart-approval"]
if len(rows) != 1:
    raise SystemExit(f"restart approval event missing: {audit}")
run_id = rows[0]["payload"]["run_id"]
approvals = get(f"/api/v1/wb/agent/run/{run_id}/approvals")
if not any(x.get("reqId") == run_id and x.get("state") == "Pending" for x in approvals):
    raise SystemExit(f"pending approval was not rehydrated: {approvals}")
resolved = post(f"/api/v1/wb/agent/run/{run_id}/approval/{run_id}", {"decision": "approve"})
if resolved.get("status") != "completed":
    raise SystemExit(f"rehydrated approval did not complete: {resolved}")
print("GS-5 PASS: pending fabric approval rehydrated across restart and completed")
PY
test "$(cat "$MARKER")" = "RESTART_APPROVED_OK"
check_views
echo "golden scenario completed through GS-1..GS-5"
