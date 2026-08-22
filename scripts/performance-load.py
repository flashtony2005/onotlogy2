#!/usr/bin/env python3
"""Concurrent submit/approve/stream baseline against the real release binary."""

from __future__ import annotations

import argparse
import base64
import json
import os
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import Any


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


class WsClient:
    """Minimal RFC 6455 client; the server sends unmasked text frames."""

    def __init__(self, base: str, token: str):
        url = urllib.parse.urlparse(base)
        self.sock = socket.create_connection((url.hostname, url.port), timeout=10)
        key = base64.b64encode(os.urandom(16)).decode()
        path = f"/api/v1/wb/agent/dsh/stream?token={urllib.parse.quote(token)}"
        self.sock.sendall(
            (
                f"GET {path} HTTP/1.1\r\nHost: {url.netloc}\r\n"
                "Upgrade: websocket\r\nConnection: Upgrade\r\n"
                f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
            ).encode()
        )
        headers = b""
        while b"\r\n\r\n" not in headers:
            chunk = self.sock.recv(4096)
            if not chunk:
                raise RuntimeError("websocket closed during handshake")
            headers += chunk
        if b" 101 " not in headers.split(b"\r\n", 1)[0]:
            raise RuntimeError(f"websocket handshake failed: {headers[:200]!r}")
        self.events: dict[str, int] = {}

    def _recv_exact(self, size: int) -> bytes:
        data = bytearray()
        while len(data) < size:
            chunk = self.sock.recv(size - len(data))
            if not chunk:
                raise EOFError
            data.extend(chunk)
        return bytes(data)

    def read_once(self, timeout: float) -> dict[str, Any] | None:
        self.sock.settimeout(timeout)
        first, second = self._recv_exact(2)
        opcode = first & 0x0F
        length = second & 0x7F
        if length == 126:
            length = struct.unpack("!H", self._recv_exact(2))[0]
        elif length == 127:
            length = struct.unpack("!Q", self._recv_exact(8))[0]
        mask = self._recv_exact(4) if second & 0x80 else b""
        payload = bytearray(self._recv_exact(length))
        if mask:
            for index in range(length):
                payload[index] ^= mask[index % 4]
        if opcode == 8:
            raise EOFError
        if opcode != 1:
            return None
        return json.loads(payload.decode())

    def drain(self, seconds: float, stop: threading.Event) -> None:
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline and not stop.is_set():
            try:
                event = self.read_once(max(0.05, deadline - time.monotonic()))
            except (socket.timeout, TimeoutError):
                continue
            except EOFError:
                break
            if event and (kind := event.get("eventType")):
                self.events[kind] = self.events.get(kind, 0) + 1

    def close(self) -> None:
        self.sock.close()


