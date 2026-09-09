"""Bounded synthetic loopback bytes for exact-artifact tests; never logs requests."""
from collections import Counter
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import hashlib
import re
import socket
import sys
import threading
import time
from urllib.parse import urlsplit

BLOCK = bytes(range(256)) * 4096  # 1 MiB, reused for arbitrarily large fixtures.
SMALL_SIZE = 8 * 1024 * 1024
PREFIX_SIZE = 2 * 1024 * 1024
RETAINED = {"retained-resume", "retained-restart", "retained-changed", "retained-cancel"}
MODES = {"range", "single", "bad-range", "change", "truncate", "slow", "large", "empty", "unknown",
         "weak", "missing-validator", "worker-ignored", "missing-range", "out-of-bounds", "corrupt", *RETAINED}


def expected_sha256(size):
    digest = hashlib.sha256()
    for _ in range(size // len(BLOCK)):
        digest.update(BLOCK)
    digest.update(BLOCK[:size % len(BLOCK)])
    return digest.hexdigest()


class BoundedServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, *args):
        super().__init__(*args)
        self.ownership = threading.Lock()
        self.handlers = []
        self.errors = 0
        self.error_kinds = Counter()

    def process_request(self, request, address):
        with self.ownership:
            self.handlers = [(t, s) for t, s in self.handlers if t.is_alive()]
            if len(self.handlers) >= 32:
                self.shutdown_request(request)
                return
            thread = threading.Thread(target=self.process_request_thread, args=(request, address), daemon=True)
            self.handlers.append((thread, request))
            try:
                thread.start()
            except Exception:
                self.handlers.remove((thread, request))
                self.shutdown_request(request)
                raise

    def handle_error(self, *_):
        # Never print an unexpected request, header or exception chain.
        failure = sys.exception()
        kind = "peer-disconnect" if isinstance(failure, (ConnectionError, TimeoutError)) else "unexpected"
        with self.ownership:
            self.errors += int(kind == "unexpected")
            self.error_kinds[kind] += 1

    def join_owned_handlers(self):
        # Called after the accept loop joins: no new ownership can be added.
        with self.ownership:
            handlers = list(self.handlers)
        for _, connection in handlers:
            try:
                connection.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
        deadline = time.monotonic() + 10
        for thread, _ in handlers:
            thread.join(timeout=max(0, deadline - time.monotonic()))
        live = sum(t.is_alive() for t, _ in handlers)
        if live or self.errors:
            raise RuntimeError(f"owned fixture shutdown refused: live={live}, errors={dict(self.error_kinds)}")


