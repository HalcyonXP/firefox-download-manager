"""Qualification-harness policies and real synthetic HTTP, not Firefox E2E."""
import hashlib
import http.client
import importlib.util
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import uuid
from types import SimpleNamespace
from unittest import mock

from qualification.fixture import Fixture, Handler, PREFIX_SIZE, SMALL_SIZE, expected_sha256
from qualification.native import Host, exact_prefix, large, qualify, task_status
from qualification.support import ARTIFACTS, LIMIT, bounded_json, leased_parent, new_report, write_report


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
        fixture.retained_body.clear()
        try:
            for mode in ("bad-range", "change", "truncate", "slow", "worker-ignored", "missing-range",
                         "out-of-bounds", "corrupt", "retained-resume"):
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

    def test_real_http_empty_unknown_identity_worker_fault_and_retained_shapes(self):
        fixture = Fixture()
        fixture.retained_body.clear()
        try:
            for mode, tag in (("weak", 'W/"fixture-v1"'), ("missing-validator", None)):
                connection = http.client.HTTPConnection("127.0.0.1", fixture.server.server_port, timeout=3)
                try:
                    connection.request("GET", "/" + mode, headers={"Range": "bytes=0-0"})
                    response = connection.getresponse()
                    self.assertEqual(response.status, 206)
                    self.assertEqual(response.getheader("ETag"), tag)
                    self.assertEqual(response.read(), b"\0")
                finally:
                    connection.close()
            for mode in ("empty", "unknown", "worker-ignored", "missing-range", "out-of-bounds", "corrupt", "retained-resume"):
                connection = http.client.HTTPConnection("127.0.0.1", fixture.server.server_port, timeout=3)
                try:
                    offset = SMALL_SIZE // 2 if mode == "corrupt" else 0
                    connection.request("GET", "/" + mode, headers={"Range": f"bytes={offset}-{offset + 15}"})
                    response = connection.getresponse()
                    if mode == "empty":
                        self.assertEqual(response.status, 416)
                        self.assertEqual(response.getheader("Content-Range"), "bytes */0")
                        self.assertEqual(response.read(), b"")
                    elif mode in {"unknown", "worker-ignored"}:
                        self.assertEqual(response.status, 200)
                        self.assertIsNone(response.getheader("Content-Range"))
                        if mode == "unknown":
                            self.assertIsNone(response.getheader("Content-Length"))
                        self.assertEqual(hashlib.sha256(response.read()).hexdigest(), expected_sha256(SMALL_SIZE))
                    else:
                        self.assertEqual(response.status, 206)
                        expected = bytes(range(16))
                        if mode == "corrupt":
                            expected = b"\1" + expected[1:]
                        elif mode == "missing-range":
                            self.assertIsNone(response.getheader("Content-Range"))
                        elif mode == "out-of-bounds":
                            self.assertEqual(response.getheader("Content-Range"), f"bytes 0-{SMALL_SIZE}/{SMALL_SIZE}")
                        self.assertEqual(response.read(), expected)
                finally:
                    connection.close()
            connection = http.client.HTTPConnection("127.0.0.1", fixture.server.server_port, timeout=3)
            try:
                connection.request("GET", "/retained-resume", headers={"Range": f"bytes={PREFIX_SIZE}-{PREFIX_SIZE + 15}"})
                response = connection.getresponse()
                self.assertEqual(response.status, 206)
                with fixture.lock:
                    self.assertIn((PREFIX_SIZE, PREFIX_SIZE + 15), fixture.retained_requests["retained-resume"])
                fixture.retained_body.set()
                self.assertEqual(response.read(), bytes(range(16)))
            finally:
                connection.close()
        finally:
            fixture.close()

    def test_gate_deadline_and_non_connection_os_errors_invalidate_fixture_success(self):
        class FailedBody(Handler):
            def end_headers(self):
                super().end_headers()
                def fail(_):
                    raise OSError("synthetic unrelated fixture I/O failure")
                self.wfile.write = fail
        for handler in (None, FailedBody):
            fixture = Fixture(handler=handler)
            if handler is None:
                fixture.slow_body = mock.Mock()
                fixture.slow_body.wait.return_value = False
            connection = http.client.HTTPConnection("127.0.0.1", fixture.server.server_port, timeout=3)
            try:
                connection.request("GET", "/slow" if handler is None else "/range", headers={"Range": "bytes=0-15"})
                with self.assertRaises(http.client.IncompleteRead):
                    connection.getresponse().read()
            finally:
                connection.close()
                with self.assertRaisesRegex(RuntimeError, "unexpected"):
                    fixture.close()
            self.assertEqual(fixture.server.errors, 1)
            self.assertEqual(fixture.server.fileno(), -1)

    def test_status_diagnostics_do_not_echo_untrusted_fields(self):
        self.assertEqual(task_status({"state": "failed", "error": {"code": "HTTP_STATUS", "message": "synthetic-private-canary"}}),
                         {"state": "failed", "code": "HTTP_STATUS"})
        value = task_status({"state": "synthetic-private-canary", "error": {"code": ["synthetic-private-canary"]}})
        self.assertEqual(value, {"state": "other", "code": "other"})

    @unittest.skipUnless(os.name == "nt", "Windows harness assertions")
    def test_optimized_python_cannot_issue_qualification_evidence(self):
        for module, arguments in (("native", "Path('not-a-package'),Path('artifacts/optimized.json'),0"),
                                  ("firefox", "Path('not-a-package'),Path('not-firefox'),Path('artifacts/optimized.json')")):
            code = ("import sys;from pathlib import Path;sys.path.insert(0,'scripts');"
                    f"from qualification.{module} import qualify;qualify({arguments})")
            result = subprocess.run([sys.executable, "-O", "-c", code], capture_output=True, timeout=15)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b"qualification requires enabled assertions", result.stderr)

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

    def test_durable_prefix_is_exact_half_open_coverage_not_an_aggregate_counter(self):
        exact_prefix([{"start": 0, "end": PREFIX_SIZE}])
        exact_prefix([{"start": PREFIX_SIZE // 2, "end": PREFIX_SIZE}, {"start": 0, "end": PREFIX_SIZE // 2}])
        for ranges in ([], [{"start": 1, "end": PREFIX_SIZE}], [{"start": 0, "end": PREFIX_SIZE - 1}],
                       [{"start": 0, "end": PREFIX_SIZE + 1}],
                       [{"start": 0, "end": 1}, {"start": 0, "end": PREFIX_SIZE - 1}],
                       [{"start": 0, "end": 1}, {"start": 2, "end": PREFIX_SIZE}],
                       [{"start": False, "end": PREFIX_SIZE}]):
            with self.assertRaises(AssertionError):
                exact_prefix(ranges)

    def test_resource_api_initialization_failure_still_closes_owned_host(self):
        host = mock.Mock()
        with mock.patch("qualification.native.Host", return_value=host), \
             mock.patch("qualification.native.Measurements", side_effect=RuntimeError("synthetic metric failure")):
            with self.assertRaisesRegex(RuntimeError, "synthetic metric failure"):
                large(Path("unused"), Path("unused"), None, 0)
        host.close.assert_called_once_with()

    @unittest.skipUnless(os.name == "nt", "Windows native cleanup evidence")
    def test_unresolved_fixture_or_native_ownership_preserves_domain_and_refuses_report(self):
        ARTIFACTS.mkdir(exist_ok=True)
        for phase in ("fixture", "owned-native-work"):
            fixture = mock.Mock()
            if phase == "fixture":
                fixture.close.side_effect = RuntimeError("synthetic fixture cleanup failure")
            with tempfile.TemporaryDirectory(dir=ARTIFACTS, prefix="qualification-policy-") as owned:
                root = Path(owned)
                domain = root / "domain"
                domain.mkdir()
                marker = domain / "marker"
                marker.write_bytes(b"preserve owned test domain")
                token = uuid.uuid4()
                ticket = ARTIFACTS.parent / ".git" / f"native28-recovery-{token}.private.json"
                report = root / "report.json"
                def matrix(_package, _root, _fixture, owners):
                    owners.append(SimpleNamespace(closed=phase != "owned-native-work"))
                    return ["synthetic policy"], {}
                try:
                    with mock.patch("qualification.native.subprocess.run"), \
                         mock.patch("qualification.native.evidence_identity", return_value={}), \
                         mock.patch("qualification.native.tempfile.mkdtemp", return_value=str(domain)), \
                         mock.patch("qualification.native.Fixture", return_value=fixture), \
                         mock.patch("qualification.native.matrix", side_effect=matrix), \
                         mock.patch("qualification.native.retention_matrix", return_value=[]), \
                         mock.patch("qualification.native.uuid.uuid4", return_value=token):
                        with self.assertRaisesRegex(RuntimeError, "owned native domain preserved"):
                            qualify(Path("not-a-package"), report, 0)
                    self.assertFalse(report.exists())
                    self.assertEqual(marker.read_bytes(), b"preserve owned test domain")
                    self.assertEqual(bounded_json(ticket), {"owned_domain": str(domain), "cleanup_phase": phase})
                    fixture.close.assert_called_once_with()
                finally:
                    ticket.unlink(missing_ok=True)  # Retained exact, unique test ticket only.

    @staticmethod
    def install_harness():
        spec = importlib.util.spec_from_file_location("qualification_install_policy", ARTIFACTS.parent / "scripts/test-package-install.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    @unittest.skipUnless(os.name == "nt", "Windows installer-harness boundary, no registry mutation")
    def test_installer_report_preflight_and_unrecorded_binding_refuse_before_mutation(self):
        module = self.install_harness()
        with mock.patch.object(module, "closed_apps", side_effect=AssertionError("preflight order")):
            with self.assertRaisesRegex(RuntimeError, "unsafe report spelling"):
                module.test(Path("not-a-package"), ARTIFACTS / "install-ads:report.json")
        with mock.patch.object(module, "closed_apps"), \
             mock.patch.object(module, "registration", return_value="unrecorded test binding"), \
             mock.patch.object(module, "delete_owned_registration") as delete, \
             mock.patch.object(module, "all_views_absent") as absent, \
             mock.patch.object(module.shutil, "rmtree") as remove:
            with self.assertRaisesRegex(RuntimeError, "unrecorded registration"):
                module.cleanup_owned(Path("unused"), {"known test binding"})
            delete.assert_not_called()
            absent.assert_not_called()
            remove.assert_not_called()
        code = ("import importlib.util;from pathlib import Path;"
                "s=importlib.util.spec_from_file_location('install','scripts/test-package-install.py');"
                "m=importlib.util.module_from_spec(s);s.loader.exec_module(m);"
                "m.test(Path('not-a-package'),Path('artifacts/optimized-install.json'))")
        result = subprocess.run([sys.executable, "-O", "-c", "import sys;sys.path.insert(0,'scripts');" + code],
                                capture_output=True, timeout=15)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"qualification requires enabled assertions", result.stderr)

    @unittest.skipUnless(os.name == "nt", "Windows installed-path proof with synthetic files only")
    def test_installer_binding_requires_exact_generation_and_both_copied_payloads(self):
        module = self.install_harness()
        ARTIFACTS.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=ARTIFACTS, prefix="qualification-policy-") as owned:
            root, package = Path(owned) / "install", Path(owned) / "package"
            generation = str(uuid.uuid4())
            folder = root / generation
            folder.mkdir(parents=True)
            package.mkdir()
            for name in ("download-manager-native-host.exe", "firefox-download-manager.xpi"):
                (package / name).write_bytes(b"synthetic non-executable payload")
                (folder / name).write_bytes((package / name).read_bytes())
            (root / "installation.json").write_text(json.dumps({"current": generation}), encoding="utf-8")
            manifest = folder / "com.halcyonxp.firefox_download_manager.json"
            manifest.write_text(json.dumps({"path": str(folder / "download-manager-native-host.exe"),
                                           "allowed_extensions": ["download-manager@halcyonxp.local"]}), encoding="utf-8")
            self.assertEqual(module.verified_binding(root, package, str(manifest)), generation)
            self.assertEqual(module.verified_binding(root, package, "\\\\?\\" + str(manifest)), generation)
            # Reject namespaces lexically before they can trigger external path resolution.
            with mock.patch.object(Path, "resolve", side_effect=AssertionError("unowned resolution")):
                with self.assertRaisesRegex(RuntimeError, "unverified installed registration"):
                    module.verified_binding(root, package, r"\\unowned-test-host\share\manifest.json")
            (folder / "firefox-download-manager.xpi").write_bytes(b"changed synthetic payload")
            with self.assertRaisesRegex(RuntimeError, "installed payload differs"):
                module.verified_binding(root, package, str(manifest))

    def test_metadata_is_bounded_strict_and_does_not_echo_contents(self):
        with tempfile.TemporaryDirectory() as owned:
            path = Path(owned) / "metadata.json"
            for content in (b"x" * (LIMIT + 1), b'{"synthetic-private-canary":1,"synthetic-private-canary":2}',
                            b'{"x":NaN}', b'\xff', b'[' * 2000):
                path.write_bytes(content)
                with self.assertRaises(RuntimeError) as error:
                    bounded_json(path)
                self.assertNotIn("synthetic-private-canary", str(error.exception))
            path.write_text('{"value":null}', encoding="utf-8")
            self.assertEqual(bounded_json(path), {"value": None})

    @unittest.skipUnless(os.name == "nt", "Windows report and owned-child policies")
    def test_windows_report_spelling_junction_and_ancestor_lease_boundaries(self):
        from qualification.firefox import qualify as qualify_firefox
        ARTIFACTS.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=ARTIFACTS, prefix="qualification-policy-") as owned:
            root = Path(owned)
            for spelling in ("ads:result.json", "NUL.json", "LPT1.more.json", "trailing. /report.json",
                             "../escaped.json", "bidi\u202e.json", "COM\u00b9.json"):
                with self.assertRaisesRegex(RuntimeError, "unsafe report spelling"):
                    qualify(Path("not-a-package"), root / spelling, 0)
                # Refusal precedes even the closed-browser precondition, not just launch.
                with mock.patch("qualification.firefox.closed_apps", side_effect=AssertionError("preflight order")):
                    with self.assertRaisesRegex(RuntimeError, "unsafe report spelling"):
                        qualify_firefox(Path("not-a-package"), Path("not-firefox"), root / spelling)
            target, link = root / "target", root / "alias"
            target.mkdir()
            (target / "marker").write_bytes(b"preserve")
            powershell = Path(os.environ["WINDIR"]) / "System32/WindowsPowerShell/v1.0/powershell.exe"
            subprocess.run([str(powershell), "-NoProfile", "-NonInteractive", "-Command",
                            "New-Item -ItemType Junction -Path $env:DM_TEST_LINK -Target $env:DM_TEST_TARGET | Out-Null"],
                           env={**os.environ, "DM_TEST_LINK": str(link), "DM_TEST_TARGET": str(target)},
                           capture_output=True, timeout=20, check=True)
            try:
                with self.assertRaisesRegex(RuntimeError, "report aliases"):
                    new_report(link / "report.json")
                self.assertEqual((target / "marker").read_bytes(), b"preserve")
            finally:
                link.rmdir()  # Only the test-created junction, never its target contents.
            report = root / "report.json"
            moved = root.with_name(root.name + "-moved")
            try:
                with leased_parent(report):
                    with self.assertRaises(OSError):
                        root.rename(moved)
                root.rename(moved)
            finally:
                # Restore this retained exact test path even if a lease mutation fails.
                if moved.exists() and not root.exists():
                    moved.rename(root)

    @unittest.skipUnless(os.name == "nt", "Windows no-replace report publication")
    def test_report_write_is_bounded_create_new_and_preserves_a_racing_destination(self):
        ARTIFACTS.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=ARTIFACTS, prefix="qualification-policy-") as owned:
            root = Path(owned)
            report = root / "nested/report.json"
            write_report(report, {"scope": "synthetic policy, not artifact evidence"})
            original = report.read_bytes()
            self.assertEqual(json.loads(original)["scope"], "synthetic policy, not artifact evidence")
            with self.assertRaisesRegex(RuntimeError, "new JSON report"):
                write_report(report, {})
            self.assertEqual(report.read_bytes(), original)
            with self.assertRaisesRegex(RuntimeError, "oversized evidence"):
                write_report(root / "large.json", {"value": "x" * LIMIT})
            self.assertFalse((root / "large.json").exists())
            rename = os.rename
            def racing_destination(source, destination):
                destination.write_bytes(b"foreign test marker")
                rename(source, destination)
            with mock.patch("qualification.support.os.rename", side_effect=racing_destination):
                with self.assertRaises(FileExistsError):
                    write_report(root / "raced.json", {"scope": "synthetic"})
            self.assertEqual((root / "raced.json").read_bytes(), b"foreign test marker")
            self.assertEqual(len(list(root.glob(".*.partial"))), 1)

    @unittest.skipUnless(os.name == "nt", "Windows isolated helper-constructor policy")
    def test_reader_start_failure_joins_the_exact_owned_synthetic_child(self):
        # Actual Python child handles, not compiled-helper or browser evidence.
        popen, start = subprocess.Popen, threading.Thread.start
        for failed_start in (1, 2):
            children, readers = [], []
            attempts = 0
            def child(*_, **kwargs):
                process = popen([sys.executable, "-I", "-S", "-c", "import time; time.sleep(60)"], **kwargs)
                children.append(process)
                return process
            def reader_start(reader):
                nonlocal attempts
                attempts += 1
                readers.append(reader)
                if attempts == failed_start:
                    raise RuntimeError("synthetic reader startup failure")
                start(reader)
            with tempfile.TemporaryDirectory() as owned:
                try:
                    with mock.patch("qualification.native.subprocess.Popen", side_effect=child), \
                         mock.patch("qualification.native.threading.Thread.start", new=reader_start):
                        with self.assertRaisesRegex(RuntimeError, "synthetic reader startup failure"):
                            Host(Path("not-a-package"), Path(owned))
                    self.assertEqual(len(children), 1)
                    self.assertIsNotNone(children[0].poll(), "constructor lost its owned child")
                    self.assertTrue(all(not reader.is_alive() for reader in readers))
                    self.assertTrue(all(stream.closed for stream in (children[0].stdin, children[0].stdout, children[0].stderr)))
                finally:
                    for process in children:
                        if process.poll() is None:
                            process.kill()  # Retained test-child handle only, even under mutation.
                            process.wait(timeout=10)
                        for reader in readers:
                            if reader.ident is not None:
                                reader.join(timeout=10)
                        for stream in (process.stdin, process.stdout, process.stderr):
                            stream.close()

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
