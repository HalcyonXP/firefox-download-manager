"""API probe policies and owned HTTP only; never launches Firefox or changes registration."""
from contextlib import closing
import copy
import http.client
import json
from pathlib import Path
import tempfile
import sys
import unittest
from unittest import mock
from types import SimpleNamespace
import zipfile

from qualification.capture import BODY, CaptureHandler, build_probe, inspect_case, message, require_joined_success
from qualification.fixture import Fixture


def observations(kind):
    capture = kind in ("direct", "redirect")
    route = "attachment" if kind == "redirect" else "direct" if kind == "allow-direct" else kind
    attachment = capture or kind == "allow-direct"
    response = {"stage": "headers", "request": 1, "route": route, "method": "POST" if kind == "post" else "GET",
                "type": "sub_frame" if kind == "frame" else "main_frame", "topFrame": kind != "frame",
                "private": False, "store": "default", "status": 200, "attachment": attachment,
                "correlated": attachment, "cancel": capture, "document": "other", "origin": "page"}
    records = [response, {"stage": "error" if capture else "completed", "request": 1, "errorKind": "NS_ERROR_ABORT"}]
    if attachment:
        records.append({"stage": "click", "route": "direct" if kind == "allow-direct" else kind, "trusted": True})
    if kind == "redirect":
        records.append({"stage": "redirect", "request": 1, "target": "attachment"})
    return {"records": records, "overflow": False}


