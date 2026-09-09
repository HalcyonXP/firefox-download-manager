"""Actual packaged-helper qualification boundary; no registration or browser access."""
import argparse
from collections import Counter
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import struct
import subprocess
import tempfile
import threading
import time
import uuid

if __package__:
    from .fixture import Fixture, PREFIX_SIZE, SMALL_SIZE, expected_sha256
    from .support import bounded_json, new_report, write_report
else:
    from fixture import Fixture, PREFIX_SIZE, SMALL_SIZE, expected_sha256
    from support import bounded_json, new_report, write_report

MAX_FRAME = 1024 * 1024


def file_sha256(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def owned_architecture(host):
    kernel = ctypes.WinDLL(str(Path(os.environ["WINDIR"]) / "System32/kernel32.dll"), use_last_error=True)
    kernel.IsWow64Process2.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.USHORT), ctypes.POINTER(wintypes.USHORT)]
    kernel.IsWow64Process2.restype = wintypes.BOOL
    process, native = wintypes.USHORT(), wintypes.USHORT()
    if not kernel.IsWow64Process2(int(host.process._handle), ctypes.byref(process), ctypes.byref(native)):
        raise RuntimeError("owned helper architecture query failed")
    if (process.value, native.value) == (0, 0x8664):
        return {"native_machine": "AMD64", "helper_execution": "native_x64"}
    if (process.value, native.value) == (0x8664, 0xAA64):
        return {"native_machine": "ARM64", "helper_execution": "x64_emulation"}
    raise RuntimeError("unsupported qualification execution architecture")


def evidence_identity(package):
    directory = Path(__file__).resolve().parent
    repository = directory.parents[1]
    revision = subprocess.run(["git", "-C", str(repository), "rev-parse", "HEAD"], check=True,
                              capture_output=True, text=True, timeout=15).stdout.strip()
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise RuntimeError("invalid harness revision")
    dirty = bool(subprocess.run(["git", "-C", str(repository), "status", "--porcelain"], check=True,
                               capture_output=True, timeout=15).stdout)
    descriptor = bounded_json(package / "package.json")
    return {"descriptor_sha256": file_sha256(package / "package.json"),
            "helper_sha256": file_sha256(package / "download-manager-native-host.exe"),
            "package_source_commit": descriptor["commit"], "package_target": descriptor["target"],
            "harness_revision": revision, "harness_worktree_dirty": dirty, "python_version": platform.python_version(),
            "harness_files_sha256": {name: file_sha256(directory / name) for name in ("native.py", "fixture.py", "support.py")}}


def read_exact(stream, size):
    data = bytearray()
    while len(data) < size:
        chunk = stream.read(size - len(data))
        if not chunk:
            raise EOFError("owned helper ended its stream")
        data.extend(chunk)
    return bytes(data)