def http_json(
    base: str,
    path: str,
    token: str | None = None,
    payload: dict[str, Any] | None = None,
    timeout: float = 20,
) -> dict[str, Any]:
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    data = None if payload is None else json.dumps(payload).encode()
    request = urllib.request.Request(base + path, data=data, headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            body = json.load(response)
    except urllib.error.HTTPError as error:
        detail = error.read().decode(errors="replace")
        raise RuntimeError(f"{path}: HTTP {error.code}: {detail}") from error
    if body.get("code") != 0:
        raise RuntimeError(f"{path}: {body}")
    return body.get("data", {})


def percentile(values: list[float], percent: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    rank = max(0, min(len(ordered) - 1, round((percent / 100) * len(ordered) + 0.5) - 1))
    return ordered[rank]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--odl", required=True)
    parser.add_argument("--output")
    parser.add_argument("--requests", type=int, default=10)
    parser.add_argument("--concurrency", type=int, default=5)
    parser.add_argument("--ws-clients", type=int, default=3)
    args = parser.parse_args()

    root = Path(__file__).resolve().parents[1]
    binary = Path(args.binary).resolve()
    odl = Path(args.odl).resolve()
    work = Path(tempfile.mkdtemp(prefix="oaip-load."))
    database = work / "load.db"
    workspace = work / "workspace"
    workspace.mkdir()
    port = free_port()
    base = f"http://127.0.0.1:{port}"

    environment = os.environ.copy()
    environment.update(
        WB_BOOTSTRAP_ADMIN_USER="load-admin",
        WB_BOOTSTRAP_ADMIN_PWD="load-password-change-me",
    )
    process = subprocess.Popen(
        [
            str(binary),
            "--db",
            str(database),
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            str(port),
            "--static-dir",
            str(root / "ui/dist"),
            "--odl",
            str(odl),
        ],
        cwd=root,
        env=environment,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    clients: list[WsClient] = []
    stop = threading.Event()
    threads: list[threading.Thread] = []
    result: dict[str, Any]
    try:
        for _ in range(100):
            if process.poll() is not None:
                raise RuntimeError(f"server exited early: {process.poll()}")
            try:
                with urllib.request.urlopen(f"{base}/readyz", timeout=1) as response:
                    if response.status == 200:
                        break
            except urllib.error.URLError:
                time.sleep(0.1)
        else:
            raise RuntimeError("server did not become ready")

        token = http_json(
            base,
            "/api/v1/wb/auth/login",
            payload={"username": "load-admin", "password": "load-password-change-me"},
        )["token"]
        for _ in range(args.ws_clients):
            clients.append(WsClient(base, token))

        def listen(client: WsClient) -> None:
            client.drain(30, stop)

        for client in clients:
            thread = threading.Thread(target=listen, args=(client,), daemon=True)
            thread.start()
            threads.append(thread)

        def submit_and_approve(index: int) -> tuple[bool, float]:
            started = time.perf_counter()
            submitted = http_json(
                base,
                "/api/v1/facade/fabric/submit",
                token,
                {
                    "sessionId": f"load-{index}",
                    "sender": "dsh",
                    "intent": "run_shell",
                    "payload": {
                        "object": "Customer:load-customer",
                        "args": ["-c", "printf LOAD_OK"],
                    },
                    "executionContext": {
                        "workspace": {"root": str(workspace), "mode": "isolated"}
                    },
                },
            )["result"]
            if submitted.get("payload", {}).get("status") != "pending":
                raise RuntimeError(f"submission was not governed: {submitted}")
            run_id = submitted["correlation_id"]
            resolved = http_json(
                base,
                f"/api/v1/wb/agent/run/{run_id}/approval/{run_id}",
                token,
                {"decision": "approve"},
            )
            if resolved.get("status") != "completed":
                raise RuntimeError(f"approval did not complete: {resolved}")
            return True, time.perf_counter() - started

        started = time.perf_counter()
        errors: list[str] = []
        latencies: list[float] = []
        with ThreadPoolExecutor(max_workers=args.concurrency) as executor:
            futures = [executor.submit(submit_and_approve, index) for index in range(args.requests)]
            for future in futures:
                try:
                    _, latency = future.result()
                    latencies.append(latency)
                except Exception as error:  # noqa: BLE001 - report every load failure
                    errors.append(str(error))
        elapsed = time.perf_counter() - started
        stop.set()
        for thread in threads:
            thread.join(timeout=1)

        combined_events: dict[str, int] = {}
        for client in clients:
            for kind, count in client.events.items():
                combined_events[kind] = combined_events.get(kind, 0) + count

        result = {
            "schema_version": 1,
            "scenario": "concurrent_fabric_submit_approve_stream",
            "requests": args.requests,
            "concurrency": args.concurrency,
            "ws_clients": args.ws_clients,
            "successful_requests": len(latencies),
            "failed_requests": len(errors),
            "errors": errors[:10],
            "wall_time_seconds": round(elapsed, 6),
            "throughput_rps": round(len(latencies) / elapsed, 3),
            "latency_ms": {
                "p50": round(percentile(latencies, 50) * 1000, 3),
                "p95": round(percentile(latencies, 95) * 1000, 3),
                "p99": round(percentile(latencies, 99) * 1000, 3),
            },
            "ws_event_counts": dict(sorted(combined_events.items())),
        }
        if errors or len(latencies) != args.requests:
            raise RuntimeError(json.dumps(result, ensure_ascii=False))
        required = {"fabric.approval_requested", "fabric.task_completed"}
        if not required.issubset(combined_events):
            raise RuntimeError(f"websocket missed events: {combined_events}")
        print(json.dumps(result, ensure_ascii=False, indent=2))
        if args.output:
            Path(args.output).write_text(json.dumps(result, ensure_ascii=False) + "\n")
        return 0
    finally:
        stop.set()
        for client in clients:
            client.close()
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        if process.stdout:
            output = process.stdout.read()
            if process.returncode not in (0, -15):
                sys.stderr.write(output)


if __name__ == "__main__":
    raise SystemExit(main())
