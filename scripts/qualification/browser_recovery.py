"""Owned no-native-transfer recovery diagnostics; temporary XPI, never install acceptance."""
import ctypes
import hashlib
import os
from pathlib import Path
import uuid

from .browser_installed import (BrowserInstalledRun, BrowserPeer, BODY, CaptureHandler, Fixture,
    Host, ELEMENT, build_probe, confirm, correct_output, file_sha256, message, new_tab,
    new_report, owned_architecture, phase_controls, restart_ui, wait)

SCENARIOS = ("unlinked", "aborted-terminal")
ACK_PROMPT = "Manager discarded this reservation and cannot complete this download. Dismiss this notice only after checking the download. If needed, restart it from the original page in Firefox. This button will not start a download."
DISCARD_PROMPT = "Discard this unused Manager reservation? Its status will be checked again. An already committed task will not be discarded. This does not restart a download in Firefox or erase the retained task identity."


def recovery_snapshot(snapshot, phase, pending, captured=False):
    expected = [{"request": 1, "stage": "decision", "cancelled": True},
                {"request": 1, "stage": "terminal", "cancelled": True}] if captured else []
    if (phase not in ("prepared", "aborted") or not isinstance(snapshot, dict)
            or snapshot.get("qualification") is not False or snapshot.get("connected") is not True
            or snapshot.get("phaseMetadataAvailable") is not True or snapshot.get("blocked") is not False
            or snapshot.get("overflow") is not False or type(snapshot.get("taskCount")) is not int
            or snapshot.get("taskCount") != 1 or snapshot.get("pending") != pending
            or snapshot.get("tasks") != [{"state": "queued" if phase == "prepared" else "cancelled", "phase": phase, "bytes": 0}]
            or snapshot.get("records") != expected or snapshot.get("commitReplaced") is not captured):
        raise RuntimeError("expected no-transfer recovery snapshot unavailable")
    if any(type(task.get("bytes")) is not int for task in snapshot["tasks"]):
        raise RuntimeError("invalid native byte-count type")
    if any(type(r.get("request")) is not int or r.get("cancelled") is not True for r in snapshot["records"]):
        raise RuntimeError("recovery cancellation correlation refused")
    return True


def cleanup_button(browser, scenario):
    text = "Discard unused reservation" if scenario == "unlinked" else "I checked the discarded download — dismiss notice"
    browser.wait("return [...document.querySelectorAll('#handoffs button')].some(b=>b.textContent===arguments[0]&&!b.disabled);", [text])
    reference = browser.script("return [...document.querySelectorAll('#handoffs button')].find(b=>b.textContent===arguments[0]);", [text])
    browser.command("WebDriver:ElementClick", {"id": reference[ELEMENT]})
    confirm(browser, DISCARD_PROMPT if scenario == "unlinked" else ACK_PROMPT)


def no_native_transfer(destination, fixture, browser_requests):
    with fixture.lock:
        counts = dict(fixture.requests)
    if (any(destination.iterdir()) or counts.get(("GET", "/direct"), 0) != browser_requests
            or any(method != "GET" for method, _ in counts)):
        raise RuntimeError("unexpected native output or fixture request")


def firefox_fallback(browser, fixture, downloads):
    browser.navigate(fixture.url("page")); browser.click("#direct")
    output = downloads / "owned-capture.bin"
    def completed():
        return browser.chrome("""const done=arguments[arguments.length-1];
const {Downloads}=ChromeUtils.importESModule('resource://gre/modules/Downloads.sys.mjs');
Downloads.getList(Downloads.PUBLIC).then(l=>l.getAll()).then(items=>done(items.length===1&&items.every(d=>
d.source.url===arguments[0]&&d.target.path===arguments[1]&&d.succeeded&&d.stopped&&!d.error&&!d.canceled&&d.currentBytes===arguments[2])),()=>done(false));""",
            [fixture.url("direct"), str(output), len(BODY)], True) is True
    wait(completed, 15)
    if set(downloads.iterdir()) != {output} or not correct_output(output):
        raise RuntimeError("independent Firefox fallback output differs")
    return output