class Host:
    def __init__(self, package, root, owners=None):
        self.package = package
        self.root = root
        self.destination = root / "Profile/Downloads"
        self.destination.mkdir(parents=True, exist_ok=True)
        (root / "Local").mkdir(exist_ok=True)
        environment = {**os.environ, "LOCALAPPDATA": str(root / "Local"), "APPDATA": str(root / "Roaming"),
                       "USERPROFILE": str(root / "Profile"), "HOME": str(root / "Profile"),
                       "PATH": str(Path(os.environ["WINDIR"]) / "System32")}
        self.process = None
        self.condition = threading.Condition()
        self.responses = {}
        self.tasks = {}
        self.completed = set()
        self.failed = set()
        self.closed = False
        self.events = Counter()
        self.event_bytes = 0
        self.failure = None
        self.stderr = b""
        self.serial = 0
        self.reader = threading.Thread(target=self.read, daemon=True)
        self.error_reader = threading.Thread(target=self.read_errors, daemon=True)
        if owners is not None:
            if len(owners) >= 16:
                raise RuntimeError("owned helper count bound")
            owners.append(self)  # Retain constructor failures until shutdown is proven.
        try:
            self.process = subprocess.Popen([str(package / "download-manager-native-host.exe")], cwd=root,
                                            env=environment, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                            stderr=subprocess.PIPE)
            self.reader.start()
            self.error_reader.start()
            result = self.command("hello", {"supported_versions": [2], "client_name": "artifact-qualification", "client_version": "0.1.0"})
            assert result["selected_version"] == 2
            assert {"sha256", "authenticated_requests", "snapshots", "coalesced_progress"} <= set(result["capabilities"])
            self.command("update_settings", {"settings": {"retry_limit": 0}})
        except BaseException:
            self.close(crash=True)
            raise

    def read_errors(self):
        self.stderr = self.process.stderr.read(64 * 1024 + 1)
        if self.stderr:
            with self.condition:
                self.failure = "owned helper emitted unexpected stderr"
                self.condition.notify_all()

    def read(self):
        try:
            while True:
                prefix = self.process.stdout.read(4)
                if not prefix:
                    break
                if len(prefix) != 4:
                    raise ValueError("truncated native prefix")
                length, = struct.unpack("<I", prefix)
                if not 0 < length <= MAX_FRAME:
                    raise ValueError("invalid native frame length")
                message = json.loads(read_exact(self.process.stdout, length))
                with self.condition:
                    if message.get("protocol_version") != 2:
                        raise ValueError("unexpected wire version")
                    if message.get("kind") == "response":
                        if len(self.responses) >= 64:
                            raise ValueError("unbounded native responses")
                        self.responses[message["correlation_id"]] = message
                    elif message.get("kind") == "event":
                        if message["event"] not in {"snapshot", "state_changed", "progress", "warning", "completed", "failed"}:
                            raise ValueError("unknown native event")
                        self.events[message["event"]] += 1
                        self.event_bytes += length + 4
                        if sum(self.events.values()) > 100_000 or self.event_bytes > 64 * 1024 * 1024:
                            raise ValueError("unbounded native event traffic")
                        data = message["data"]
                        tasks = data.get("tasks", [data.get("task", data)])
                        for task in tasks:
                            if "task_id" in task and "state" in task:
                                self.tasks[task["task_id"]] = task
                            if message["event"] == "progress" and task.get("task_id") in self.tasks:
                                self.tasks[task["task_id"]]["bytes_completed"] = task["bytes_completed"]
                        if message["event"] == "completed":
                            self.completed.add(data["task_id"])
                        if message["event"] == "failed":
                            self.failed.add(data["task"]["task_id"])
                        if len(self.tasks) > 64:
                            raise ValueError("unbounded fixture task state")
                    else:
                        raise ValueError("unexpected native message")
                    self.condition.notify_all()
        except (ValueError, KeyError, EOFError, OSError, TypeError, AttributeError):
            with self.condition:
                self.failure = "invalid owned-helper output"
                self.condition.notify_all()

    def wait(self, predicate, timeout=30):
        deadline = time.monotonic() + timeout
        with self.condition:
            while not predicate():
                if self.failure or self.process.poll() is not None:
                    raise RuntimeError(self.failure or "owned helper exited before acknowledgement")
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise RuntimeError("owned helper acknowledgement deadline")
                self.condition.wait(min(remaining, 0.2))

    def command(self, command, payload):
        self.serial += 1
        correlation = f"qualification-{self.serial}"
        body = json.dumps({"protocol_version": 2, "correlation_id": correlation, "kind": "command", "command": command, "payload": payload}).encode()
        assert len(body) < MAX_FRAME
        self.process.stdin.write(struct.pack("<I", len(body)) + body)
        self.process.stdin.flush()
        self.wait(lambda: correlation in self.responses)
        with self.condition:
            response = self.responses.pop(correlation)
        if response.get("ok") is not True:
            raise RuntimeError(f"native fixture command rejected: {command}")
        return response["result"]

    def add(self, url, name, workers=4, size=SMALL_SIZE, digest=None):
        return self.command("add", {"url": url, "destination": str(self.destination), "suggested_filename": name,
                                    "workers": workers, "checksum": {"algorithm": "sha256", "digest": digest or expected_sha256(size)}})["task_id"]

    def terminal(self, task_id, timeout=120):
        self.wait(lambda: task_id in self.tasks and (self.tasks[task_id]["state"] == "cancelled" or task_id in self.failed or task_id in self.completed), timeout)
        with self.condition:
            return dict(self.tasks[task_id])

    def close(self, crash=False):
        if self.closed:
            return
        if self.process is None:
            self.closed = True
            return
        forced = False
        if self.process.poll() is None:
            if crash:
                # Popen retains the handle for this exact owned child, not a reused PID.
                self.process.kill()
            else:
                try:
                    self.process.stdin.close()
                except BrokenPipeError:
                    pass
            try:
                self.process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=10)
                forced = True
        for reader in (self.reader, self.error_reader):
            if reader.ident is not None:
                reader.join(timeout=10)
            if reader.is_alive():
                raise RuntimeError("owned pipe reader did not join")
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            stream.close()
        self.closed = True
        if forced:
            raise RuntimeError("owned helper required forced shutdown")
        if not crash and (self.process.returncode != 0 or self.stderr or self.failure):
            raise RuntimeError("owned helper shutdown was not clean")