class Fixture:
    def __init__(self, large_size=2 * 1024**3, handler=None):
        self.large_size = large_size
        self.lock = threading.Lock()
        self.requests = Counter()
        self.active = 0
        self.peak = 0
        self.slow_body = threading.Event()
        self.slow_body.set()
        self.large_body = threading.Event()
        self.large_body.set()
        self.retained_body = threading.Event()
        self.retained_body.set()
        self.retained_requests = {mode: [] for mode in RETAINED}
        self.retained_waiting = {mode: 0 for mode in RETAINED}
        self.generations = {mode: 1 for mode in RETAINED}
        self.server = BoundedServer(("127.0.0.1", 0), handler or Handler)
        self.server.daemon_threads = True
        self.server.fixture = self
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        try:
            self.thread.start()
        except Exception:
            self.server.server_close()
            raise

    def url(self, mode):
        return f"http://127.0.0.1:{self.server.server_port}/{mode}"

    def close(self):
        self.slow_body.set()
        self.large_body.set()
        self.retained_body.set()
        self.server.shutdown()
        self.thread.join(timeout=10)
        try:
            self.server.join_owned_handlers()
        finally:
            self.server.server_close()
        if self.thread.is_alive() or self.active:
            raise RuntimeError("owned fixture did not finish its bounded shutdown")


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def setup(self):
        super().setup()
        self.connection.settimeout(10)  # Apply before request/header parsing.

    def log_message(self, *_):
        pass

    def do_HEAD(self):
        self.reply(False)

    def do_GET(self):
        self.reply(True)

    def reply(self, body):
        fixture = self.server.fixture
        mode = urlsplit(self.path).path.removeprefix("/")
        if mode not in MODES:
            self.send_error(404)
            return
        self.connection.settimeout(10)
        size = fixture.large_size if mode == "large" else 0 if mode == "empty" else SMALL_SIZE
        start, end = 0, size - 1
        ranged = False
        if body and mode not in {"single", "unknown"} and self.headers.get("Range"):
            value = self.headers["Range"]
            match = re.fullmatch(r"bytes=([0-9]+)-([0-9]+)", value) if len(value) <= 64 else None
            if not match or not 0 <= int(match[1]) <= int(match[2]) < size:
                self.send_response(416)
                self.send_header("Content-Range", f"bytes */{size}")
                self.send_header("Content-Length", "0")
                self.send_header("Connection", "close")
                self.end_headers()
                return
            start, end = int(match[1]), int(match[2])
            ranged = True
        # ProbeClient verifies both the first and last byte before scheduling.
        probe = ranged and start == end and start in {0, size - 1}
        with fixture.lock:
            fixture.requests[(mode, self.command, "probe" if probe else "body")] += 1
            generation = fixture.generations.get(mode, 1)
            if mode in RETAINED and body and not probe:
                if len(fixture.retained_requests[mode]) >= 128:
                    raise RuntimeError("retained request observation bound")
                fixture.retained_requests[mode].append((start, end))
            if body:
                fixture.active += 1
                fixture.peak = max(fixture.peak, fixture.active)
        try:
            if mode == "worker-ignored" and ranged and not probe:
                ranged, start, end = False, 0, size - 1
            self.send_response(206 if ranged else 200)
            if mode != "unknown":
                self.send_header("Content-Length", str(end - start + 1))
            self.send_header("Accept-Ranges", "bytes")
            if mode != "missing-validator":
                tag = f'"fixture-v{generation}"'
                if mode == "change" and ranged and not probe:
                    tag = '"fixture-v2"'
                self.send_header("ETag", "W/" + tag if mode == "weak" else tag)
            self.send_header("Connection", "close")
            if ranged and not (mode == "missing-range" and not probe):
                reported = start + 1 if mode == "bad-range" and not probe else start
                reported_end = size if mode == "out-of-bounds" and not probe else end
                self.send_header("Content-Range", f"bytes {reported}-{reported_end}/{size}")
            self.end_headers()
            if body:
                gate = fixture.large_body if mode == "large" else fixture.slow_body
                if mode in {"slow", "large"} and not probe and not gate.wait(timeout=30):
                    raise RuntimeError("owned response gate deadline")
                if mode in RETAINED and not probe and start >= PREFIX_SIZE:
                    with fixture.lock:
                        fixture.retained_waiting[mode] += 1
                    try:
                        if not fixture.retained_body.wait(timeout=30):
                            raise RuntimeError("owned retained-response gate deadline")
                    finally:
                        with fixture.lock:
                            fixture.retained_waiting[mode] -= 1
                limit = start + (end - start + 1) // 2 if mode == "truncate" and not probe else end + 1
                while start < limit:
                    offset = start % len(BLOCK)
                    amount = min(64 * 1024, limit - start, len(BLOCK) - offset)
                    chunk = BLOCK[offset:offset + amount]
                    if mode == "corrupt" and not probe and start <= SMALL_SIZE // 2 < start + amount:
                        corrupt = SMALL_SIZE // 2 - start
                        chunk = chunk[:corrupt] + bytes([chunk[corrupt] ^ 1]) + chunk[corrupt + 1:]
                    self.wfile.write(chunk)
                    start += amount
                    if mode == "slow" and not probe:
                        time.sleep(0.025)
        except (ConnectionError, TimeoutError):
            # Disconnects are expected for cancellation/rejection; never log paths.
            pass
        finally:
            if body:
                with fixture.lock:
                    fixture.active -= 1