class BrowserRecoveryRun(BrowserInstalledRun):
    def __init__(self, package, executable, report, scenario):
        if scenario not in SCENARIOS:
            raise RuntimeError("unsupported recovery scenario")
        super().__init__(package, executable, report)
        self.scenario = scenario

    def scope(self):
        return "owned installed no-native-transfer recovery; verified output belongs to Firefox"

    def additional_evidence(self):
        return {**super().additional_evidence(), "native_output_bytes": 0, "output_owner": "firefox",
                "native_handoff_phase": "aborted"}

    def transfer(self, identity):
        self.stage = "recovery-probe-build"
        xpi = build_probe(self.plan.path); self.probe_sha256 = file_sha256(xpi)
        self.firefox_sha256 = file_sha256(self.executable)
        peer = BrowserPeer(self.owner, self.binding, self.current_binding)
        profile, downloads = self.plan.path / "Firefox", self.plan.path / "FirefoxDownloads"
        downloads.mkdir()
        fixture = Fixture(handler=CaptureHandler, owners=self.fixtures)
        origins = [fixture.url("").rstrip("/")]
        browser, inspector = self.open_browser(peer, profile, downloads, xpi, origins)
        new_tab(browser)
        captured = self.scenario == "aborted-terminal"
        self.stage = "recovery-fixture"
        if captured:
            armed = message(browser, inspector, {"action": "arm-aborted-terminal"})
            if not isinstance(armed, dict) or armed.get("enabled") is not True:
                raise RuntimeError("recovery fault not armed")
            browser.navigate(fixture.url("page")); browser.click("#direct")
        else:
            if message(browser, inspector, {"action": "seed-unlinked"}) is None:
                raise RuntimeError("owned unlinked reservation not prepared")
        phase, pending = ("aborted", ["cancelled"]) if captured else ("prepared", [])
        def initial():
            snapshot = message(browser, inspector, {"action": "snapshot"})
            try: return recovery_snapshot(snapshot, phase, pending, captured)
            except RuntimeError: return False
        wait(initial, 15)
        no_native_transfer(self.destination, fixture, int(captured))
        if any(downloads.iterdir()): raise RuntimeError("unexpected competing Firefox output")
        browser.navigate(browser.manager)
        label = browser.task("owned-capture.bin", "cancelled" if captured else "queued")
        task_id = label.removeprefix("task-")
        if label != "task-" + str(uuid.UUID(task_id, version=4)):
            raise RuntimeError("recovery task identity refused")
        phase_controls(browser)
        self.close_browser(browser)
        self.stage = "recovery-restart"
        browser, inspector = self.open_browser(peer, profile, downloads, xpi, origins)
        recovery_snapshot(message(browser, inspector, {"action": "snapshot"}), phase, pending)
        no_native_transfer(self.destination, fixture, int(captured))
        restart_ui(browser, label, "cancelled" if captured else "queued"); phase_controls(browser)
        self.stage = "explicit-recovery-warning"
        cleanup_button(browser, self.scenario)
        def settled():
            try: return recovery_snapshot(message(browser, inspector, {"action": "snapshot"}), "aborted", [])
            except RuntimeError: return False
        wait(settled, 15)
        if browser.task("owned-capture.bin", "cancelled") != label:
            raise RuntimeError("discarded identity changed")
        phase_controls(browser)
        no_native_transfer(self.destination, fixture, int(captured))
        self.browser_checks.append("verified_warning_same_id_aborted_no_native_transfer_or_replay")
        self.stage = "recovery-firefox-fallback"
        output = firefox_fallback(browser, fixture, downloads)
        no_native_transfer(self.destination, fixture, int(captured) + 1)
        recovery_snapshot(message(browser, inspector, {"action": "snapshot"}), "aborted", [])
        self.close_browser(browser)
        if self.owner._observe()[1] != identity or not self.ui.tray(self.manager_window):
            raise RuntimeError("same companion lifetime not observed")
        self.stage = "recovery-independent-native-receipt"
        host = Host(self.binding.generation, self.plan.path, self.hosts, environment=self.environment)
        architecture = owned_architecture(host)
        host.wait(lambda: task_id in host.tasks)
        if set(host.tasks) != {task_id}: raise RuntimeError("unexpected native task identities")
        receipt = host.command("get_handoff", {"task_id": task_id})
        if (receipt["phase"] != "aborted" or receipt["task"]["task_id"] != task_id
                or receipt["task"].get("handoff_phase") != "aborted"
                or receipt["task"]["state"] != "cancelled" or receipt["task"]["bytes_completed"] != 0):
            raise RuntimeError("independent retained Aborted identity not established")
        no_native_transfer(self.destination, fixture, int(captured) + 1)
        if file_sha256(xpi) != self.probe_sha256: raise RuntimeError("diagnostic XPI changed")
        self.browser_checks.append("unarmed_correct_firefox_output_independent_aborted_native_receipt")
        return output, len(BODY), hashlib.sha256(BODY).hexdigest(), architecture


def qualify(package, executable, report, scenario):
    if not __debug__ or os.name != "nt" or ctypes.sizeof(ctypes.c_void_p) != 8:
        raise RuntimeError("recovery driver requires assertions and 64-bit Windows Python")
    executable = executable.resolve()
    if not executable.is_file(): raise RuntimeError("Firefox executable missing")
    run = BrowserRecoveryRun(Path(os.path.abspath(package)), executable, new_report(report), scenario)
    try: run.execute()
    except BaseException:
        run.hold_failed_owners()
        raise
