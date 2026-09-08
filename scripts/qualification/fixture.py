"""Bounded synthetic loopback bytes for exact-artifact tests; never logs requests."""
from collections import Counter
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import hashlib
import re
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


class Fixture:
    def __init__(self, large_size=2 * 1024**3):
        self.large_size = large_size
        self.lock = threading.Lock()
        self.requests = Counter()
        self.active = 0
        self.peak = 0
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.server.fixture = self
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def url(self, mode):
        return f"http://127.0.0.1:{self.server.server_port}/{mode}"

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=10)
        deadline = time.monotonic() + 10
        while self.active and time.monotonic() < deadline:
            time.sleep(0.02)
        if self.thread.is_alive() or self.active:
            raise RuntimeError("owned fixture did not finish its bounded shutdown")


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

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
            match = re.fullmatch(r"bytes=([0-9]+)-([0-9]+)", self.headers["Range"])
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