class CaptureProbePolicy(unittest.TestCase):
    def test_acceptance_requires_correlated_terminal_context_and_trusted_click(self):
        for kind in ("direct", "redirect", "navigation", "post", "frame", "allow-direct"):
            snapshot = observations(kind)
            self.assertEqual(inspect_case(snapshot, kind)["case"], kind)
            for field, wrong in (("private", "missing"), ("store", "missing"), ("store", "other"),
                                 ("status", 302), ("cancel", not snapshot["records"][0]["cancel"]),
                                 ("method", "other"), ("type", "other"), ("topFrame", kind == "frame")):
                changed = copy.deepcopy(snapshot)
                changed["records"][0][field] = wrong
                with self.assertRaises(RuntimeError):
                    inspect_case(changed, kind)
            snapshot["records"][1]["request"] = 2
            with self.assertRaises(RuntimeError):
                inspect_case(snapshot, kind)
        for index, field, wrong in ((0, "correlated", False), (1, "errorKind", "other"), (2, "trusted", False), (3, "request", 2)):
            snapshot = observations("redirect")
            snapshot["records"][index][field] = wrong
            with self.assertRaises(RuntimeError):
                inspect_case(snapshot, "redirect")
        with self.assertRaises(RuntimeError):
            inspect_case({**observations("direct"), "overflow": True}, "direct")

    def test_failed_but_joined_browser_exit_cannot_pass_the_nominal_probe(self):
        browser = SimpleNamespace(closed=True, process=SimpleNamespace(returncode=0))
        fixture = SimpleNamespace(closed=True)
        require_joined_success(browser, [fixture])
        for result in (None, 1, -1):
            browser.process.returncode = result
            with self.assertRaisesRegex(RuntimeError, "successful browser exit"):
                require_joined_success(browser, [fixture])
        browser.process.returncode = 0
        fixture.closed = False
        with self.assertRaises(RuntimeError):
            require_joined_success(browser, [fixture])

    def test_probe_identity_and_permissions_cannot_select_native_host_or_remote_sites(self):
        with tempfile.TemporaryDirectory(prefix="dm capture metadata ") as path:
            first, identity = build_probe(Path(path))
            with zipfile.ZipFile(first) as archive:
                self.assertEqual(set(archive.namelist()), {"manifest.json", "inspect.html", "background.js", "click.js"})
                manifest = json.loads(archive.read("manifest.json"))
            self.assertNotEqual(identity, "download-manager@halcyonxp.local")
            self.assertEqual(manifest["permissions"], ["webRequest", "webRequestBlocking"])
            self.assertEqual(manifest["host_permissions"], ["http://127.0.0.1/*"])
            self.assertEqual(manifest["incognito"], "not_allowed")
            with self.assertRaises(FileExistsError):
                build_probe(Path(path))

    def test_snapshot_switches_tabs_without_navigating_or_cancelling_test_request(self):
        calls = []
        class Browser:
            def command(self, name, args=None):
                calls.append((name, args))
                return "test" if name == "WebDriver:GetWindowHandle" else None
            def script(self, *_):
                return {"records": []}
        self.assertEqual(message(Browser(), "inspector", {"action": "snapshot"}), {"records": []})
        self.assertEqual(calls, [("WebDriver:GetWindowHandle", None),
                                ("WebDriver:SwitchToWindow", {"handle": "inspector"}),
                                ("WebDriver:SwitchToWindow", {"handle": "test"})])

    def test_failed_start_and_failed_diagnostics_still_retire_owned_actors(self):
        from qualification import capture
        for fail_close in (False, True):
            retained = []
            original_open = Path.open
            def fail_diagnostic(path, *args, **kwargs):
                if path.name == "failure.private.json":
                    raise OSError("synthetic diagnostic sink failure")
                return original_open(path, *args, **kwargs)
            def start_fixture(*args, **kwargs):
                result = Fixture(*args, **kwargs)
                retained.append(result)
                return result
            browser = mock.Mock()
            browser.start.side_effect = RuntimeError("synthetic startup failure")
            if fail_close:
                browser.close.side_effect = [RuntimeError("synthetic close observation failure"), None]
            with tempfile.TemporaryDirectory(prefix="dm capture failure ") as temporary:
                root = Path(temporary).resolve()
                (root / ".git").mkdir()
                (root / "artifacts").mkdir()
                with mock.patch.multiple(capture, ROOT=root, ARTIFACTS=root / "artifacts"), \
                     mock.patch.object(capture, "closed_apps"), mock.patch.object(capture, "absent_registration"), \
                     mock.patch.object(capture, "new_report", side_effect=lambda path: path), \
                     mock.patch.object(capture, "Firefox", return_value=browser), \
                     mock.patch.object(capture, "Fixture", side_effect=start_fixture), \
                     mock.patch.object(capture, "write_report") as report, mock.patch("builtins.print"), \
                     mock.patch.object(Path, "open", fail_diagnostic):
                    with self.assertRaisesRegex(RuntimeError, "API probe failed"):
                        capture.run(Path(sys.executable), root / "artifacts/result.json")
                    report.assert_not_called()
                self.assertEqual(browser.close.call_count, 2 if fail_close else 1)
                self.assertEqual(len(retained), 1)
                self.assertTrue(retained[0].closed)
                self.assertFalse(retained[0].thread.is_alive())

    def test_owned_redirect_preserves_query_and_attachment_has_bounded_bytes(self):
        fixture = Fixture(handler=CaptureHandler)
        try:
            for path, status in (("/redirect?fixture=a%2Fb&x=1&x=2", 302),
                                 ("/redirect?fixture=a%2Fb&x=2&x=1", 404),
                                 ("/attachment?fixture=a%2Fb&x=1&x=2", 200)):
                with closing(http.client.HTTPConnection("127.0.0.1", fixture.server.server_port, timeout=5)) as connection:
                    connection.request("GET", path)
                    response = connection.getresponse()
                    self.assertEqual(response.status, status)
                    if status == 302:
                        self.assertEqual(response.getheader("Location"), "/attachment?fixture=a%2Fb&x=1&x=2")
                    data = response.read(len(BODY) + 1)
                    self.assertEqual(data, BODY if status == 200 else b"")
        finally:
            fixture.close()
        self.assertTrue(fixture.closed)


if __name__ == "__main__":
    unittest.main()