class MemoryCounters(ctypes.Structure):
    _fields_ = [("cb", wintypes.DWORD), ("page_faults", wintypes.DWORD)] + [(name, ctypes.c_size_t) for name in
        ("peak_working_set", "working_set", "quota_peak_paged", "quota_paged", "quota_peak_nonpaged", "quota_nonpaged", "pagefile", "peak_pagefile", "private_usage")]


class IoCounters(ctypes.Structure):
    _fields_ = [(name, ctypes.c_ulonglong) for name in ("read_ops", "write_ops", "other_ops", "read_bytes", "write_bytes", "other_bytes")]


class Measurements:
    def __init__(self, host):
        self.host = host
        system = Path(os.environ["WINDIR"]) / "System32"
        self.kernel = ctypes.WinDLL(str(system / "kernel32.dll"), use_last_error=True)
        self.psapi = ctypes.WinDLL(str(system / "psapi.dll"), use_last_error=True)
        self.kernel.GetProcessTimes.argtypes = [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4
        self.kernel.GetProcessTimes.restype = wintypes.BOOL
        self.kernel.GetProcessIoCounters.argtypes = [wintypes.HANDLE, ctypes.POINTER(IoCounters)]
        self.kernel.GetProcessIoCounters.restype = wintypes.BOOL
        self.psapi.GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(MemoryCounters), wintypes.DWORD]
        self.psapi.GetProcessMemoryInfo.restype = wintypes.BOOL
        self.kernel.GetCompressedFileSizeW.argtypes = [wintypes.LPCWSTR, ctypes.POINTER(wintypes.DWORD)]
        self.kernel.GetCompressedFileSizeW.restype = wintypes.DWORD

    def sample(self):
        handle = int(self.host.process._handle)  # CPython Windows: retained Popen child handle.
        memory, io = MemoryCounters(), IoCounters()
        memory.cb = ctypes.sizeof(memory)
        times = [wintypes.FILETIME() for _ in range(4)]
        if not (self.psapi.GetProcessMemoryInfo(handle, ctypes.byref(memory), memory.cb)
                and self.kernel.GetProcessTimes(handle, *(ctypes.byref(value) for value in times))
                and self.kernel.GetProcessIoCounters(handle, ctypes.byref(io))):
            raise RuntimeError("owned-process resource sampling failed")
        cpu = sum(value.dwLowDateTime + (value.dwHighDateTime << 32) for value in times[2:]) / 10_000_000
        return {"cpu_seconds": cpu, "peak_working_set_bytes": memory.peak_working_set,
                "working_set_bytes": memory.working_set, "private_usage_bytes": memory.private_usage,
                "aggregate_io_read_bytes": io.read_bytes, "aggregate_io_write_bytes": io.write_bytes}

    def allocated(self, path):
        high = wintypes.DWORD()
        ctypes.set_last_error(0)
        low = self.kernel.GetCompressedFileSizeW(str(path), ctypes.byref(high))
        if low == 0xFFFFFFFF and ctypes.get_last_error():
            raise RuntimeError("owned output allocation query failed")
        return low + (high.value << 32)


