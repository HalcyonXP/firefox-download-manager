"""Real installed companion + temporary loopback diagnostic XPI, never install readiness.

Manager's add-on identity is used ONLY in the owned Firefox profile, because the
owned native manifest permits that identity. Packaged XPI bytes remain unchanged.
"""
import ctypes
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time
import uuid
import zipfile

from .browser_cases import confirm, current_handle, value
from .browser_peer import BrowserPeer
from .capture import BODY, CaptureHandler, load_probe, message
from .firefox import ADDON, ELEMENT, Firefox
from .fixture import Fixture
from .installed import FAULTS, InstalledRun, ROOT, ordinary, wait
from .native import Host, file_sha256, owned_architecture
from .support import bounded_json, new_report

PAYLOADS = {"background.js", "click.js", "manager.js", "manager.html", "manager.css", "manifest.json",
            "inspect.html", "LICENSE.txt", "THIRD-PARTY-NOTICES.txt"}


def build_probe(domain):
    directory = domain / "handoff-probe"
    environment = {**os.environ, "ESBUILD_WORKER_THREADS": "0", "ESBUILD_MAX_BUFFER": "16777216"}
    environment.pop("ESBUILD_BINARY_PATH", None)
    process = subprocess.Popen(["node", str(ROOT / "scripts/build-handoff-probe.mjs"), str(directory)],
                               cwd=ROOT, env=environment, stdin=subprocess.DEVNULL,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        if process.wait(timeout=60) != 0:
            raise RuntimeError("owned diagnostic compiler refused")
    except BaseException:
        # Do not kill Node and abandon its synchronous compiler child. Refuse
        # success and retain the exact build parent until it naturally exits.
        try:
            print("Diagnostic build failed; retaining its owned compiler parent until exit.", flush=True)
        except OSError:
            pass
        while True:
            try:
                process.wait()
                break
            except BaseException:
                try:
                    time.sleep(.1)
                except BaseException:
                    pass
        raise
    metadata = bounded_json(directory / "probe.json")
    if metadata.get("qualification") is not False or metadata.get("version") != 1 or set(metadata["files"]) != PAYLOADS:
        raise RuntimeError("diagnostic payload inventory refused")
    payloads = {}
    for name, digest in metadata["files"].items():
        path = directory / name
        ordinary(path)
        with path.open("rb") as source:
            data = source.read(1024 * 1024 + 1)
        if not 0 < len(data) <= 1024 * 1024 or hashlib.sha256(data).hexdigest() != digest:
            raise RuntimeError("diagnostic payload identity refused")
        payloads[name] = data
    manifest = json.loads(payloads["manifest.json"])
    if (manifest["browser_specific_settings"]["gecko"]["id"] != ADDON
            or manifest["permissions"] != ["nativeMessaging", "menus", "storage", "webRequest", "webRequestBlocking"]
            or manifest["host_permissions"] != ["http://127.0.0.1/*"] or manifest["incognito"] != "not_allowed"
            or "optional_permissions" in manifest or "optional_host_permissions" in manifest):
        raise RuntimeError("diagnostic permissions refused")
    xpi = domain / "handoff-probe.xpi"
    with zipfile.ZipFile(xpi, "x", compression=zipfile.ZIP_STORED) as archive:
        for name, data in payloads.items():
            archive.writestr(name, data)
    return xpi


def correct_output(path):
    ordinary(path)
    if path.stat().st_size != len(BODY):
        return False
    with path.open("rb") as stream:
        return stream.read(len(BODY) + 1) == BODY


def new_tab(browser):
    handle = value(browser.command("WebDriver:NewWindow", {"type": "tab"}))["handle"]
    browser.command("WebDriver:SwitchToWindow", {"handle": handle})
    return handle


def restart_ui(browser, label, state):
    # Never repurpose the dedicated inspector as a Manager or fixture page.
    new_tab(browser)
    browser.navigate(browser.manager)
    if browser.task("owned-capture.bin", state) != label:
        raise RuntimeError("UI task identity changed across restart")


CONTINUE_PROMPT = "Continue only if Firefox has stopped this download. If you cannot identify this download, do not continue. If Firefox is still downloading, continuing can create competing output. Continue in Manager?"


def explicit_continue(browser):
    text = "I checked Firefox stopped — continue in Manager"
    browser.wait("return [...document.querySelectorAll('#handoffs button')].some(b=>b.textContent===arguments[0]&&!b.disabled);", [text])
    reference = browser.script("return [...document.querySelectorAll('#handoffs button')].find(b=>b.textContent===arguments[0]);", [text])
    browser.command("WebDriver:ElementClick", {"id": reference[ELEMENT]})
    confirm(browser, CONTINUE_PROMPT)  # Read and verify the actual owned warning before accepting.


def phase_controls(browser):
    # These two live checkpoints are Prepared or Completed handoffs, never ordinary tasks.
    if browser.script("return [...document.querySelectorAll('#tasks .actions button')].map(b=>b.textContent);") != ["Open folder"]:
        raise RuntimeError("handoff UI exposed inappropriate ordinary controls")


def check_snapshot(snapshot, *, captured, pending, state, size):
    if (not isinstance(snapshot, dict) or snapshot.get("qualification") is not False
            or snapshot.get("connected") is not True or snapshot.get("phaseMetadataAvailable") is not True
            or snapshot.get("overflow") is not False
            or snapshot.get("blocked") is not False or snapshot.get("pending") != pending
            or type(snapshot.get("taskCount")) is not int or snapshot.get("taskCount") != 1
            or snapshot.get("tasks") != [{"state": state, "bytes": size, "phase": "prepared" if pending else "committed"}]):
        raise RuntimeError("one completed native task with settled journal not observed")
    expected = [{"request": 1, "stage": "decision", "cancelled": True},
                {"request": 1, "stage": "terminal", "cancelled": True}] if captured else []
    records = snapshot.get("records")
    if (records != expected or any(type(r.get("request")) is not int or r.get("cancelled") is not True for r in records)):
        raise RuntimeError("unique cancellation decision/terminal correlation not observed")
    return True


def settled(snapshot, *, captured):
    return check_snapshot(snapshot, captured=captured, pending=[], state="completed", size=len(BODY))


def pending_confirmation(snapshot, *, captured):
    return check_snapshot(snapshot, captured=captured, pending=["intent"], state="queued", size=0)


class BrowserInstalledRun(InstalledRun):
    def __init__(self, package, executable, report, fault=None, scenario="nominal"):
        super().__init__(package, report, fault)
        if scenario not in ("nominal", "missing-terminal") or (scenario != "nominal" and fault is not None):
            raise RuntimeError("unsupported combined scenario/fault pair")
        self.scenario = scenario
        self.executable = executable
        self.browser_checks = []
        self.probe_sha256 = self.firefox_sha256 = None

    def failure_record(self, error):
        try:
            super().failure_record(error)
        finally:
            if self.plan is not None and self.plan.created:
                frames = []
                trace = error.__traceback__
                while trace is not None and len(frames) < 12:
                    frames.append({"function": trace.tb_frame.f_code.co_name, "line": trace.tb_lineno})
                    trace = trace.tb_next
                with (self.plan.path / "browser-failure.private.json").open("x", encoding="utf-8") as stream:
                    json.dump({"stage": self.stage, "frames": frames,
                        "browsers": [{"verified": b.verified, "closed": b.closed,
                            "exit": b.process.poll() if b.process is not None else None} for b in self.browsers]}, stream)

    def scope(self):
        return "owned installed companion and temporary loopback Firefox handoff"

    def additional_evidence(self):
        if len(self.browsers) != 2 or any(not b.closed or b.process is None or b.process.returncode != 0 for b in self.browsers):
            raise RuntimeError("successful retained browser exits required")
        if self.firefox_sha256 != file_sha256(self.executable):
            raise RuntimeError("Firefox executable identity changed")
        return {"qualification": False, "temporary_xpi": True, "scenario": self.scenario, "browser_checks": self.browser_checks,
                "probe_xpi_sha256": self.probe_sha256, "firefox_exe_sha256": self.firefox_sha256}

    def open_browser(self, peer, profile, downloads, xpi, origin):
        environment = {**self.environment, "MOZ_CRASHREPORTER_DISABLE": "1"}
        browser = Firefox(self.executable, profile, environment, owned_peer=peer)
        self.browsers.append(browser)  # Before any launch or profile write.
        browser.start()
        if browser.chrome("""Services.prefs.setIntPref('browser.download.folderList',2);
Services.prefs.setBoolPref('browser.download.useDownloadDir',true);
Services.prefs.setStringPref('browser.download.dir',arguments[0]);
return Services.prefs.getStringPref('browser.download.dir')===arguments[0];""", [str(downloads)]) is not True:
            raise RuntimeError("owned fallback destination not established")
        inspector_url = load_probe(browser, xpi, ADDON)
        browser.manager = inspector_url.removesuffix("inspect.html") + "manager.html"
        browser.navigate(inspector_url)
        inspector = current_handle(browser)
        snapshot = message(browser, inspector, {"action": "ready", "destination": str(self.destination), "origin": origin})
        if not isinstance(snapshot, dict) or (snapshot.get("connected") is not True or snapshot.get("destinationVerified") is not True or snapshot.get("phaseMetadataAvailable") is not True):
            raise RuntimeError("real Firefox native connection or owned destination unavailable")
        return browser, inspector

    def close_browser(self, browser):
        browser.close()
        if not browser.closed or browser.process is None or browser.process.wait(timeout=0) != 0:
            raise RuntimeError("successful browser exit/join not observed")

    def transfer(self, identity):
        self.stage = "browser-probe-build"
        xpi = build_probe(self.plan.path)
        self.probe_sha256 = file_sha256(xpi)
        self.firefox_sha256 = file_sha256(self.executable)
        peer = BrowserPeer(self.owner, self.binding, self.current_binding)
        profile, downloads = self.plan.path / "Firefox", self.plan.path / "FirefoxDownloads"
        downloads.mkdir()
        fixture = Fixture(handler=CaptureHandler, owners=self.fixtures)
        self.stage = "browser-startup"
        browser, inspector = self.open_browser(peer, profile, downloads, xpi, fixture.url("").rstrip("/"))
        self.checkpoint("bridge-started")
        missing = self.scenario == "missing-terminal"
        armed = message(browser, inspector, {"action": "arm-missing-terminal" if missing else "arm"})
        if not isinstance(armed, dict) or armed.get("enabled") is not True or armed.get("taskCount") != 0:
            raise RuntimeError("empty owned handoff capture not armed")
        test_tab = value(browser.command("WebDriver:NewWindow", {"type": "tab"}))["handle"]
        browser.command("WebDriver:SwitchToWindow", {"handle": test_tab})
        browser.navigate(fixture.url("page"))
        browser.click("#direct")
        self.stage = "browser-cancellation-completion"
        def completed():
            snapshot = message(browser, inspector, {"action": "snapshot"})
            with (self.plan.path / "browser-observation.private.json").open("w", encoding="utf-8") as stream:
                json.dump(snapshot, stream)
            try:
                if missing:
                    return pending_confirmation(snapshot, captured=True) and snapshot.get("terminalSuppressed") is True
                return settled(snapshot, captured=True)
            except RuntimeError:
                return False
        wait(completed, 30)
        if any(downloads.iterdir()):
            raise RuntimeError("competing Firefox output exists")
        output = self.destination / "owned-capture.bin"
        ordinary(output)
        if missing:
            if any(self.destination.iterdir()) or fixture.requests[("GET", "/direct")] != 1:
                raise RuntimeError("unconfirmed preparation dispatched network/output")
        elif not correct_output(output):
            raise RuntimeError("native output differs from independent fixture")
        browser.navigate(browser.manager)
        label = browser.task("owned-capture.bin", "queued" if missing else "completed")
        phase_controls(browser)
        task_id = label.removeprefix("task-")
        if label != "task-" + str(uuid.UUID(task_id, version=4)):
            raise RuntimeError("UI task identity not established")
        self.browser_checks.append("actual_cancellation_withheld_from_coordinator_prepared_only" if missing else
                                   "trusted_click_cancelled_one_completed_native_task_ui_independent_output")
        if not missing:
            self.checkpoint("completed")
        self.close_browser(browser)
        if self.owner._observe()[1] != identity or not self.ui.tray(self.manager_window):
            raise RuntimeError("companion did not survive Firefox exit")
        self.stage = "browser-restart"
        browser, inspector = self.open_browser(peer, profile, downloads, xpi, fixture.url("").rstrip("/"))
        self.stage = "restarted-native-snapshot"
        (pending_confirmation if missing else settled)(message(browser, inspector, {"action": "snapshot"}), captured=False)
        if missing and (any(self.destination.iterdir()) or fixture.requests[("GET", "/direct")] != 1):
            raise RuntimeError("restart committed uncertain cancellation automatically")
        # Temporary reinstallation is explicit, NOT persistent-XPI qualification.
        restart_ui(browser, label, "queued" if missing else "completed")
        phase_controls(browser)
        if missing:
            self.stage = "explicit-continuation"
            explicit_continue(browser)
            browser.task("owned-capture.bin", "completed")
            def resolved():
                try:
                    return settled(message(browser, inspector, {"action": "snapshot"}), captured=False)
                except RuntimeError:
                    return False
            wait(resolved, 30)
            if not correct_output(output):
                raise RuntimeError("explicit continuation output differs")
            self.browser_checks.append("intent_survives_restart_no_auto_commit_explicit_ui_continuation_same_task")
        self.stage = "unarmed-browser-click"
        browser.navigate(fixture.url("page"))
        browser.click("#direct")  # Unarmed after restart: Firefox retains this request.
        fallback = downloads / "owned-capture.bin"
        self.stage = "unarmed-browser-terminal"
        def fallback_completed():
            observed = browser.chrome("""const done=arguments[arguments.length-1];
const {Downloads}=ChromeUtils.importESModule('resource://gre/modules/Downloads.sys.mjs');
Downloads.getList(Downloads.PUBLIC).then(l=>l.getAll()).then(items=>done({count:items.length,
items:items.slice(0,8).map(d=>({source:d.source.url===arguments[0],target:d.target.path===arguments[1],
succeeded:d.succeeded,stopped:d.stopped,error:!!d.error,canceled:d.canceled,bytes:d.currentBytes}))}),()=>done(null));""",
                         [fixture.url("direct"), str(fallback)], True)
            with (self.plan.path / "fallback-observation.private.json").open("w", encoding="utf-8") as stream:
                json.dump(observed, stream)
            return observed == {"count": 1, "items": [{"source": True, "target": True, "succeeded": True,
                "stopped": True, "error": False, "canceled": False, "bytes": len(BODY)}]}
        wait(fallback_completed, 15)
        if set(downloads.iterdir()) != {fallback} or not correct_output(fallback):
            raise RuntimeError("unarmed Firefox fallback output differs")
        snapshot = message(browser, inspector, {"action": "snapshot"})
        with (self.plan.path / "fallback-native.private.json").open("x", encoding="utf-8") as stream:
            json.dump(snapshot, stream)
        settled(snapshot, captured=False)
        self.close_browser(browser)
        if self.owner._observe()[1] != identity or not self.ui.tray(self.manager_window):
            raise RuntimeError("same companion lifetime not observed after restart")
        self.browser_checks.append("temporary_reload_existing_task_no_replay_unarmed_correct_firefox_fallback")
        self.stage = "independent-native-reconnect"
        host = Host(self.binding.generation, self.plan.path, self.hosts, environment=self.environment)
        architecture = owned_architecture(host)
        host.wait(lambda: len(host.tasks) == 1)
        if set(host.tasks) != {task_id} or host.tasks[task_id]["state"] != "completed":
            raise RuntimeError("independent native reconnect did not observe the same completed task")
        receipt = host.command("get_handoff", {"task_id": task_id})
        if receipt["phase"] != "committed" or receipt["task"]["task_id"] != task_id or receipt["task"]["state"] != "completed" or receipt["task"].get("handoff_phase") != "committed":
            raise RuntimeError("independent committed handoff identity not established")
        if file_sha256(xpi) != self.probe_sha256:
            raise RuntimeError("diagnostic XPI changed")
        self.checkpoint("reconnected")
        return output, len(BODY), hashlib.sha256(BODY).hexdigest(), architecture


def qualify(package, executable, report, fault=None, scenario="nominal"):
    if not __debug__ or os.name != "nt" or ctypes.sizeof(ctypes.c_void_p) != 8:
        raise RuntimeError("combined driver requires assertions and 64-bit Windows Python")
    if fault is not None and fault not in FAULTS:
        raise RuntimeError("invalid combined fault stage")
    executable = executable.resolve()
    if not executable.is_file():
        raise RuntimeError("Firefox executable missing")
    run = BrowserInstalledRun(Path(os.path.abspath(package)), executable, new_report(report), fault, scenario)
    try:
        run.execute()
    except BaseException:
        run.hold_failed_owners()
        raise
