"""Model preference authority and owned-fixture archival; no browser launch."""
from collections import Counter
from pathlib import Path
from tempfile import TemporaryDirectory
from threading import Lock
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch
from qualification.capture_toggle import ELEMENT, exercise_off, preference
from qualification.capture import BODY


class CaptureToggleTests(unittest.TestCase):
    def test_actual_control_click_and_verified_state(self):
        browser = Mock(); browser.script.side_effect = [True, {ELEMENT: "owned-control"}]
        state = {"available": True, "ready": True, "enabled": False, "busy": False, "failed": False}
        with patch("qualification.capture_toggle.message", return_value={"capturePreference": state}):
            preference(browser, "owned-inspector", False)
        browser.command.assert_called_once_with("WebDriver:ElementClick", {"id": "owned-control"})

    def test_restart_check_does_not_silently_reapply_off(self):
        browser = Mock(); browser.script.side_effect = [True, {ELEMENT: "owned-control"}]
        state = {"available": True, "ready": True, "enabled": False, "busy": False, "failed": False}
        with patch("qualification.capture_toggle.message", return_value={"capturePreference": state}):
            with self.assertRaisesRegex(RuntimeError, "did not persist"):
                preference(browser, "owned-inspector", False, change=False)
        browser.command.assert_not_called()

    def test_numeric_booleans_and_unavailable_state_refuse(self):
        browser = Mock(); browser.script.return_value = False
        state = {"available": True, "ready": True, "enabled": False, "busy": False, "failed": False}
        for change in ({"enabled": 0}, {"ready": False}, {"extra": False}, {"failed": True}):
            with patch("qualification.capture_toggle.message", return_value={"capturePreference": {**state, **change}}):
                with self.assertRaises(RuntimeError): preference(browser, "owned-inspector", False)

    def test_only_verified_owned_fixture_is_archived_and_history_removal_must_succeed(self):
        for removed in (False, True):
            with self.subTest(removed=removed), TemporaryDirectory() as root:
                root = Path(root); downloads = root / "FirefoxDownloads"; downloads.mkdir()
                destination = root / "NativeDownloads"; destination.mkdir()
                output = downloads / "owned-capture.bin"; output.write_bytes(BODY)
                run = SimpleNamespace(destination=destination, plan=SimpleNamespace(path=root), browser_checks=[])
                browser = Mock(); browser.chrome.return_value = removed
                fixture = Mock(); fixture.lock = Lock(); fixture.requests = Counter({("GET", "/direct"): 1}); fixture.url.return_value = "http://127.0.0.1:9876/direct"
                snapshot = {"taskCount": 0, "pending": [], "records": []}
                with patch("qualification.capture_toggle.preference") as control, \
                     patch("qualification.capture_toggle.firefox_fallback", return_value=output), \
                     patch("qualification.capture_toggle.message", side_effect=[{"enabled": True}, snapshot]):
                    if removed:
                        exercise_off(run, browser, "owned-inspector", fixture, downloads)
                        self.assertFalse(output.exists()); self.assertEqual((root / "firefox-off.bin").read_bytes(), BODY)
                        self.assertEqual([call.args[2] for call in control.call_args_list], [False, True])
                    else:
                        with self.assertRaises(RuntimeError): exercise_off(run, browser, "owned-inspector", fixture, downloads)
                        self.assertTrue(output.exists()); self.assertFalse((root / "firefox-off.bin").exists())


if __name__ == "__main__": unittest.main()
