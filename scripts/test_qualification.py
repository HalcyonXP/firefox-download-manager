"""Qualification-harness policies and real synthetic HTTP, not Firefox E2E."""
import hashlib
import http.client
import os
from pathlib import Path
import socket
import tempfile
import time
import unittest

from qualification.fixture import Fixture, SMALL_SIZE, expected_sha256
from qualification.native import qualify


class QualificationPolicy(unittest.TestCase):
    def test_fixture_digest_matches_independently_constructed_vectors(self):
        for size in (0, 1, 255, 256, 1024 * 1024 + 19, SMALL_SIZE):
            data = (bytes(range(256)) * (size // 256 + 1))[:size]
            self.assertEqual(expected_sha256(size), hashlib.sha256(data).hexdigest())

    def test_actual_http_exact_cross_block_ranges_and_adversarial_shapes(self):
        fixture = Fixture()
        try:
            start, end = 1024 * 1024 - 13, 1024 * 1024 + 19
            for mode in ("range", "bad-range", "change"):
                connection = http.client.HTTPConnection("127.0.0.1", fixture.server.server_port, timeout=3)
                try:
                    connection.request("GET", "/" + mode, headers={"Range": f"bytes={start}-{end}"})
                    response = connection.getresponse()
                    self.assertEqual(response.status, 206)
                    self.assertEqual(response.read(), bytes(i % 256 for i in range(start, end + 1)))
                    reported = start + 1 if mode == "bad-range" else start
                    self.assertEqual(response.getheader("Content-Range"), f"bytes {reported}-{end}/{SMALL_SIZE}")
                    self.assertEqual(response.getheader("ETag"), '"fixture-v2"' if mode == "change" else '"fixture-v1"')
                finally:
                    connection.close()
            connection = http.client.HTTPConnection("127.0.0.1", fixture.server.server_port, timeout=3)
            try:
                connection.request("GET", "/truncate", headers={"Range": "bytes=0-63"})
                with self.assertRaises(http.client.IncompleteRead) as caught:
                    connection.getresponse().read()
                self.assertEqual(caught.exception.partial, bytes(range(32)))
            finally:
                connection.close()
        finally:
            fixture.close()

    def test_incomplete_header_is_owned_and_joined_on_shutdown(self):
        fixture = Fixture()
        client = socket.create_connection(("127.0.0.1", fixture.server.server_port), timeout=3)
        try:
            client.sendall(b"GET /range HTTP/1.1\r\nHost: fixture\r\n")
            deadline = time.monotonic() + 3
            while True:
                with fixture.server.ownership:
                    if fixture.server.handlers:
                        break
                self.assertLess(time.monotonic(), deadline, "request never entered ownership")
                time.sleep(0.005)
        finally:
            fixture.close()
            client.close()
        self.assertFalse(fixture.thread.is_alive())
        self.assertTrue(all(not t.is_alive() for t, _ in fixture.server.handlers))

    @unittest.skipUnless(os.name == "nt", "Windows harness preflight")
    def test_reports_are_new_confined_and_refused_before_process_launch(self):
        with tempfile.TemporaryDirectory() as outside:
            with self.assertRaisesRegex(RuntimeError, "report must be beneath artifacts"):
                qualify(Path("not-a-package"), Path(outside) / "report.json", 0)
        artifacts = Path(__file__).resolve().parents[1] / "artifacts"
        artifacts.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=artifacts, prefix="qualification-policy-") as owned:
            report = Path(owned) / "report.json"
            report.write_bytes(b"previous evidence")
            with self.assertRaisesRegex(RuntimeError, "use a new JSON report"):
                qualify(Path("not-a-package"), report, 0)
            self.assertEqual(report.read_bytes(), b"previous evidence")


if __name__ == "__main__":
    unittest.main()
