"""Bounded synthetic loopback bytes for exact-artifact tests; never logs requests."""
from collections import Counter
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import hashlib
import re
import socket
import threading
import time
from urllib.parse import urlsplit

BLOCK = bytes(range(256)) * 4096  # 1 MiB, reused for arbitrarily large fixtures.
SMALL_SIZE = 8 * 1024 * 1024


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
        with self.ownership:
            self.errors += 1

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
        if any(t.is_alive() for t, _ in handlers) or self.errors:
            raise RuntimeError("owned fixture handler failed or did not join")


class Fixture:
    def __init__(self, large_size=2 * 1024**3):
        self.large_size = large_size
        self.lock = threading.Lock()
        self.requests = Counter()
        self.active = 0
        self.peak = 0
        self.server = BoundedServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.server.fixture = self
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def url(self, mode):
        return f"http://127.0.0.1:{self.server.server_port}/{mode}"

    def close(self):
        self.server.shutdown()
        self.thread.join(timeout=10)
        self.server.join_owned_handlers()
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
        if mode not in {"range", "single", "bad-range", "change", "truncate", "slow", "large"}:
            self.send_error(404)
            return
        self.connection.settimeout(10)
        size = fixture.large_size if mode == "large" else SMALL_SIZE
        start, end = 0, size - 1
        ranged = False
        if body and mode != "single" and self.headers.get("Range"):
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
        probe = ranged and start == end == 0
        with fixture.lock:
            fixture.requests[(mode, self.command, "probe" if probe else "body")] += 1
            if body:
                fixture.active += 1
                fixture.peak = max(fixture.peak, fixture.active)
        try:
            self.send_response(206 if ranged else 200)
            self.send_header("Content-Length", str(end - start + 1))
            self.send_header("Accept-Ranges", "bytes")
            self.send_header("ETag", '"fixture-v2"' if mode == "change" and ranged and not probe else '"fixture-v1"')
            self.send_header("Connection", "close")
            if ranged:
                reported = start + 1 if mode == "bad-range" and not probe else start
                self.send_header("Content-Range", f"bytes {reported}-{end}/{size}")
            self.end_headers()
            if body:
                limit = start + (end - start + 1) // 2 if mode == "truncate" and not probe else end + 1
                while start < limit:
                    offset = start % len(BLOCK)
                    amount = min(64 * 1024, limit - start, len(BLOCK) - offset)
                    self.wfile.write(BLOCK[offset:offset + amount])
                    start += amount
                    if mode == "slow" and not probe:
                        time.sleep(0.025)
        except (ConnectionError, TimeoutError, OSError):
            # Disconnects are expected for cancellation/rejection; never log paths.
            pass
        finally:
            if body:
                with fixture.lock:
                    fixture.active -= 1
