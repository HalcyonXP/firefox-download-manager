"""Modeled combined-driver ownership plus real build-only XPI checks; no Firefox/registration."""
import copy
import json
from pathlib import Path
from types import SimpleNamespace
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch
import zipfile

from qualification.browser_installed import BrowserInstalledRun, BODY, PAYLOADS, build_probe, settled, restart_ui, pending_confirmation, explicit_continue, phase_controls, CONTINUE_PROMPT, ELEMENT, cross_origin_handler, CaptureHandler, Fixture
from qualification.installed import InstalledRun


class BrowserInstalledTests(unittest.TestCase):
    def test_correlated_completion_requires_strict_positive_observations(self):
        record = {"qualification": False, "connected": True, "phaseMetadataAvailable": True, "overflow": False, "blocked": False,
                  "pending": [], "taskCount": 1, "tasks": [{"state": "completed", "bytes": len(BODY), "phase": "committed"}],
                  "records": [{"request": 1, "stage": "decision", "cancelled": True},
                              {"request": 1, "stage": "terminal", "cancelled": True}]}
        self.assertTrue(settled(record, captured=True))
        for field, value in (("qualification", True), ("pending", ["intent"]), ("taskCount", True),
                             ("taskCount", 2), ("phaseMetadataAvailable", False), ("overflow", True), ("connected", False), ("records", [])):
            with self.subTest(field=field, value=value):
                with self.assertRaises(RuntimeError): settled({**record, field: value}, captured=True)
        for field, value in (("request", True), ("request", 2), ("cancelled", 1), ("cancelled", False)):
            mutated = copy.deepcopy(record); mutated["records"][1][field] = value
            with self.assertRaises(RuntimeError): settled(mutated, captured=True)
        self.assertTrue(settled({**record, "records": []}, captured=False))
        with self.assertRaises(RuntimeError): settled(record, captured=False)

    def test_uncertain_intent_cannot_be_interpreted_as_committed_completion(self):
        pending = {"qualification": False, "connected": True, "phaseMetadataAvailable": True, "overflow": False, "blocked": False,
                   "pending": ["intent"], "taskCount": 1, "tasks": [{"state": "queued", "bytes": 0, "phase": "prepared"}], "records": []}
        self.assertTrue(pending_confirmation(pending, captured=False))
        with self.assertRaises(RuntimeError): settled(pending, captured=False)
        with self.assertRaises(RuntimeError): pending_confirmation({**pending, "pending": []}, captured=False)
        with self.assertRaises(RuntimeError):
            BrowserInstalledRun(Path("unused"), Path("unused"), Path("unused"), fault="completed", scenario="missing-terminal")

    def test_browser_is_retained_before_start_can_fail(self):
        run = BrowserInstalledRun(Path("unused"), Path("unused"), Path("unused"))
        run.environment = {}
        browser = Mock(); browser.start.side_effect = RuntimeError("synthetic startup")
        with patch("qualification.browser_installed.Firefox", return_value=browser):
            with self.assertRaises(RuntimeError):
                run.open_browser(Mock(), Path("unused"), Path("unused"), Path("unused"), ["http://127.0.0.1"])
        self.assertEqual(run.browsers, [browser])
        run.close_resources()
        browser.close.assert_called_once()
        browser.process.wait.assert_called_once_with(timeout=0)

    def test_failed_browser_cleanup_prevents_uninstall_but_not_owned_quit(self):
        run = InstalledRun(Path("unused"), Path("unused"))
        browser = Mock(); browser.close.side_effect = RuntimeError("synthetic unresolved browser")
        run.browsers = [browser]; run.install_requested = True
        run.owner = Mock(joined=False); run.uninstall = Mock()
        run.failure_cleanup()
        run.uninstall.assert_not_called()
        run.owner.quiesce.assert_called_once()
        run.owner.retire.assert_called_once()
        self.assertIn("browser", run.cleanup_errors)

    def test_failure_retention_includes_browser_processes(self):
        run = InstalledRun(Path("unused"), Path("unused"))
        process = Mock(); process.poll.return_value = None
        browser = Mock(process=process)
        browser.close.side_effect = lambda: setattr(process.poll, "return_value", 0)
        run.browsers = [browser]
        with patch("qualification.installed.time.sleep"), patch("builtins.print"):
            run.hold_failed_owners()
        browser.close.assert_called_once()
        self.assertGreaterEqual(process.wait.call_count, 1)

    def test_missing_failed_or_unjoined_browser_cannot_publish_success(self):
        run = BrowserInstalledRun(Path("unused"), Path("unused"), Path("unused"))
        with self.assertRaises(RuntimeError): run.additional_evidence()
        for closed, code in ((False, 0), (True, 1), (True, None)):
            run.browsers = [SimpleNamespace(closed=True, process=SimpleNamespace(returncode=0)),
                            SimpleNamespace(closed=closed, process=SimpleNamespace(returncode=code))]
            with self.assertRaises(RuntimeError): run.additional_evidence()

    def test_restart_ui_navigation_cannot_replace_the_inspector(self):
        active = ["inspector"]
        pages = {"inspector": "inspect.html"}
        browser = Mock()
        def command(name, arguments):
            if name == "WebDriver:NewWindow":
                pages["test"] = "about:blank"
                return {"handle": "test"}
            self.assertEqual(name, "WebDriver:SwitchToWindow")
            active[0] = arguments["handle"]
        browser.command.side_effect = command
        browser.navigate.side_effect = lambda url: pages.__setitem__(active[0], url)
        browser.manager = "manager.html"
        browser.task.return_value = "task-fixture"
        restart_ui(browser, "task-fixture", "completed")
        self.assertEqual(pages["inspector"], "inspect.html")
        self.assertEqual(pages["test"], "manager.html")

    def test_explicit_continuation_checks_actual_warning_before_accepting(self):
        browser = Mock(); browser.script.return_value = {ELEMENT: "owned-button"}
        browser.command.side_effect = [None, {"value": CONTINUE_PROMPT}, None]
        explicit_continue(browser)
        self.assertEqual([call.args[0] for call in browser.command.call_args_list],
                         ["WebDriver:ElementClick", "WebDriver:GetAlertText", "WebDriver:AcceptAlert"])
        self.assertIn(CONTINUE_PROMPT, Path("extension/src/manager.ts").read_text(encoding="utf-8"))
        browser.reset_mock(); browser.command.side_effect = [None, {"value": "unexpected warning"}]
        with self.assertRaises(RuntimeError): explicit_continue(browser)
        self.assertNotIn("WebDriver:AcceptAlert", [call.args[0] for call in browser.command.call_args_list])

    def test_phase_controls_refuse_mutating_buttons_or_missing_owned_row(self):
        browser = Mock(); browser.script.return_value = ["Open folder"]
        phase_controls(browser)
        for labels in ([], ["Start", "Open folder"], ["Remove history", "Open folder"]):
            browser.script.return_value = labels
            with self.assertRaises(RuntimeError): phase_controls(browser)

    def test_cross_origin_fixture_retains_two_servers_and_exact_query(self):
        import http.client
        from urllib.parse import urlsplit
        from contextlib import closing
        owners = []
        try:
            target = Fixture(handler=CaptureHandler, owners=owners)
            source = Fixture(handler=cross_origin_handler(target), owners=owners)
            self.assertNotEqual(source.url(""), target.url(""))
            address = urlsplit(source.url("redirect?fixture=a%2Fb&x=1&x=2"))
            with closing(http.client.HTTPConnection(address.hostname, address.port, timeout=5)) as connection:
                connection.request("GET", address.path+"?"+address.query)
                with closing(connection.getresponse()) as response:
                    self.assertEqual(response.status, 302)
                    self.assertEqual(response.getheader("Location"), target.url("attachment?fixture=a%2Fb&x=1&x=2"))
                    self.assertEqual(response.read(1), b"")
            address = urlsplit(target.url("attachment?fixture=a%2Fb&x=1&x=2"))
            with closing(http.client.HTTPConnection(address.hostname, address.port, timeout=5)) as connection:
                connection.request("GET", address.path+"?"+address.query)
                with closing(connection.getresponse()) as response:
                    self.assertEqual(response.status, 200)
                    self.assertEqual(response.getheader("Content-Length"), str(len(BODY)))
                    self.assertEqual(response.read(len(BODY)+1), BODY)
            self.assertEqual(len(owners), 2)
        finally:
            for fixture in owners: fixture.close()
        self.assertTrue(all(f.closed and not f.thread.is_alive() for f in owners))

    def test_compiler_deadline_keeps_exact_parent_even_with_broken_status_sink(self):
        process = Mock(); process.wait.side_effect = [subprocess.TimeoutExpired("owned-node", 60), 0]
        with patch("qualification.browser_installed.subprocess.Popen", return_value=process), \
             patch("builtins.print", side_effect=OSError("sink")):
            with self.assertRaises(subprocess.TimeoutExpired): build_probe(Path("unused"))
        self.assertEqual(process.wait.call_count, 2)
        process.kill.assert_not_called()
        process.terminate.assert_not_called()

    def test_real_diagnostic_build_is_exclusive_loopback_only_and_unselected(self):
        manifest_path = Path("extension/src/manifest.json")
        original = manifest_path.read_bytes()
        with tempfile.TemporaryDirectory(prefix="dm-handoff-build-") as temporary:
            root = Path(temporary).resolve()
            xpi = build_probe(root)
            with zipfile.ZipFile(xpi) as archive:
                self.assertEqual(set(archive.namelist()), PAYLOADS)
                manifest = json.loads(archive.read("manifest.json"))
                self.assertEqual(manifest["host_permissions"], ["http://127.0.0.1/*"])
                self.assertNotIn("optional_host_permissions", manifest)
                self.assertNotIn("cookies", manifest["permissions"])
                self.assertEqual(manifest["content_scripts"][0]["matches"], ["http://127.0.0.1/page"])
                self.assertIn(b"ordinary-download-click", archive.read("click.js"))
                self.assertIn(b"commit_handoff", archive.read("background.js"))
            with self.assertRaises(RuntimeError): build_probe(root)
        self.assertEqual(manifest_path.read_bytes(), original)
        builder = Path("scripts/build-extension.mjs").read_text(encoding="utf-8")
        self.assertNotIn("diagnostic", builder)
        self.assertNotIn("webRequest", json.loads(original)["permissions"])


if __name__ == "__main__":
    unittest.main()
