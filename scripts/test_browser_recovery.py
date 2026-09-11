"""Fileless recovery-driver models; no browser, profile or registration execution."""
import copy
from collections import Counter
from pathlib import Path
from threading import Lock
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

from qualification.browser_recovery import (ACK_PROMPT, DISCARD_PROMPT, BrowserRecoveryRun,
    ELEMENT, cleanup_button, no_native_transfer, recovery_snapshot)


def snapshot(phase="aborted", pending=None, captured=False):
    return {"qualification": False, "connected": True, "phaseMetadataAvailable": True,
            "blocked": False, "overflow": False, "pending": pending or [], "taskCount": 1,
            "commitReplaced": captured,
            "tasks": [{"phase": phase, "state": "queued" if phase == "prepared" else "cancelled", "bytes": 0}],
            "records": [{"request": 1, "stage": "decision", "cancelled": True},
                        {"request": 1, "stage": "terminal", "cancelled": True}] if captured else []}


class RecoveryTests(unittest.TestCase):
    def test_only_explicit_no_transfer_shapes_are_accepted(self):
        for phase, pending, captured in (("prepared", [], False), ("aborted", ["cancelled"], True), ("aborted", [], False)):
            record = snapshot(phase, pending, captured)
            self.assertTrue(recovery_snapshot(record, phase, pending, captured))
            for key, value in (("qualification", True), ("connected", False), ("taskCount", True),
                               ("taskCount", 2), ("blocked", True), ("overflow", True), ("commitReplaced", not captured)):
                with self.subTest(key=key), self.assertRaises(RuntimeError):
                    recovery_snapshot({**record, key: value}, phase, pending, captured)
            changed = copy.deepcopy(record); changed["tasks"][0]["bytes"] = 1
            with self.assertRaises(RuntimeError): recovery_snapshot(changed, phase, pending, captured)
        changed = snapshot(); changed["tasks"][0]["bytes"] = False
        with self.assertRaises(RuntimeError): recovery_snapshot(changed, "aborted", [])
        with self.assertRaises(RuntimeError): recovery_snapshot(snapshot("committed"), "committed", [])
        changed = snapshot(captured=True); changed["records"][0]["request"] = True
        with self.assertRaises(RuntimeError): recovery_snapshot(changed, "aborted", [], True)

    def test_warning_is_verified_before_accepting_actual_ui_action(self):
        source = Path("extension/src/handoff-actions.ts").read_text(encoding="utf-8")
        for scenario, prompt in (("unlinked", DISCARD_PROMPT), ("aborted-terminal", ACK_PROMPT)):
            self.assertIn(prompt, source)
            browser = Mock(); browser.script.return_value = {ELEMENT: "owned"}
            browser.command.side_effect = [None, {"value": prompt}, None]
            cleanup_button(browser, scenario)
            self.assertEqual([call.args[0] for call in browser.command.call_args_list],
                             ["WebDriver:ElementClick", "WebDriver:GetAlertText", "WebDriver:AcceptAlert"])
            browser.reset_mock(); browser.command.side_effect = [None, {"value": "unexpected"}]
            with self.assertRaises(RuntimeError): cleanup_button(browser, scenario)

    def test_native_output_and_extra_requests_refuse(self):
        destination = Mock(); destination.iterdir.return_value = iter(())
        fixture = SimpleNamespace(lock=Lock(), requests=Counter({("GET", "/direct"): 1}))
        no_native_transfer(destination, fixture, 1)
        for requests in ({("GET", "/direct"): 2}, {("GET", "/direct"): 1, ("HEAD", "/direct"): 1}):
            fixture.requests = Counter(requests); destination.iterdir.return_value = iter(())
            with self.assertRaises(RuntimeError): no_native_transfer(destination, fixture, 1)
        fixture.requests = Counter({("GET", "/direct"): 1}); destination.iterdir.return_value = iter(["unexpected"])
        with self.assertRaises(RuntimeError): no_native_transfer(destination, fixture, 1)

    def test_executable_identity_is_retained_before_browser_launch(self):
        run = BrowserRecoveryRun(Path("unused"), Path("owned-firefox"), Path("unused"), "unlinked")
        run.plan = SimpleNamespace(path=Path("owned-domain"))
        run.owner = Mock(); run.binding = Mock()
        def stop_before_launch(*args):
            self.assertEqual(run.firefox_sha256, "browser-digest")
            raise RuntimeError("owned launch boundary")
        run.open_browser = Mock(side_effect=stop_before_launch)
        with patch("qualification.browser_recovery.build_probe", return_value=Path("owned-xpi")), \
             patch("qualification.browser_recovery.file_sha256", side_effect=lambda path: "browser-digest" if path == run.executable else "probe-digest"), \
             patch("qualification.browser_recovery.BrowserPeer"), \
             patch("qualification.browser_recovery.Fixture"), patch("pathlib.Path.mkdir"):
            with self.assertRaisesRegex(RuntimeError, "owned launch boundary"):
                run.transfer((1, 1))
        run.open_browser.assert_called_once()

    def test_recovery_scope_does_not_call_firefox_bytes_native_completion(self):
        run = BrowserRecoveryRun(Path("unused"), Path("unused"), Path("unused"), "unlinked")
        self.assertIn("output belongs to Firefox", run.scope())
        self.assertIsNone(run.fault)
        with self.assertRaises(RuntimeError):
            BrowserRecoveryRun(Path("unused"), Path("unused"), Path("unused"), "unknown")


if __name__ == "__main__":
    unittest.main()
