#!/usr/bin/env python3
"""Repeatable real-binary database backup / restore rehearsal.

The rehearsal exercises the operational path rather than an in-process API:
start the release binary over HTTP, seed deterministic objects through the
public project-event transport, stop the service, invoke ``oaip backup``,
restore into a fresh destination, restart the restored service, then validate
readiness, deterministic identity, and audit-chain health.  The JSON report is
intended to be archived as release evidence.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import signal
import socket
import subprocess
import sys
import sqlite3
import tempfile
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

MIB = 1024 * 1024


class RehearsalError(RuntimeError):
    pass


@dataclass(frozen=True)
class Server:
    process: subprocess.Popen[bytes]
    base_url: str
    log_path: Path


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def observed_database_bytes(database: Path) -> int:
    """Return main database plus SQLite/libSQL sidecar size in bytes."""
    paths = [database, *database.parent.glob(database.name + "-*")]
    return sum(path.stat().st_size for path in paths if path.is_file())


def http_json(
    base_url: str,
    path: str,
    token: str | None = None,
    payload: dict[str, Any] | None = None,
    timeout: float = 30.0,
) -> dict[str, Any]:
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    request = urllib.request.Request(
        base_url + path,
        data=None if payload is None else json.dumps(payload).encode(),
        headers=headers,
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            body = json.load(response)
    except urllib.error.HTTPError as error:
        detail = error.read().decode(errors="replace")
        raise RehearsalError(f"{path}: HTTP {error.code}: {detail}") from error
    except (OSError, json.JSONDecodeError) as error:
        raise RehearsalError(f"{path}: {error}") from error
    if body.get("code") != 0:
        raise RehearsalError(f"{path}: unexpected API response: {body}")
    return body.get("data") or {}


def wait_ready(base_url: str, timeout: float) -> tuple[float, dict[str, Any]]:
    deadline = time.monotonic() + timeout
    last_error: Exception | None = None
    started = time.perf_counter()
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(f"{base_url}/readyz", timeout=1) as response:
                body = json.load(response)
                if response.status == 200 and body.get("status") == "ready":
                    return time.perf_counter() - started, body
                last_error = RuntimeError(f"readyz returned {response.status}: {body}")
        except (OSError, urllib.error.URLError, json.JSONDecodeError) as error:
            last_error = error
        time.sleep(0.05)
    raise RehearsalError(f"service did not become ready in {timeout:.3f}s: {last_error}")


def run_checked(command: list[str | Path], cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    completed = subprocess.run(
        command,
        cwd=None if cwd is None else str(cwd),
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        check=False,
    )
    rendered = " ".join(str(item) for item in command)
    if completed.returncode != 0:
        raise RehearsalError(
            f"command failed ({completed.returncode}): {rendered}\n{completed.stdout.strip()}"
        )
    return completed


def read_log(path: Path) -> str:
    try:
        return path.read_text(errors="replace")
    except OSError:
        return ""


def start_server(
    binary: Path,
    database: Path,
    odl: Path,
    root: Path,
    timeout: float,
) -> tuple[Server, float]:
    port = free_port()
    base_url = f"http://127.0.0.1:{port}"
    log_path = database.parent / f"server-{database.stem}.log"
    environment = os.environ.copy()
    environment.update(
        WB_BOOTSTRAP_ADMIN_USER="backup-rehearsal-admin",
        WB_BOOTSTRAP_ADMIN_PWD="backup-rehearsal-password",
    )
    started = time.perf_counter()
    process: subprocess.Popen[bytes] | None = None
    try:
        with log_path.open("wb") as log_stream:
            process = subprocess.Popen(
                [
                    str(binary),
                    "--db", str(database),
                    "serve",
                    "--host", "127.0.0.1",
                    "--port", str(port),
                    "--static-dir", str(root / "ui/dist"),
                    "--odl", str(odl),
                ],
                cwd=str(root),
                env=environment,
                stdout=log_stream,
                stderr=subprocess.STDOUT,
            )
        _, body = wait_ready(base_url, timeout)
    except Exception:
        if process is not None and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        raise
    server = Server(process=process, base_url=base_url, log_path=log_path)
    return server, time.perf_counter() - started


def stop_server(server: Server, timeout: float = 15.0) -> float:
    started = time.perf_counter()
    if server.process.poll() is None:
        server.process.send_signal(signal.SIGTERM)
    try:
        server.process.wait(timeout=timeout)
    except subprocess.TimeoutExpired as error:
        server.process.kill()
        server.process.wait(timeout=5)
        raise RehearsalError("server did not stop before SIGKILL fallback") from error
    elapsed = time.perf_counter() - started
    if server.process.returncode != 0:
        raise RehearsalError(
            f"server exited unexpectedly ({server.process.returncode}):\n"
            f"{read_log(server.log_path)}"
        )
    return elapsed


def terminate_quietly(server: Server | None) -> None:
    if server is None or server.process.poll() is not None:
        return
    server.process.terminate()
    try:
        server.process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        server.process.kill()
        server.process.wait(timeout=5)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(4 * MIB), b""):
            digest.update(block)
    return digest.hexdigest()


def seed_objects(
    server: Server,
    token: str,
    database: Path,
    target_bytes: int,
    payload_kib: int,
    concurrency: int,
    timeout: float,
) -> tuple[int, float, int, int]:
    marker = "backup-rehearsal-v1"
    effective_payload = max(1, payload_kib * 1024 - 128)
    padding = "r" * effective_payload
    started = time.perf_counter()

    def write_object(index: int) -> None:
        body = {
            "object_type": "BackupProbe",
            "id": f"rehearsal:{index:08d}",
            "project": "backup-rehearsal",
            "input": {
                "index": index,
                "marker": marker,
                "padding": padding,
            },
            "edge": None,
        }
        http_json(server.base_url, "/api/v1/facade/project/event", token, body)

    index = 0
    observed = observed_database_bytes(database)
    while observed < target_bytes:
        remaining = max(0, target_bytes - observed)
        batch_size = max(
            concurrency,
            min(concurrency * 32, (remaining + effective_payload - 1) // effective_payload),
        )
        indexes = range(index, index + batch_size)
        next_index = index + batch_size
        with ThreadPoolExecutor(max_workers=concurrency) as executor:
            failures: list[tuple[int, Exception]] = []
            futures = [(current_index, executor.submit(write_object, current_index)) for current_index in indexes]
            for current_index, future in futures:
                try:
                    future.result()
                except Exception as error:
                    failures.append((current_index, error))
            if failures:
                first_index, first_error = failures[0]
                raise RehearsalError(f"seed write failed at object {first_index}: {first_error}")
        index = next_index
        observed = observed_database_bytes(database)
        if time.perf_counter() - started > timeout:
            raise RehearsalError(
                f"seeding reached {observed / MIB:.3f} MiB after {index} objects; "
                f"target was {target_bytes / MIB:.3f} MiB"
            )

    duration = time.perf_counter() - started
    logical_payload_bytes = index * payload_kib * 1024
    return index, duration, observed, logical_payload_bytes


def read_readyz(base_url: str) -> dict[str, Any]:
    """Read the public readiness contract, which is not API-envelope wrapped."""
    with urllib.request.urlopen(f"{base_url}/readyz", timeout=30) as response:
        if response.status != 200:
            raise RehearsalError(f"/readyz returned HTTP {response.status}")
        return json.load(response)


def verify_identity(
    database: Path,
    project_id: str,
    object_type: str,
    marker: str,
    expected_count: int,
    samples: list[str],
    expected_payload_bytes: int,
) -> dict[str, Any]:
    """Validate restored rows directly and read-only.

    The public ``object list`` CLI is intentionally not used for count
    evidence: its current contract is a 1,000-row operational view, not a
    scalable audit query.  SQLite read-only access checks durable restored rows
    without bypassing the backup/restore tool boundary.
    """
    connection = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
    try:
        count_row = connection.execute(
            "SELECT COUNT(*) FROM wb_ontology_object "
            "WHERE project_id=? AND type=? AND is_delete=0",
            (project_id, object_type),
        ).fetchone()
        marker_row = connection.execute(
            "SELECT COUNT(*) FROM wb_ontology_object "
            "WHERE project_id=? AND type=? AND is_delete=0 "
            "AND json_extract(data, '$.marker')=?",
            (project_id, object_type, marker),
        ).fetchone()
        if count_row is None or marker_row is None:
            raise RehearsalError("restored database returned no verification row")
        placeholders = ",".join("?" for _ in samples)
        selected = {
            str(value[0])
            for value in connection.execute(
                "SELECT id FROM wb_ontology_object "
                "WHERE project_id=? AND type=? AND is_delete=0 "
                f"AND id IN ({placeholders})",
                (project_id, object_type, *samples),
            ).fetchall()
        }
        payload_lengths = [
            len(row[0]) if row[0] is not None else 0
            for row in connection.execute(
                "SELECT json_extract(data, '$.padding') FROM wb_ontology_object "
                "WHERE project_id=? AND type=? AND is_delete=0 LIMIT ?",
                (project_id, object_type, max(1, expected_count)),
            )
        ]
    finally:
        connection.close()

    actual_count = int(count_row[0])
    marker_count = int(marker_row[0])
    return {
        "expectedCount": expected_count,
        "actualCount": actual_count,
        "countMatches": actual_count == expected_count,
        "markerCount": marker_count,
        "markerMatches": marker_count == expected_count,
        "sampleIdsChecked": len(samples),
        "missingSampleIds": [value for value in samples if value not in selected],
        "payloadRowsChecked": len(payload_lengths),
        "payloadLengthsMatch": bool(payload_lengths) and set(payload_lengths) == {expected_payload_bytes},
        "identityMatches": (
            actual_count == expected_count
            and marker_count == expected_count
            and all(value in selected for value in samples)
            and set(payload_lengths) == {expected_payload_bytes}
        ),
    }


def write_report(path: Path | None, report: dict[str, Any]) -> None:
    if path is None:
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    temporary.replace(path)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--odl", required=True, type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--target-mib", type=float, default=64.0)
    parser.add_argument("--payload-kib", type=int, default=16)
    parser.add_argument("--concurrency", type=int, default=8)
    parser.add_argument("--ready-timeout-seconds", type=float, default=120.0)
    parser.add_argument("--seed-timeout-seconds", type=float, default=3600.0)
    parser.add_argument("--sample-count", type=int, default=8)
    parser.add_argument("--keep-work-dir", action="store_true")
    args = parser.parse_args()

    if args.target_mib <= 0:
        parser.error("--target-mib must be positive")
    if not 1 <= args.payload_kib <= 1024:
        parser.error("--payload-kib must be in [1, 1024]")
    if not 1 <= args.concurrency <= 64:
        parser.error("--concurrency must be in [1, 64]")
    if args.sample_count < 1:
        parser.error("--sample-count must be positive")
    return args


def main() -> int:
    args = parse_args()
    root = Path(__file__).resolve().parents[1]
    binary = args.binary.resolve()
    odl = args.odl.resolve()
    if not binary.is_file():
        print(f"binary is not a file: {binary}", file=sys.stderr)
        return 2
    if not odl.is_file():
        print(f"ODL is not a file: {odl}", file=sys.stderr)
        return 2

    work_dir = Path(tempfile.mkdtemp(prefix="oaip-backup-rehearsal."))
    source_db = work_dir / "source.db"
    backup_db = work_dir / "backup.db"
    restored_dir = work_dir / "restored"
    restored_db = restored_dir / "openaip.db"
    restored_dir.mkdir()
    audit_export = work_dir / "restored-audit.json"

    servers: list[Server] = []
    report: dict[str, Any] = {
        "format": "openaip.backup-rpo-rto-rehearsal.v1",
        "status": "running",
        "startedAt": utc_now(),
        "workDir": str(work_dir),
        "inputs": {
            "binary": str(binary),
            "odl": str(odl),
            "targetBytes": int(args.target_mib * MIB),
            "targetMiB": args.target_mib,
            "payloadKiB": args.payload_kib,
            "concurrency": args.concurrency,
            "objectType": "BackupProbe",
        },
        "targets": {
            "rpoSeconds": 900,
            "rtoSeconds": 300,
            "maximumDatabaseBytes": 1024 * MIB,
        },
    }

    try:
        source_server, source_startup = start_server(
            binary, source_db, odl, root, args.ready_timeout_seconds
        )
        servers.append(source_server)
        token = http_json(
            source_server.base_url,
            "/api/v1/wb/auth/login",
            payload={
                "username": "backup-rehearsal-admin",
                "password": "backup-rehearsal-password",
            },
        )["token"]

        object_count, seed_duration, observed_after_seed, logical_payload = seed_objects(
            source_server,
            token,
            source_db,
            int(args.target_mib * MIB),
            args.payload_kib,
            args.concurrency,
            args.seed_timeout_seconds,
        )
        final_source_bytes = observed_database_bytes(source_db)
        shutdown_duration = stop_server(servers.pop())

        sample_stride = max(1, object_count // args.sample_count)
        sample_ids = [
            f"rehearsal:{min(object_count - 1, offset * sample_stride):08d}"
            for offset in range(args.sample_count)
        ]

        backup_started = time.perf_counter()
        run_checked([
            str(binary),
            "--db", str(source_db),
            "backup", str(backup_db),
        ])
        backup_duration = time.perf_counter() - backup_started
        backup_bytes = backup_db.stat().st_size
        backup_sha256 = sha256_file(backup_db)

        rto_started_wall = time.time()
        rto_started_perf = time.perf_counter()
        run_checked([
            str(binary),
            "--db", str(restored_db),
            "restore", str(backup_db),
        ])
        restore_duration = time.perf_counter() - rto_started_perf
        restored_server, restored_startup = start_server(
            binary, restored_db, odl, root, args.ready_timeout_seconds
        )
        servers.append(restored_server)
        ready_body = read_readyz(restored_server.base_url)
        rto_duration = time.perf_counter() - rto_started_perf

        identity = verify_identity(
            restored_db,
            "backup-rehearsal",
            "BackupProbe",
            "backup-rehearsal-v1",
            object_count,
            sample_ids,
            args.payload_kib * 1024 - 128,
        )
        stop_server(servers.pop())
        run_checked([
            str(binary),
            "--db", str(restored_db),
            "audit", "export", "--output", str(audit_export),
        ])

        report.update({
            "status": "passed" if (
                identity["identityMatches"]
                and ready_body.get("status") == "ready"
                and ready_body.get("auditChain") == "valid"
                and final_source_bytes <= 1024 * MIB
                and rto_duration <= 300
            ) else "failed",
            "completedAt": utc_now(),
            "source": {
                "startupReadySeconds": source_startup,
                "shutdownSeconds": shutdown_duration,
                "objectsWritten": object_count,
                "seedDurationSeconds": seed_duration,
                "logicalPayloadBytes": logical_payload,
                "observedDatabaseBytes": final_source_bytes,
                "observedDatabaseMiB": final_source_bytes / MIB,
                "sizeWithinDocumentedTarget": final_source_bytes <= 1024 * MIB,
            },
            "backup": {
                "durationSeconds": backup_duration,
                "fileBytes": backup_bytes,
                "fileMiB": backup_bytes / MIB,
                "sha256": backup_sha256,
            },
            "restore": {
                "cliDurationSeconds": restore_duration,
                "serviceStartupReadySeconds": restored_startup,
                "totalRtoSeconds": rto_duration,
                "startedAt": datetime.fromtimestamp(rto_started_wall, timezone.utc)
                    .isoformat()
                    .replace("+00:00", "Z"),
                "withinRtoTargetSeconds": rto_duration <= 300,
                "readiness": {
                    "status": ready_body.get("status"),
                    "auditChain": ready_body.get("auditChain"),
                },
                "identity": identity,
                "auditExportPath": str(audit_export),
            },
            "rpoEvidenceBoundary": (
                "A quiescent rehearsal proves snapshot integrity but cannot measure "
                "scheduled-backup write loss; production RPO requires an external "
                "15-minute scheduler and failed-backup alerting evidence."
            ),
        })
        if args.keep_work_dir:
            report["retainedWorkDir"] = str(work_dir)
        else:
            report["retainedWorkDir"] = None
    except Exception as error:
        report.update({
            "status": "failed",
            "completedAt": utc_now(),
            "error": str(error),
        })
        write_report(args.output, report)
        print(f"❌ backup rehearsal failed: {error}", file=sys.stderr)
        return 1
    finally:
        while servers:
            terminate_quietly(servers.pop())
        if not args.keep_work_dir:
            import shutil
            shutil.rmtree(work_dir, ignore_errors=True)

    write_report(args.output, report)
    backup = report["backup"]
    restore = report["restore"]
    print(
        "✅ backup rehearsal passed\n"
        f"objects={report['source']['objectsWritten']} "
        f"source_mib={report['source']['observedDatabaseMiB']:.3f}\n"
        f"backup_s={backup['durationSeconds']:.6f} "
        f"backup_mib={backup['fileMiB']:.3f}\n"
        f"restore_cli_s={restore['cliDurationSeconds']:.6f} "
        f"ready_s={restore['serviceStartupReadySeconds']:.6f} "
        f"rto_s={restore['totalRtoSeconds']:.6f}"
    )
    if args.output:
        print(f"report={args.output}")
    if report["status"] != "passed":
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
