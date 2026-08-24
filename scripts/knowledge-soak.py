#!/usr/bin/env python3
"""Long-running knowledge reasoning/evolution soak with crash-recovery evidence."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import platform
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


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


def rss_kb(pid: int) -> int | None:
    try:
        for line in Path(f"/proc/{pid}/status").read_text(encoding="utf-8").splitlines():
            if line.startswith("VmRSS:"):
                return int(line.split()[1])
    except (FileNotFoundError, PermissionError, ValueError):
        pass
    try:
        output = subprocess.check_output(["ps", "-o", "rss=", "-p", str(pid)], text=True)
    except (OSError, subprocess.CalledProcessError):
        return None
    try:
        return int(output.strip().splitlines()[0])
    except (IndexError, ValueError):
        return None


def linear_growth_per_hour(samples: list[tuple[float, int]]) -> float | None:
    """Fit post-warmup RSS with ordinary least squares and return KiB/hour."""
    if len(samples) < 2:
        return None
    origin = samples[0][0]
    points = [((started - origin) / 3600, float(value)) for started, value in samples]
    mean_x = sum(x for x, _ in points) / len(points)
    mean_y = sum(y for _, y in points) / len(points)
    variance = sum((x - mean_x) ** 2 for x, _ in points)
    if variance == 0:
        return None
    covariance = sum((x - mean_x) * (y - mean_y) for x, y in points)
    return covariance / variance


def operation_metrics_failed(metrics: Metrics, operation: str) -> int:
    item = metrics.data.get(operation)
    return int(item.get("failed", 0)) if item else 0


class Metrics:
    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.data: dict[str, dict[str, Any]] = {}

    def record(self, operation: str, latency: float, error: str | None = None) -> None:
        with self.lock:
            item = self.data.setdefault(
                operation,
                {"attempts": 0, "successful": 0, "failed": 0, "latencies": [], "errors": []},
            )
            item["attempts"] += 1
            if error is None:
                item["successful"] += 1
                item["latencies"].append(latency)
            else:
                item["failed"] += 1
                if len(item["errors"]) < 10:
                    item["errors"].append(error)

    def snapshot(self) -> dict[str, dict[str, Any]]:
        with self.lock:
            copied = json.loads(json.dumps(self.data))
        for item in copied.values():
            latencies: list[float] = item.pop("latencies")
            item["latency_ms"] = {
                "p50": round(percentile(latencies, 50) * 1000, 3),
                "p95": round(percentile(latencies, 95) * 1000, 3),
                "p99": round(percentile(latencies, 99) * 1000, 3),
            }
        return copied


class Server:
    def __init__(self, binary: Path, database: Path, odl: Path, static_dir: Path) -> None:
        self.binary = binary
        self.database = database
        self.odl = odl
        self.static_dir = static_dir
        self.process: subprocess.Popen[str] | None = None
        self.base: str | None = None

    def start(self) -> tuple[subprocess.Popen[str], str]:
        port = free_port()
        base = f"http://127.0.0.1:{port}"
        process = subprocess.Popen(
            [
                str(self.binary),
                "--db",
                str(self.database),
                "serve",
                "--host",
                "127.0.0.1",
                "--port",
                str(port),
                "--static-dir",
                str(self.static_dir),
                "--odl",
                str(self.odl),
            ],
            cwd=self.static_dir.parents[1],
            env={**os.environ, "WB_BOOTSTRAP_ADMIN_USER": "soak-admin", "WB_BOOTSTRAP_ADMIN_PWD": "soak-password-change-me"},
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            text=True,
        )
        self.process = process
        self.base = base
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError(f"server exited early during start: {process.returncode}")
            try:
                with urllib.request.urlopen(f"{base}/readyz", timeout=1) as response:
                    if response.status == 200:
                        return process, base
            except (urllib.error.URLError, TimeoutError):
                time.sleep(0.05)
        raise RuntimeError("server did not become ready")

    def kill(self) -> float:
        assert self.process is not None
        started = time.perf_counter()
        self.process.send_signal(signal.SIGKILL)
        self.process.wait(timeout=10)
        elapsed = time.perf_counter() - started
        self.process = None
        return elapsed

    def close(self) -> None:
        if self.process is None:
            return
        self.process.terminate()
        try:
            self.process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=5)
        self.process = None


def login(server: Server) -> tuple[str, str]:
    assert server.base is not None
    token = http_json(
        server.base,
        "/api/v1/wb/auth/login",
        payload={"username": "soak-admin", "password": "soak-password-change-me"},
    )["token"]
    return server.base, token


def initialize_knowledge(server: Server, token: str) -> str:
    assert server.base is not None
    axiom = lambda name: {
        "SubClassOf": {"sub": {"Class": name}, "sup": {"Class": "Thing"}}
    }
    version = http_json(
        server.base,
        "/api/v1/knowledge/ontology/init",
        token,
        {
            "id": "soak-root",
            "ontology": {"axioms": [axiom("SoakA"), axiom("SoakB")]},
        },
    )
    return version["id"]


def swap_probe(server: Server, token: str, cycle: int, metrics: Metrics) -> None:
    assert server.base is not None
    started = time.perf_counter()
    error: str | None = None
    try:
        current = http_json(server.base, "/api/v1/knowledge/ontology/current", token)
        inactive, active = ("SoakA", "SoakB") if cycle % 2 == 0 else ("SoakB", "SoakA")
        proposal_id = f"soak-swap-{cycle}"
        target_id = f"soak-version-{cycle}"
        http_json(
            server.base,
            "/api/v1/knowledge/proposals",
            token,
            {
                "sourceVersionId": current["id"],
                "proposal": {
                    "id": proposal_id,
                    "changes": [
                        {
                            "Remove": {
                                "SubClassOf": {
                                    "sub": {"Class": active},
                                    "sup": {"Class": "Thing"},
                                }
                            }
                        },
                        {
                            "Add": {
                                "SubClassOf": {
                                    "sub": {"Class": inactive},
                                    "sup": {"Class": "Thing"},
                                }
                            }
                        },
                    ],
                },
            },
        )
        http_json(
            server.base,
            f"/api/v1/knowledge/proposals/{proposal_id}/review",
            token,
            {"approve": True, "reason": "bounded soak evolution"},
        )
        http_json(
            server.base,
            f"/api/v1/knowledge/proposals/{proposal_id}/apply",
            token,
            {"targetVersionId": target_id},
        )
    except Exception as caught:
        error = str(caught)
        raise
    finally:
        metrics.record("governed_evolution_cycles", time.perf_counter() - started, error)


def reader_worker(
    server: Server,
    token: str,
    stop: threading.Event,
    metrics: Metrics,
) -> None:
    assert server.base is not None
    queries = [
        ("/api/v1/knowledge/reason", {"query": "consistency"}),
        ("/api/v1/knowledge/reason", {"query": "classification"}),
        ("/api/v1/knowledge/diagnosis", None),
    ]
    index = 0
    while not stop.is_set():
        path, payload = queries[index % len(queries)]
        index += 1
        started = time.perf_counter()
        error: str | None = None
        try:
            answer = http_json(server.base, path, token, payload)
            if path.endswith("/reason"):
                if payload == {"query": "consistency"} and answer.get("consistent") is not True:
                    raise RuntimeError(f"inconsistent knowledge base: {answer}")
            elif answer.get("consistent") is not True:
                raise RuntimeError(f"unhealthy knowledge base: {answer}")
        except Exception as caught:
            error = str(caught)
        finally:
            metrics.record("knowledge_reads", time.perf_counter() - started, error)


def memory_summary(
    timeline: list[tuple[float, int]],
    warmup_until: float | None,
    sample_interval: float,
) -> dict[str, Any]:
    values = [value for _, value in timeline]
    post = [sample for sample in timeline if warmup_until is not None and sample[0] >= warmup_until]
    post_values = [value for _, value in post]
    fitted = linear_growth_per_hour(post)
    direct = None
    if len(post) >= 2:
        elapsed_hours = (post[-1][0] - post[0][0]) / 3600
        if elapsed_hours > 0:
            direct = (post_values[-1] - post_values[0]) / elapsed_hours
    return {
        "start_kb": values[0] if values else None,
        "peak_kb": max(values) if values else None,
        "final_kb": values[-1] if values else None,
        "sample_count": len(values),
        "sample_interval_seconds": sample_interval,
        "post_warmup_sample_count": len(post_values),
        "post_warmup_start_kb": post_values[0] if post_values else None,
        "post_warmup_final_kb": post_values[-1] if post_values else None,
        "fitted_growth_kb_per_hour": round(fitted, 3) if fitted is not None else None,
        "endpoint_growth_kb_per_hour": round(direct, 3) if direct is not None else None,
        "timeline_kb": [value for _, value in timeline],
    }


def run(args: argparse.Namespace) -> dict[str, Any]:
    started_at_utc = dt.datetime.now(dt.timezone.utc).isoformat()
    root = Path(__file__).resolve().parents[1]
    binary = Path(args.binary).resolve()
    odl = Path(args.odl).resolve()
    static_dir = root / "ui/dist"
    if not binary.is_file():
        raise RuntimeError(f"binary does not exist: {binary}")
    if not odl.is_file():
        raise RuntimeError(f"ODL does not exist: {odl}")
    if not static_dir.is_dir():
        raise RuntimeError(f"UI assets do not exist: {static_dir}")

    with tempfile.TemporaryDirectory(prefix="oaip-knowledge-soak.") as temporary:
        work = Path(temporary)
        server = Server(binary, work / "soak.db", odl, static_dir)
        started_monotonic = time.monotonic()
        memory: list[tuple[float, int]] = []
        stop_memory = threading.Event()
        memory_thread = threading.Thread(
            target=lambda: sample_memory(server, memory, args.rss_sample_interval, stop_memory),
            name="rss-sampler",
        )

        server.start()
        memory.append((time.monotonic(), rss_kb(server.process.pid) or 0))
        memory_thread.start()
        try:
            base, token = login(server)
            root_id = initialize_knowledge(server, token)
            if args.memory_warmup_seconds > 0:
                time.sleep(args.memory_warmup_seconds)
            warmup_until = time.monotonic()

            metrics = Metrics()
            stop_workers = threading.Event()
            threads = [
                threading.Thread(
                    target=reader_worker,
                    args=(server, token, stop_workers, metrics),
                    name=f"knowledge-reader-{index}",
                )
                for index in range(args.concurrency)
            ]

            def governor() -> None:
                cycle = 0
                next_cycle = time.monotonic() + args.mutation_interval
                while not stop_workers.is_set():
                    delay = next_cycle - time.monotonic()
                    if delay > 0:
                        if stop_workers.wait(delay):
                            break
                    swap_probe(server, token, cycle, metrics)
                    cycle += 1
                    next_cycle += args.mutation_interval

            governor_thread = threading.Thread(target=governor, name="knowledge-governor")
            workload_started = time.perf_counter()
            for thread in threads:
                thread.start()
            governor_thread.start()
            stop_workers.wait(args.duration_seconds)
            stop_workers.set()
            for thread in threads:
                thread.join()
            governor_thread.join()
            workload_elapsed = time.perf_counter() - workload_started

            failed_reads = operation_metrics_failed(metrics, "knowledge_reads")
            failed_cycles = operation_metrics_failed(metrics, "governed_evolution_cycles")
            if failed_reads or failed_cycles:
                raise RuntimeError(
                    f"soak operation failures: reads={failed_reads}, evolution={failed_cycles}"
                )

            # Stop RSS sampling before destroying the process.  Samples from the
            # restarted process must never be mixed into the long-run growth fit.
            stop_memory.set()
            memory_thread.join(timeout=2)

            # Freeze the evidence used by the RSS trend before recovery; samples
            # from the restarted process have a different lifetime and must not
            # affect the long-run trend.
            soak_memory = list(memory)

            # Leave the system quiescent briefly so the crash tests persisted state,
            # not an in-flight request whose outcome would legitimately be uncertain.
            time.sleep(min(0.5, args.rss_sample_interval))
            current_before = http_json(base, "/api/v1/knowledge/ontology/current", token)
            probe_id = "recovery-pending-probe"
            http_json(
                base,
                "/api/v1/knowledge/proposals",
                token,
                {
                    "sourceVersionId": current_before["id"],
                    "proposal": {
                        "id": probe_id,
                        "changes": [{
                            "Add": {
                                "SubClassOf": {
                                    "sub": {"Class": "RecoveryProbe"},
                                    "sup": {"Class": "Thing"},
                                }
                            }
                        }],
                    },
                },
            )
            proposals_before = http_json(base, "/api/v1/knowledge/proposals", token)["list"]
            probe_before = next(item for item in proposals_before if item["id"] == probe_id)
            if probe_before["applied_version_id"] is not None:
                raise RuntimeError("recovery probe unexpectedly has an applied version")

            kill_elapsed = server.kill()
            recovery_started = time.perf_counter()
            server.start()
            base = server.base
            assert base is not None
            _, token = login(server)
            ready_elapsed = time.perf_counter() - recovery_started

            verification_started = time.perf_counter()
            current_after = http_json(server.base, "/api/v1/knowledge/ontology/current", token)
            proposals_after = http_json(server.base, "/api/v1/knowledge/proposals", token)["list"]
            probe_after = next(item for item in proposals_after if item["id"] == probe_id)
            diagnosis = http_json(server.base, "/api/v1/knowledge/diagnosis", token)
            verified = (
                current_after == current_before
                and probe_after["source_version_id"] == probe_before["source_version_id"]
                and probe_after["proposal"] == probe_before["proposal"]
                and probe_after["review"]["decision"] == "Pending"
                and probe_after["applied_version_id"] is None
                and diagnosis["consistent"] is True
            )
            verification_elapsed = time.perf_counter() - verification_started
            if not verified:
                raise RuntimeError(
                    json.dumps(
                        {
                            "current_before": current_before,
                            "current_after": current_after,
                            "probe_before": probe_before,
                            "probe_after": probe_after,
                            "diagnosis": diagnosis,
                        },
                        ensure_ascii=False,
                    )
                )
        finally:
            stop_memory.set()
            memory_thread.join(timeout=2)
            server.close()

        total_seconds = time.monotonic() - started_monotonic
        operation_metrics = metrics.snapshot()
        successful_reads = operation_metrics.get("knowledge_reads", {}).get("successful", 0)
        successful_cycles = operation_metrics.get("governed_evolution_cycles", {}).get("successful", 0)
        return {
            "schema_version": 1,
            "scenario": "knowledge_reason_evolution_soak_crash_recovery",
            "status": "passed",
            "environment": environment_metadata(binary, odl, started_at_utc),
            "root_version_id": root_id,
            "duration_requested_seconds": args.duration_seconds,
            "workload_elapsed_seconds": round(workload_elapsed, 6),
            "total_elapsed_seconds": round(total_seconds, 6),
            "reader_concurrency": args.concurrency,
            "mutation_interval_seconds": args.mutation_interval,
            "operations": operation_metrics,
            "throughput_rps": round(successful_reads / workload_elapsed, 3) if workload_elapsed else 0,
            "evolution_cycles_per_second": round(successful_cycles / workload_elapsed, 6) if workload_elapsed else 0,
            "memory_rss_kb": memory_summary(soak_memory, warmup_until, args.rss_sample_interval),
            "crash_recovery": {
                "method": "SIGKILL after committing a Pending ontology proposal",
                "kill_seconds": round(kill_elapsed, 6),
                "restart_to_ready_seconds": round(ready_elapsed, 6),
                "state_verification_seconds": round(verification_elapsed, 6),
                "total_unavailable_seconds": round(time.perf_counter() - recovery_started, 6),
                "verified": True,
                "pending_proposal_id": probe_id,
            },
        }


def sample_memory(
    server: Server,
    samples: list[tuple[float, int]],
    interval: float,
    stop: threading.Event,
) -> None:
    while not stop.wait(interval):
        if server.process is not None:
            value = rss_kb(server.process.pid)
            if value is not None:
                samples.append((time.monotonic(), value))


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--odl", required=True)
    parser.add_argument("--output")
    parser.add_argument("--duration-seconds", type=float, default=3600)
    parser.add_argument("--concurrency", type=int, default=4)
    parser.add_argument("--mutation-interval", type=float, default=5)
    parser.add_argument("--rss-sample-interval", type=float, default=1)
    parser.add_argument("--memory-warmup-seconds", type=float, default=60)
    args = parser.parse_args()
    if args.duration_seconds <= 0:
        parser.error("--duration-seconds must be positive")
    if args.concurrency <= 0:
        parser.error("--concurrency must be positive")
    if args.mutation_interval <= 0:
        parser.error("--mutation-interval must be positive")
    if args.rss_sample_interval <= 0:
        parser.error("--rss-sample-interval must be positive")
    if args.memory_warmup_seconds < 0 or args.memory_warmup_seconds >= args.duration_seconds:
        parser.error("--memory-warmup-seconds must be zero or less than --duration-seconds")
    return args


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def environment_metadata(binary: Path, odl: Path, started_at_utc: str) -> dict[str, Any]:
    try:
        revision = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    except (OSError, subprocess.CalledProcessError):
        revision = None
    uname = platform.uname()
    return {
        "started_at_utc": started_at_utc,
        "hostname": socket.gethostname(),
        "os": f"{uname.system} {uname.release}",
        "machine": uname.machine,
        "cpu_count": os.cpu_count(),
        "git_revision": revision,
        "binary_sha256": sha256(binary),
        "odl_sha256": sha256(odl),
    }


def write_json(output: Path, result: dict[str, Any]) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_name(output.name + ".tmp")
    temporary.write_text(json.dumps(result, ensure_ascii=False) + "\n")
    temporary.replace(output)


def main() -> int:
    args = parse_args()
    try:
        result = run(args)
    except Exception as error:
        failure = {"schema_version": 1, "status": "failed", "error": str(error)}
        print(json.dumps(failure, ensure_ascii=False), file=sys.stderr)
        if args.output:
            write_json(Path(args.output), failure)
        return 1
    if args.output:
        write_json(Path(args.output), result)
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