def task_status(task):
    """Allowlisted diagnostics only: never serialize a URL, path, header or arbitrary error."""
    state = task.get("state")
    states = {"queued", "probing", "downloading", "paused", "validating", "completed", "cancelled", "failed"}
    error = task.get("error")
    code = error.get("code") if isinstance(error, dict) else None
    codes = {"PROBE_FAILED", "RANGE_UNSUPPORTED", "RANGE_RESPONSE_INVALID", "RESOURCE_CHANGED", "HTTP_STATUS",
             "RETRY_EXHAUSTED", "STORAGE_ERROR", "DISK_FULL", "ACCESS_DENIED", "FILE_LOCKED", "FILE_EXISTS",
             "STATE_CORRUPT", "CHECKSUM_MISMATCH", "CANCELLED", "INTERNAL_ERROR", "AUTH_REQUIRED", "AUTH_EXPIRED"}
    return {"state": state if isinstance(state, str) and state in states else "other",
            "code": code if isinstance(code, str) and code in codes else "other"}


def completed(host, task_id, name, size, workers, mode="segmented"):
    task = host.terminal(task_id)
    assert task["state"] == "completed" and task["workers"] == workers and task["transfer_mode"] == mode
    path = host.destination / name
    assert path.stat().st_size == size and file_sha256(path) == expected_sha256(size)
    return task


def matrix(package, root, fixture, owners=None):
    host = Host(package, root, owners)
    checks = []
    try:
        architecture = owned_architecture(host)
        for workers in (1, 2, 4, 8):
            name = f"workers-{workers}.bin"
            completed(host, host.add(fixture.url("range"), name, workers), name, SMALL_SIZE, workers)
            checks.append(f"{workers}-worker-exact-output-and-checksum")
        completed(host, host.add(fixture.url("single"), "single.bin"), "single.bin", SMALL_SIZE, 4, "single")
        checks.append("ignored-range-single-fallback-exact-output")
        for mode, size in (("empty", 0), ("unknown", SMALL_SIZE), ("weak", SMALL_SIZE), ("missing-validator", SMALL_SIZE)):
            name = mode + ".bin"
            completed(host, host.add(fixture.url(mode), name, size=size), name, size, 4, "single")
            checks.append(f"{mode}-single-stream-exact-output-and-checksum")
        for mode, allowed in [("bad-range", {"RANGE_RESPONSE_INVALID"}), ("change", {"RESOURCE_CHANGED", "RANGE_RESPONSE_INVALID"}),
                              ("truncate", {"RETRY_EXHAUSTED", "RANGE_RESPONSE_INVALID"}),
                              ("worker-ignored", {"HTTP_STATUS"}), ("missing-range", {"RANGE_RESPONSE_INVALID"}),
                              ("out-of-bounds", {"RANGE_RESPONSE_INVALID"}), ("corrupt", {"CHECKSUM_MISMATCH"})]:
            task = host.terminal(host.add(fixture.url(mode), f"{mode}.bin"))
            assert task["state"] == "failed" and task["error"]["code"] in allowed, (mode, task_status(task))
            assert not (host.destination / f"{mode}.bin").exists()
            checks.append(f"{mode}-refused-no-publication")
        task = host.terminal(host.add(fixture.url("range"), "mismatch.bin", digest="f" * 64))
        assert task["state"] == "failed" and task["error"]["code"] == "CHECKSUM_MISMATCH"
        assert not (host.destination / "mismatch.bin").exists()
        checks.append("checksum-mismatch-no-publication")
        marker = host.destination / "collision.bin"
        marker.write_bytes(b"existing final marker")
        collision = host.add(fixture.url("range"), "collision.bin")
        task = host.terminal(collision)
        assert task["display_name"] == "collision (1).bin"
        completed(host, collision, "collision (1).bin", SMALL_SIZE, 4)
        assert marker.read_bytes() == b"existing final marker"
        checks.append("collision-resolved-output-without-overwriting-existing-final")
        assert host.command("remove", {"task_id": collision, "delete_partial": True})["removed_task_id"] == collision
        assert not task_record_path(host, collision).exists()
        assert file_sha256(host.destination / "collision (1).bin") == expected_sha256(SMALL_SIZE)
        checks.append("remove-completed-task-preserves-published-output")
        task_id = host.add(fixture.url("slow"), "pause.bin")
        host.wait(lambda: host.tasks.get(task_id, {}).get("bytes_completed", 0) > 0)
        host.command("pause", {"task_id": task_id})
        host.wait(lambda: host.tasks.get(task_id, {}).get("state") == "paused")
        assert not (host.destination / "pause.bin").exists()
        host.command("resume", {"task_id": task_id})
        completed(host, task_id, "pause.bin", SMALL_SIZE, 4)
        checks.append("pause-resume-exact-output")
        task_id = host.add(fixture.url("slow"), "cancel.bin")
        host.wait(lambda: host.tasks.get(task_id, {}).get("bytes_completed", 0) > 0)
        host.command("cancel", {"task_id": task_id, "partial_policy": "keep"})
        assert host.terminal(task_id)["state"] == "cancelled"
        assert not (host.destination / "cancel.bin").exists()
        checks.append("cancel-no-publication")
        task_id = host.add(fixture.url("slow"), "restart.bin")
        host.wait(lambda: host.tasks.get(task_id, {}).get("bytes_completed", 0) >= 2 * 1024 * 1024)
        host.close(crash=True)
        host = Host(package, root, owners)
        host.wait(lambda: task_id in host.tasks)
        assert host.tasks[task_id]["state"] not in {"downloading", "completed"}
        host.command("resume", {"task_id": task_id})
        completed(host, task_id, "restart.bin", SMALL_SIZE, 4)
        checks.append("actual-owned-helper-kill-restart-explicit-resume-exact-output")
    finally:
        host.close()
    return checks, architecture


