"""File/loopback/models only; never launch Firefox or change registration."""
from http.client import HTTPConnection
import io
import json
import subprocess
from pathlib import Path
from unittest.mock import Mock, patch
import unittest
import zipfile

from qualification.fixture import Fixture
from qualification.xpi_policy import ADDON, PAYLOADS, approval, inspect_xpi, persistent_receipt
from qualification.xpi_ui import WATCH, SNAPSHOT, RECEIPT as RECEIPT_SOURCE, approve_visible, protections, observe_failure, observe_install
from qualification.xpi_persistence import PersistenceRun, handler_for, require_final_owners

ROOT = Path(__file__).resolve().parents[1]
IDENTITY = {"addon_id": ADDON, "version": "0.1.0", "name": "Firefox Download Manager"}
RECEIPT = {"id": ADDON, "version": "0.1.0", "active": True, "temporary": False, "scope": 1, "private_allowed": False}
PROMPT = {"id": "addon-webext-permissions", "open": True, "enabled": True, "label": "Add",
          "source_matches": True, "install_count": 1, "addon_id": ADDON, "name": IDENTITY["name"],
          "permission_schema": True, "permissions": ["nativeMessaging", "menus", "storage"], "origins": []}


def archive(manifest=None, extra=None):
    stream = io.BytesIO()
    data = json.loads((ROOT / "extension/src/manifest.json").read_text(encoding="utf-8")) if manifest is None else manifest
    with zipfile.ZipFile(stream, "w") as output:
        for name in sorted(PAYLOADS): output.writestr(name, json.dumps(data) if name == "manifest.json" else "fixture")
        if extra: output.writestr(extra, "fixture")
    return stream.getvalue()


