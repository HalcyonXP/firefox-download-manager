"""Qualification-harness policies and real synthetic HTTP, not Firefox E2E."""
import hashlib
import http.client
import os
from pathlib import Path
import socket
import struct
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

    def test_both_boundary_probes_bypass_worker_faults_and_body_gate(self):
        fixture = Fixture()
        fixture.slow_body.clear()
        try:
            for mode in ("bad-range", "change", "truncate", "slow"):
                for offset in (0, SMALL_SIZE - 1):
                    connection = http.client.HTTPConnection("127.0.0.1", fixture.server.server_port, timeout=3)
                    try:
                        connection.request("GET", "/" + mode, headers={"Range": f"bytes={offset}-{offset}"})
                        response = connection.getresponse()
                        self.assertEqual(response.status, 206)
                        self.assertEqual(response.getheader("ETag"), '\"fixture-v1\"')
                        self.assertEqual(response.getheader("Content-Range"), f"bytes {offset}-{offset}/{SMALL_SIZE}")
                        self.assertEqual(response.read(), bytes([offset % 256]))
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

    def test_real_peer_reset_before_headers_is_expected_but_unexpected_errors_refuse(self):
        fixture = Fixture()
        client = socket.create_connection(("127.0.0.1", fixture.server.server_port), timeout=3)
        try:
            deadline = time.monotonic() + 3
            while not fixture.server.handlers:
                self.assertLess(time.monotonic(), deadline, "request never entered ownership")
                time.sleep(0.005)
            client.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER, struct.pack("hh" if os.name == "nt" else "ii", 1, 0))
        finally:
            client.close()
            fixture.close()
        self.assertEqual(fixture.server.error_kinds, {"peer-disconnect": 1})
        self.assertEqual(fixture.server.errors, 0)
        fixture = Fixture()
        try:
            raise ValueError("synthetic unexpected handler failure")
        except ValueError:
            fixture.server.handle_error()
        with self.assertRaisesRegex(RuntimeError, "unexpected"):
            fixture.close()
        self.assertEqual(fixture.server.fileno(), -1)

    @unittest.skipUnless(os.name == "nt", "Windows Firefox harness")
    def test_automation_bounds_correlation_and_unowned_context_refusal(self):
        from qualification.firefox import Firefox
        class Stream:
            def __init__(self, data):
                self.data = data
            def recv(self, count):
                result, self.data = self.data[:count], self.data[count:]
                return result
            def sendall(self, _):
                pass
        driver = Firefox(Path("unused"), Path("unused"), {})
        with self.assertRaisesRegex(RuntimeError, "no owned browser authority"):
            driver.chrome("return true")
        with self.assertRaisesRegex(RuntimeError, "no owned browser authority"):
            driver.command("WebDriver:Navigate")
        for encoded in (b":", b"0:", b"-1:", b"99999999:", b"2:{"):
            driver.connection = Stream(encoded)
            with self.assertRaises(RuntimeError):
                driver.receive()
        data = b"[1,99,null,{}]"
        driver.connection = Stream(str(len(data)).encode() + b":" + data)
        with self.assertRaisesRegex(RuntimeError, "uncorrelated"):
            driver.command("WebDriver:NewSession")
        driver.close()  # No process was started; does not inspect/stop any browser.

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
