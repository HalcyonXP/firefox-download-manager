"""Owned-context driver models, never actual browser/profile automation."""
import copy
import json
from pathlib import Path
import subprocess
from threading import Lock
from types import SimpleNamespace
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import Mock, patch

from qualification.browser_contexts import (CONTAINER, PRIVATE, CONTEXT, valid_context,
    untouched, context_observations, archive_output, exercise_contexts, context_evidence)
from qualification.capture import BODY


def snapshot():
    return {"qualification": False, "connected": True, "enabled": True, "phaseMetadataAvailable": True,
            "blocked": False, "overflow": False, "taskCount": 0, "tasks": [], "pending": [], "records": [],
            "capturePreference": {"available": True, "ready": True, "enabled": True, "busy": False, "failed": False},
            "contexts": [{"method": "GET", "frame": "main", "store": "other", "private": False}]}


class BrowserContextTests(unittest.TestCase):
    def test_exact_nondefault_and_private_contexts_not_boolean_or_default_aliases(self):
        valid_context({"container": 5, "private": False}, "container", 5)
        valid_context({"container": 0, "private": True}, "private", 0)
        for value, kind, identity in [({"container": 0, "private": False}, "container", 0),
                                      ({"container": True, "private": False}, "container", 1),
                                      ({"container": 0, "private": False}, "private", 0),
                                      ({"container": 0, "private": 1}, "private", 0)]:
            with self.assertRaises(RuntimeError): valid_context(value, kind, identity)

    def test_negative_controls_require_available_armed_capture_and_zero_native_work(self):
        untouched(snapshot())
        for change in ({"enabled": False}, {"connected": False}, {"taskCount": False}, {"taskCount": 1},
                       {"pending": ["intent"]}, {"overflow": True}, {"records": [{}]}):
            with self.assertRaises(RuntimeError): untouched({**snapshot(), **change})
        for key in ("available", "ready", "enabled"):
            value = snapshot(); value["capturePreference"][key] = False
            with self.assertRaises(RuntimeError): untouched(value)

    def test_container_classification_and_denied_private_visibility_are_distinct(self):
        context_observations(snapshot())
        for change in ({"private": True}, {"private": 0}, {"store": "default"}, {"store": "missing"}, {"frame": "other"}):
            value = snapshot(); value["contexts"][0].update(change)
            with self.assertRaises(RuntimeError): context_observations(value)
        value = snapshot(); value["contexts"].append(copy.deepcopy(value["contexts"][0]))
        with self.assertRaises(RuntimeError): context_observations(value)

    def test_archival_checks_private_list_completed_bytes_and_exclusive_output(self):
        for kind in ("container", "private"):
            with self.subTest(kind=kind), TemporaryDirectory() as root:
                root = Path(root).resolve(); downloads = root / "downloads"; downloads.mkdir()
                output = downloads / "owned-capture.bin"; output.write_bytes(BODY)
                browser = Mock(); browser.chrome.return_value = True
                archive_output(browser, downloads, root, kind, "owned-source")
                self.assertEqual((root / f"firefox-{kind}.bin").read_bytes(), BODY); self.assertFalse(output.exists())
                self.assertTrue(all(call.args[1][2] is (kind == "private") for call in browser.chrome.call_args_list))
                output.write_bytes(BODY)
                with self.assertRaises(FileExistsError): archive_output(browser, downloads, root, kind, "owned-source")
                self.assertEqual(output.read_bytes(), BODY)

    def test_context_loop_requires_each_window_close_and_keeps_parent_driver(self):
        with TemporaryDirectory() as directory:
            root = Path(directory).resolve(); destination = root / "native"; destination.mkdir()
            run = SimpleNamespace(plan=SimpleNamespace(path=root), destination=destination, browser_checks=[])
            fixture = SimpleNamespace(url=lambda item: "http://127.0.0.1/" + item, requests={}, lock=Lock())
            browser = Mock(); browser.chrome.side_effect = [5, {"private": False, "container": 5}, True, {"private": True, "container": 0}]
            def archive(*_):
                fixture.requests[("GET", "/direct")] = fixture.requests.get(("GET", "/direct"), 0) + 1
                return len(BODY)
            initial = snapshot(); initial["contexts"] = []
            module = "qualification.browser_contexts."
            with patch(module + "message", side_effect=[initial, snapshot(), snapshot()]), patch(module + "handles", return_value={"inspector"}), \
                 patch(module + "new_window", side_effect=["container", "private"]), patch(module + "current_handle", side_effect=["container", "private"]), \
                 patch(module + "archive_output", side_effect=archive):
                result = exercise_contexts(run, browser, "inspector", fixture, root)
            self.assertEqual(len(run.browser_checks), 2)
            self.assertEqual([r["extension_direct_observations"] for r in result], [1, 0])
            with self.assertRaises(RuntimeError): context_evidence([])
            result[0]["private"] = 0
            with self.assertRaises(RuntimeError): context_evidence(result)
            self.assertEqual(sum(call.args == ("WebDriver:CloseWindow",) for call in browser.command.call_args_list), 2)
            browser.close.assert_not_called()  # The enclosing InstalledRun retains/joins the process.

    def test_incorrect_fixture_bytes_are_not_archived_or_removed(self):
        with TemporaryDirectory() as directory:
            root = Path(directory).resolve(); downloads = root / "downloads"; downloads.mkdir()
            output = downloads / "owned-capture.bin"; output.write_bytes(b"x" * len(BODY))
            browser = Mock(); browser.chrome.return_value = True
            with self.assertRaises(RuntimeError): archive_output(browser, downloads, root, "private", "owned-source")
            self.assertTrue(output.exists()); self.assertFalse((root / "firefox-private.bin").exists())

    def test_context_scripts_use_owned_service_creation_without_permission_or_pref_override(self):
        script = """const assert=require('node:assert/strict');const sources=JSON.parse(require('node:fs').readFileSync(0,'utf8'));
for(const source of sources)new Function(source);
const principal={},tab={};global.Services={scriptSecurityManager:{getSystemPrincipal(){return principal;}}};
global.ChromeUtils={importESModule(uri){assert.equal(uri,'moz-src:///toolkit/components/contextualidentity/ContextualIdentityService.sys.mjs');
return {ContextualIdentityService:{create(){return {userContextId:7};}}};}};
global.gBrowser={addTab(url,options){assert.equal(url,'about:blank');assert.equal(options.userContextId,7);assert.equal(options.triggeringPrincipal,principal);return tab;}};
assert.equal(new Function(sources[0])(),7);assert.equal(gBrowser.selectedTab,tab);
global.OpenBrowserWindow=options=>assert.deepEqual(options,{private:true});assert.equal(new Function(sources[1])(),true);
"""
        subprocess.run(["node", "-e", script], input=json.dumps([CONTAINER, PRIVATE, CONTEXT]).encode("utf-8"), capture_output=True, timeout=15, check=True)
        for source in (CONTAINER, PRIVATE, CONTEXT):
            for forbidden in ("setBoolPref", "setIntPref", "permissions.request", "cookies.get"):
                self.assertNotIn(forbidden, source)


if __name__ == "__main__": unittest.main()