def task_record_path(host, task_id):
    if str(uuid.UUID(task_id, version=4)) != task_id:
        raise RuntimeError("invalid owned task identifier")
    return host.root / "Local/HalcyonXP/FirefoxDownloadManager/state/tasks" / f"{task_id}.task.json"


def owned_task(host, task_id):
    record = bounded_json(task_record_path(host, task_id))
    assert record["version"] == 4 and record["task"]["task_id"] == task_id
    return record["task"]


def exact_prefix(ranges):
    assert 0 < len(ranges) <= 128
    for item in ranges:
        assert set(item) == {"start", "end"} and all(type(value) is int for value in item.values())
    cursor = 0
    for item in sorted(ranges, key=lambda value: value["start"]):
        assert item["start"] == cursor and cursor < item["end"] <= PREFIX_SIZE
        cursor = item["end"]
    assert cursor == PREFIX_SIZE  # No gaps/overlaps, regardless of adjacent range coalescing.


def retained_prefix(host, task_id):
    task = owned_task(host, task_id)
    assert task["state"] == "paused"
    exact_prefix(task["completed_ranges"])
    raw = task["partial_path"]
    # Rust canonicalize emits the extended local-drive prefix; Python resolve does not.
    # Strip only that spelling for this exact owned drive, never a UNC/device namespace.
    extended = "\\\\?\\" + host.destination.drive + "\\"
    if raw.startswith(extended):
        raw = raw[4:]
    partial = Path(raw)
    if (partial.parent != host.destination or ":" in partial.name or partial.name.endswith((".", " "))
            or partial.is_symlink() or partial.is_junction() or partial.resolve() != partial or not partial.is_file()):
        raise RuntimeError("retained partial is not an ordinary owned file")
    with partial.open("rb") as source:
        assert hashlib.sha256(source.read(PREFIX_SIZE)).hexdigest() == expected_sha256(PREFIX_SIZE)
    return partial


def assignments(fixture, mode):
    with fixture.lock:
        return list(fixture.retained_requests[mode])