class XpiPolicyTests(unittest.TestCase):
    def test_closed_package_identity_permissions_and_inventory(self):
        self.assertEqual(inspect_xpi(archive())["addon_id"], ADDON)
        for change in ({"permissions": ["nativeMessaging", "menus", "storage", "cookies"]},
                       {"host_permissions": ["<all_urls>"]}, {"incognito": "spanning"}):
            manifest = json.loads((ROOT / "extension/src/manifest.json").read_text(encoding="utf-8")); manifest.update(change)
            with self.assertRaises(RuntimeError): inspect_xpi(archive(manifest))
        with self.assertRaises(RuntimeError): inspect_xpi(archive(extra="unexpected.js"))

    def test_exact_permission_prompt_and_site_warning(self):
        self.assertIn("addon-webext-permissions", approval(PROMPT, IDENTITY))
        site = {**PROMPT, "id": "addon-install-blocked", "label": "Continue to Installation", "addon_id": None}
        self.assertIn("addon-install-blocked", approval(site, IDENTITY))
        for change in ({"enabled": False}, {"open": False}, {"source_matches": False}, {"label": "Allow"},
                       {"addon_id": "other"}, {"origins": ["<all_urls>"]}, {"permissions": ["cookies"]},
                       {"install_count": True}, {"permission_schema": False}, {"extra": True}, {"id": "addon-install-confirmation"}):
            with self.subTest(change=change), self.assertRaises(RuntimeError): approval({**PROMPT, **change}, IDENTITY)

    def test_delay_uncertain_click_and_unknown_warning_never_replay(self):
        browser = Mock(); seen = set()
        browser.chrome.return_value = {**PROMPT, "enabled": False}
        self.assertFalse(approve_visible(browser, "owned-source", IDENTITY, seen)); browser.click.assert_not_called()
        browser.chrome.return_value = PROMPT; browser.click.side_effect = RuntimeError("synthetic lost reply")
        with self.assertRaises(RuntimeError): approve_visible(browser, "owned-source", IDENTITY, seen)
        self.assertFalse(approve_visible(browser, "owned-source", IDENTITY, seen)); browser.click.assert_called_once()

    def test_no_temporary_inactive_private_or_scalar_ambiguous_receipt(self):
        self.assertTrue(persistent_receipt(RECEIPT, IDENTITY))
        for change in ({"temporary": True}, {"active": False}, {"scope": True}, {"private_allowed": True},
                       {"active": 1}, {"id": "other"}, {"extra": False}):
            with self.assertRaises(RuntimeError): persistent_receipt({**RECEIPT, **change}, IDENTITY)

    def test_readonly_witnesses_require_boolean_fields(self):
        browser = Mock(); browser.chrome.return_value = {"signatures_required": True, "install_enabled": True}
        self.assertTrue(protections(browser)["signatures_required"])
        browser.chrome.return_value = {"signatures_required": 1, "install_enabled": True}
        with self.assertRaises(RuntimeError): protections(browser)
        browser.chrome.return_value = {"failed": True, "signatureRequired": True, "otherFailure": False}
        self.assertTrue(observe_failure(browser)["signatureRequired"])
        browser.chrome.return_value = {"failed": 1, "signatureRequired": True, "otherFailure": False}
        with self.assertRaises(RuntimeError): observe_failure(browser)

    def test_bounded_actual_fixture_serves_only_exact_xpi(self):
        owners = []; data = archive()
        try:
            fixture = Fixture(handler=handler_for(data), owners=owners)
            client = HTTPConnection("127.0.0.1", fixture.server.server_port, timeout=5)
            try:
                client.request("GET", "/manager.xpi"); response = client.getresponse()
                self.assertEqual(response.status, 200); self.assertEqual(response.getheader("Content-Type"), "application/x-xpinstall")
                self.assertEqual(response.read(len(data) + 1), data)
            finally: client.close()
        finally:
            for fixture in owners: fixture.close()
        self.assertTrue(all(fixture.closed for fixture in owners))

    def test_failed_start_is_retained_before_cleanup_and_failed_exit_refuses(self):
        run = PersistenceRun(Path("unused"), Path("unused"), Path("unused"))
        browser = Mock(); browser.start.side_effect = RuntimeError("synthetic failed start")
        with patch("qualification.xpi_persistence.preflight"), patch("qualification.xpi_persistence.Firefox", return_value=browser):
            with self.assertRaises(RuntimeError): run.open(Path("owned"), {})
        self.assertEqual(run.browsers, [browser]); self.assertTrue(run.retire()); browser.close.assert_called_once()
        browser.closed = True; browser.process.wait.return_value = 1
        with self.assertRaises(RuntimeError): run.close(browser)

    def test_observation_requires_permission_step_and_preserves_environment_failure_distinction(self):
        browser = Mock(); browser.chrome.return_value = True
        settings = {"signatures_required": True, "install_enabled": True}
        clean = {"failed": False, "signatureRequired": False, "otherFailure": False}
        with patch("qualification.xpi_ui.protections", return_value=settings), \
             patch("qualification.xpi_ui.observe_failure", return_value=clean), \
             patch("qualification.xpi_ui.receipt", side_effect=[None, RECEIPT]):
            with self.assertRaisesRegex(RuntimeError, "approval not observed"):
                observe_install(browser, "owned-page", "owned-xpi", IDENTITY)
        def approve(*args): args[-1].add("addon-webext-permissions")
        with patch("qualification.xpi_ui.protections", return_value=settings), \
             patch("qualification.xpi_ui.observe_failure", return_value=clean), \
             patch("qualification.xpi_ui.receipt", side_effect=[None, None, RECEIPT]), \
             patch("qualification.xpi_ui.approve_visible", side_effect=approve), patch("qualification.xpi_ui.time.sleep"):
            outcome, seen, observed = observe_install(browser, "owned-page", "owned-xpi", IDENTITY)
        self.assertEqual(outcome, "installed"); self.assertEqual(seen, {"addon-webext-permissions"}); self.assertEqual(observed, settings)
        with patch("qualification.xpi_ui.protections", side_effect=[settings, {**settings, "signatures_required": False}]), \
             patch("qualification.xpi_ui.observe_failure", return_value=clean), \
             patch("qualification.xpi_ui.receipt", side_effect=[None, None, RECEIPT]), \
             patch("qualification.xpi_ui.approve_visible", side_effect=approve), patch("qualification.xpi_ui.time.sleep"):
            with self.assertRaisesRegex(RuntimeError, "protections changed"):
                observe_install(browser, "owned-page", "owned-xpi", IDENTITY)
        blocked = {"failed": True, "signatureRequired": True, "otherFailure": False}
        with patch("qualification.xpi_ui.protections", return_value=settings), \
             patch("qualification.xpi_ui.observe_failure", return_value=blocked), \
             patch("qualification.xpi_ui.receipt", return_value=None):
            self.assertEqual(observe_install(browser, "owned-page", "owned-xpi", IDENTITY)[0], "signature-requirement-observed")

    def test_embedded_readonly_js_compiles_and_failure_witness_is_exact(self):
        script = """const assert=require('node:assert/strict');
const sources=JSON.parse(require('node:fs').readFileSync(0,'utf8'));
for(const source of sources)new Function(source);
for(const code of [undefined,-5]){
 global.window={};global.gBrowser={selectedBrowser:{}};let observer;
 global.Services={obs:{addObserver(value,topic){assert.equal(topic,'addon-install-failed');observer=value;}}};
 global.ChromeUtils={importESModule(){return {AddonManager:{ERROR_SIGNEDSTATE_REQUIRED:code}};}};
 assert.equal(new Function(sources[0])('owned-source'),true);
 const emit=(source,error)=>observer.observe({wrappedJSObject:{browser:gBrowser.selectedBrowser,installs:[{sourceURI:{spec:source},error}]}},'addon-install-failed');
 emit('foreign-source',code);assert.equal(window.__ownedManagerInstallWitness.failed,false);
 emit('owned-source',code);assert.equal(window.__ownedManagerInstallWitness.signatureRequired,code===-5);
 assert.equal(window.__ownedManagerInstallWitness.otherFailure,code!==-5);
 emit('owned-source',code);assert.equal(window.__ownedManagerInstallWitness.signatureRequired,false);
 assert.equal(window.__ownedManagerInstallWitness.otherFailure,true);
}
"""
        subprocess.run(["node", "-e", script], input=json.dumps([WATCH, SNAPSHOT, RECEIPT_SOURCE]).encode("utf-8"),
                       capture_output=True, check=True, timeout=15)

    def test_final_report_requires_two_successful_exits_for_persistence(self):
        first, second, fixture = Mock(), Mock(), Mock()
        for browser in (first, second): browser.closed = True; browser.process.returncode = 0
        fixture.closed = True
        require_final_owners("installed", [first, second], [fixture])
        require_final_owners("signature-requirement-observed", [first], [fixture])
        with self.assertRaises(RuntimeError): require_final_owners("installed", [first], [fixture])
        second.process.returncode = 1
        with self.assertRaises(RuntimeError): require_final_owners("installed", [first, second], [fixture])
        second.process.returncode = 0; fixture.closed = False
        with self.assertRaises(RuntimeError): require_final_owners("installed", [first, second], [fixture])

    def test_observer_and_driver_have_no_install_or_protection_bypass(self):
        source = (ROOT / "scripts/qualification/xpi_ui.py").read_text(encoding="utf-8") + (ROOT / "scripts/qualification/xpi_persistence.py").read_text(encoding="utf-8")
        for forbidden in ("installTemporaryAddon", "getInstallForFile", "Addon:Install", "installAddonFromWebpage", "setBoolPref", "setIntPref", "setStringPref", ".mainAction.callback", "disableSecurityDelay"):
            self.assertNotIn(forbidden, source)
        self.assertIn('seen.add(state["id"])', source)
        self.assertIn('browser.click(selector, chrome=True)', source)
        self.assertIn('require_joined_success(retired, fixtures)', source)


if __name__ == "__main__": unittest.main()