def wait_retained(host, task_id, fixture, mode, count):
    def observed():
        with fixture.lock:
            return fixture.retained_waiting[mode] == count
    try:
        host.wait(lambda: host.tasks.get(task_id, {}).get("bytes_completed", 0) >= PREFIX_SIZE and observed())
    except RuntimeError:
        with fixture.lock:
            detail = {"requests": len(fixture.retained_requests[mode]), "gate_waiters": fixture.retained_waiting[mode]}
        raise RuntimeError(f"retained observation deadline: case={mode}, {detail}, {task_status(host.tasks.get(task_id, {}))}") from None


def retention_matrix(package, root, fixture, owners=None):
    host = Host(package, root, owners)
    checks = []
    try:
        for mode in ("retained-resume", "retained-restart", "retained-changed", "retained-cancel"):
            fixture.retained_body.clear()
            name = mode + ".bin"
            task_id = host.add(fixture.url(mode), name)
            # All four workers are held beyond a complete prefix. The planner is adaptive:
            # four workers use 1 MiB chunks here, not the one-worker fixture's 2 MiB.
            wait_retained(host, task_id, fixture, mode, 4)
            initial = assignments(fixture, mode)
            exact_prefix([{"start": start, "end": end + 1} for start, end in initial if start < PREFIX_SIZE])
            host.command("pause", {"task_id": task_id})
            host.wait(lambda: host.tasks.get(task_id, {}).get("state") == "paused")
            partial = retained_prefix(host, task_id)  # Joined acknowledgement + actual persisted coverage/disk prefix.
            assert not (host.destination / name).exists()
            if mode == "retained-cancel":
                host.command("cancel", {"task_id": task_id, "partial_policy": "delete"})
                assert host.terminal(task_id)["state"] == "cancelled" and not partial.exists()
                assert owned_task(host, task_id)["partial_path"] is None
                host.command("remove", {"task_id": task_id, "delete_partial": True})
                assert not task_record_path(host, task_id).exists() and not (host.destination / name).exists()
                checks.append("cancel-delete-durable-partial-and-remove-task-without-publication")
            elif mode == "retained-changed":
                with fixture.lock:
                    fixture.generations[mode] = 2
                host.command("resume", {"task_id": task_id})
                task = host.terminal(task_id)
                assert task["state"] == "failed" and task["error"]["code"] == "RESOURCE_CHANGED"
                assert assignments(fixture, mode) == initial and not (host.destination / name).exists()
                checks.append("changed-identity-refuses-nonempty-resume-before-new-workers")
            else:
                host.command("resume", {"task_id": task_id})
                # Cancelled server-side waiters persist until release; they are not local workers.
                wait_retained(host, task_id, fixture, mode, 8)
                if mode == "retained-restart":
                    exact_prefix(owned_task(host, task_id)["completed_ranges"])
                    host.close(crash=True)
                    host = Host(package, root, owners)
                    host.wait(lambda: task_id in host.tasks)
                    assert host.tasks[task_id]["state"] not in {"downloading", "completed"}
                    retained_prefix(host, task_id)
                    host.command("resume", {"task_id": task_id})
                fixture.retained_body.set()
                completed(host, task_id, name, SMALL_SIZE, 4)
                seen = assignments(fixture, mode)
                assert seen[:len(initial)] == initial
                assert all(start >= PREFIX_SIZE for start, _ in seen[len(initial):])
                checks.append(f"{mode}-durable-prefix-exact-output-without-prefix-refetch")
            fixture.retained_body.set()
    finally:
        fixture.retained_body.set()
        host.close()
    return checks


def large(package, root, fixture, size, owners=None):
    host = Host(package, root, owners)
    try:
        metrics = Measurements(host)
        architecture = owned_architecture(host)
        digest = expected_sha256(size)
        start_metrics = metrics.sample()
        start = time.monotonic()
        initial_events = sum(host.events.values())
        initial_bytes = host.event_bytes
        task_id = host.add(fixture.url("large"), "large.bin", size=size, digest=digest)
        samples = []
        while task_id not in host.completed:
            if host.failure or host.process.poll() is not None or host.tasks.get(task_id, {}).get("state") == "failed" or time.monotonic() - start > 600:
                raise RuntimeError("large native fixture failed or exceeded deadline")
            samples.append(metrics.sample())
            if len(samples) > 6000:
                raise RuntimeError("resource sample bound exceeded")
            time.sleep(0.1)
        elapsed = time.monotonic() - start
        final = metrics.sample()
        path = host.destination / "large.bin"
        assert path.stat().st_size == size and file_sha256(path) == digest
        return {**architecture, "size_bytes": size, "expected_sha256": digest, "workers": 4, "elapsed_seconds": elapsed,
                "cpu_seconds": final["cpu_seconds"] - start_metrics["cpu_seconds"],
                "peak_working_set_bytes": final["peak_working_set_bytes"],
                "peak_sampled_private_usage_bytes": max([final["private_usage_bytes"], *[s["private_usage_bytes"] for s in samples]]),
                "sampling_interval_seconds": 0.1, "sample_count": len(samples), "final_allocated_bytes": metrics.allocated(path),
                "aggregate_io_read_bytes": final["aggregate_io_read_bytes"] - start_metrics["aggregate_io_read_bytes"],
                "aggregate_io_write_bytes": final["aggregate_io_write_bytes"] - start_metrics["aggregate_io_write_bytes"],
                "native_events": sum(host.events.values()) - initial_events, "native_event_bytes": host.event_bytes - initial_bytes,
                "scope": "loopback transfer+validation to completion; owned-process memory excludes OS file cache/kernel/other processes; aggregate I/O is not disk-only; no physical-device or Internet/VPN throughput claim"}
    finally:
        host.close()


def qualify(package, report, large_size):
    if not __debug__:
        raise RuntimeError("qualification requires enabled assertions")
    if os.name != "nt":
        raise RuntimeError("qualification harness requires Windows")
    package = package.resolve()
    report = new_report(report)
    subprocess.run([str(package / "download-manager-setup.exe"), "verify"], check=True, timeout=30, capture_output=True)
    identity = evidence_identity(package)
    parent = Path(tempfile.mkdtemp(prefix="dm28 artifact ")).resolve()
    fixture = None
    owners = []
    try:
        fixture = Fixture(large_size)
        if large_size and shutil.disk_usage(parent).free < large_size * 2 + 512 * 1024 * 1024:
            raise RuntimeError("insufficient free space for bounded owned fixture")
        checks, architecture = matrix(package, parent / "Matrix", fixture, owners)
        checks.extend(retention_matrix(package, parent / "Retention", fixture, owners))
        performance = large(package, parent / "Resources", fixture, large_size, owners) if large_size else None
        if evidence_identity(package) != identity:
            raise RuntimeError("artifact or harness identity changed during qualification")
        evidence = {**identity, **architecture, "os": platform.platform(), "harness_process_machine": platform.machine(),
                    "checks": checks, "resources": performance,
                    "scope": "actual packaged native artifact only; no Firefox/registration/profile access; not release approval"}
    finally:
        cleanup_phase = "fixture"
        try:
            if fixture is not None:
                fixture.close()
            cleanup_phase = "owned-native-work"
            if any(not owner.closed for owner in owners):
                raise RuntimeError("native ownership is unresolved")
            cleanup_phase = "domain"
            shutil.rmtree(parent)
        except BaseException:
            ticket = Path(__file__).resolve().parents[2] / ".git" / f"native28-recovery-{uuid.uuid4()}.private.json"
            with ticket.open("x", encoding="utf-8") as recovery:
                json.dump({"owned_domain": str(parent), "cleanup_phase": cleanup_phase}, recovery)
            raise RuntimeError("owned native domain preserved; no success report authorized") from None
    # A report is only committed after owned helper/fixture/root cleanup succeeds.
    write_report(report, evidence)
    print("Native artifact matrix passed; report is scoped, not Firefox/release approval.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--large-bytes", type=int, default=2 * 1024**3)
    args = parser.parse_args()
    if args.large_bytes not in (0, 2 * 1024**3):
        parser.error("use 0 for a labeled smoke or 2147483648 for the 2 GiB measurement")
    qualify(args.package, args.report, args.large_bytes)
