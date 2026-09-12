"""Consolidated actual candidate campaign, temporary loading explicitly not persistence."""
import ctypes
import hashlib
import os
from pathlib import Path
from .browser_cases import current_handle, handles, new_window
from .browser_contexts import CONTAINER, PRIVATE, CONTEXT, valid_context
from .browser_installed import BrowserInstalledRun, new_tab, correct_output
from .browser_peer import BrowserPeer
from .candidate_fixture import CandidateHandler, filename
from .candidate_input import candidate_input
from .candidate_ui import archive, empty_downloads, preference, state
from .capture import BODY
from .firefox import Firefox
from .fixture import Fixture
from .installed import ordinary, wait
from .native import Host, file_sha256, owned_architecture
from .support import new_report


class CandidateRun(BrowserInstalledRun):
    def __init__(self, package, candidate, executable, report):
        super().__init__(package, executable, report)
        self.candidate = candidate
        self.xpi, self.candidate_identity = candidate_input(candidate)  # Before setup/domain writes.
        self.tasks = {}
        self.enabled, self.access, self.saved = True, True, None
        self.cases = []

    def scope(self):
        return "owned installed companion and actual automatic-capture candidate loopback campaign"

    def additional_evidence(self):
        previous = super().additional_evidence()
        if candidate_input(self.candidate) != (self.xpi, self.candidate_identity):
            raise RuntimeError("candidate input changed")
        wanted = ["direct", "cross", "off", "navigation", "post", "frame", "blob", "newtab", "cookie", "set-cookie", "vary",
                  "container", "private", "revoked", "denied", "regranted", "restart"]
        if self.cases != wanted: raise RuntimeError("candidate campaign incomplete")
        native_cases = {"direct", "cross", "regranted"}
        for case in wanted:
            if case == "navigation": continue
            output = self.destination / filename(case) if case in native_cases else self.plan.path / ("firefox-"+filename(case))
            if not correct_output(output): raise RuntimeError("candidate output changed during cleanup")
        previous.pop("probe_xpi_sha256")
        return {**previous, "scenario": "candidate-campaign", "candidate_identity": self.candidate_identity,
                "candidate_cases": self.cases, "diagnostic_arming_used": False, "native_verified_files": 3, "firefox_verified_files": 13,
                "permission_revocation": "owned_extension_api", "permission_denial_regrant": "normal_visible_buttons",
                "public_provider_qualified": False, "browser_download_protection_parity_qualified": False}

    def open_candidate(self, peer, profile, downloads):
        browser = Firefox(self.executable, profile, {**self.environment, "MOZ_CRASHREPORTER_DISABLE": "1"}, owned_peer=peer)
        self.browsers.append(browser)
        browser.start()
        if browser.chrome("""Services.prefs.setIntPref('browser.download.folderList',2);
Services.prefs.setBoolPref('browser.download.useDownloadDir',true);
Services.prefs.setStringPref('browser.download.dir',arguments[0]);return Services.prefs.getStringPref('browser.download.dir')===arguments[0];""", [str(downloads)]) is not True:
            raise RuntimeError("owned candidate download destination unavailable")
        browser.load(self.xpi)  # Exact candidate, not a generated diagnostic. Explicit temporary load.
        browser.wait("return document.querySelector('#setting-destination').value===arguments[0];", [str(self.destination)])
        self.manager_tab = current_handle(browser)
        browser.wait("const c=document.querySelector('#automatic-capture');return !c.disabled&&!c.indeterminate&&document.querySelector('#capture-access-status').textContent.includes('Website access verified');")
        for name in self.tasks: browser.task(name, "completed")
        self.settle_state(browser)
        return browser

    def check_state(self, browser):
        browser.command("WebDriver:SwitchToWindow", {"handle": self.manager_tab})
        found = state(browser, self.tasks, self.enabled, self.access, self.saved)
        self.tasks = found
        expected = {self.destination / name for name in self.tasks}
        if set(self.destination.iterdir()) != expected or any(not correct_output(p) for p in expected):
            raise RuntimeError("candidate native output set differs")

    def settle_state(self, browser):
        # UI completion events can precede the coordinator's final storage readback.
        # Wait for the complete invariant, not a sleep or a weaker task-only receipt.
        def settled():
            try:
                self.check_state(browser)
                return True
            except (RuntimeError, ValueError):
                return False
        wait(settled)

    def choose(self, browser, enabled):
        browser.command("WebDriver:SwitchToWindow", {"handle": self.manager_tab})
        self.saved = preference(browser, enabled)
        self.enabled = enabled
        self.settle_state(browser)

    def file_case(self, browser, fixture, downloads, case, capture=False, *, redirect=None, context=None):
        self.stage = "candidate-" + case
        self.check_state(browser)
        before = handles(browser)
        tab = context if context is not None else new_tab(browser)
        browser.command("WebDriver:SwitchToWindow", {"handle": tab})
        browser.navigate(fixture.url("page/" + case))
        source = fixture.url("file/"+case)
        if case == "blob":
            source = browser.script("return document.querySelector('#download').href;")
            if not isinstance(source, str) or not source.startswith("blob:"+fixture.url("")) or len(source) > 256:
                raise RuntimeError("owned blob source differs")
        if case == "frame":
            reference = browser.command("WebDriver:FindElement", {"using": "css selector", "value": "#frame"})
            if "value" in reference: reference = reference["value"]
            browser.command("WebDriver:SwitchToFrame", {"id": reference})
            browser.wait("return window.top!==window&&!!document.querySelector('#download');")
        browser.click("#download")
        if case == "frame": browser.command("WebDriver:SwitchToFrame", {"id": None})
        if capture:
            name = filename(case)
            browser.command("WebDriver:SwitchToWindow", {"handle": self.manager_tab})
            browser.task(name, "completed")
            self.tasks[name] = None
            self.settle_state(browser)
            empty_downloads(browser, downloads)
        else:
            archive(browser, downloads, self.plan.path, case, redirect or source, case == "private")
            with fixture.lock:
                requests = sum(count for (method, route), count in fixture.requests.items() if route == "/file/"+case)
            if case != "blob" and requests != 1:
                raise RuntimeError("fallback caused competing native requests")
            self.check_state(browser)
        # New-tab targets may leave a blank auxiliary tab, or Firefox may close it.
        extras = handles(browser) - before
        if context is not None: extras.add(context)
        if len(extras) > (2 if case == "newtab" else 1) or (context is None and tab not in extras):
            raise RuntimeError("candidate case window inventory differs")
        for handle in extras:
            browser.command("WebDriver:SwitchToWindow", {"handle": handle})
            if current_handle(browser) != handle: raise RuntimeError("candidate close target differs")
            browser.command("WebDriver:CloseWindow")
        expected = before - ({context} if context is not None else set())
        if handles(browser) != expected: raise RuntimeError("candidate case windows did not retire")
        browser.command("WebDriver:SwitchToWindow", {"handle": self.manager_tab})
        self.cases.append(case)

    def transfer(self, identity):
        self.firefox_sha256 = file_sha256(self.executable)
        peer = BrowserPeer(self.owner, self.binding, self.current_binding)
        profile, downloads = self.plan.path / "Firefox", self.plan.path / "FirefoxDownloads"
        downloads.mkdir()
        target = Fixture(handler=CandidateHandler, owners=self.fixtures)
        fixture = Fixture(handler=CandidateHandler, owners=self.fixtures)
        fixture.candidate_target = target.url("file/cross")
        self.stage = "candidate-startup"
        browser = self.open_candidate(peer, profile, downloads)
        self.checkpoint("bridge-started")
        self.file_case(browser, fixture, downloads, "direct", True)
        self.file_case(browser, fixture, downloads, "cross", True)
        self.choose(browser, False)
        self.file_case(browser, fixture, downloads, "off")
        self.choose(browser, True)
        self.stage = "candidate-navigation"
        tab = new_tab(browser)
        browser.navigate(fixture.url("page/navigation")); browser.click("#download")
        browser.wait("return document.title==='Owned ordinary navigation';")
        empty_downloads(browser, downloads)
        browser.command("WebDriver:CloseWindow")
        if tab in handles(browser): raise RuntimeError("navigation tab remains")
        self.check_state(browser); self.cases.append("navigation")
        for case in ("post", "frame", "blob", "newtab", "cookie", "set-cookie", "vary"):
            self.file_case(browser, fixture, downloads, case)
        for case in ("container", "private"):
            self.stage = "candidate-context-"+case
            before = handles(browser)
            created = browser.chrome(CONTAINER if case == "container" else PRIVATE)
            if case == "container":
                if type(created) is not int or not 0 < created < 2**31: raise RuntimeError("candidate container unavailable")
            elif created is not True: raise RuntimeError("candidate private window unavailable")
            tab = new_window(browser, before)
            browser.navigate(fixture.url("page/"+case))
            valid_context(browser.chrome(CONTEXT), case, created if case == "container" else 0)
            # Return to the case context after the Manager-only state read in file_case.
            self.file_case(browser, fixture, downloads, case, context=tab)
        from .candidate_permissions import revoke, request
        self.stage = "candidate-permission-revocation"
        revoke(browser); self.access = False; self.settle_state(browser)
        self.file_case(browser, fixture, downloads, "revoked")
        self.stage = "candidate-permission-denial"
        request(browser, False); self.settle_state(browser)
        self.file_case(browser, fixture, downloads, "denied")
        self.stage = "candidate-permission-regrant"
        request(browser, True); self.access = True; self.settle_state(browser)
        self.file_case(browser, fixture, downloads, "regranted", True)
        self.checkpoint("completed")
        self.choose(browser, False)
        self.close_browser(browser)
        if self.owner._observe()[1] != identity or not self.ui.tray(self.manager_window):
            raise RuntimeError("companion did not survive candidate exit")
        self.stage = "candidate-restart"
        browser = self.open_candidate(peer, profile, downloads)
        self.file_case(browser, fixture, downloads, "restart")  # Saved Off read, never reapplied.
        self.close_browser(browser)
        if self.owner._observe()[1] != identity or not self.ui.tray(self.manager_window):
            raise RuntimeError("candidate companion identity changed")
        self.stage = "candidate-independent-native-receipts"
        host = Host(self.binding.generation, self.plan.path, self.hosts, environment=self.environment)
        architecture = owned_architecture(host)
        host.wait(lambda: len(host.tasks) >= len(self.tasks))
        if set(host.tasks) != set(self.tasks.values()): raise RuntimeError("candidate native task inventory differs")
        for task_id in self.tasks.values():
            receipt = host.command("get_handoff", {"task_id": task_id})
            task = receipt["task"]
            if (receipt["phase"] != "committed" or task["task_id"] != task_id or task["state"] != "completed"
                    or task.get("handoff_phase") != "committed" or type(task["bytes_completed"]) is not int or task["bytes_completed"] != len(BODY)):
                raise RuntimeError("candidate independent committed completion differs")
        self.checkpoint("reconnected")
        self.browser_checks.append("actual_candidate_ui_seventeen_cases_three_native_outputs_no_arming_joined_restart")
        return self.destination / filename("direct"), len(BODY), hashlib.sha256(BODY).hexdigest(), architecture


def qualify(package, candidate, executable, report):
    if not __debug__ or os.name != "nt" or ctypes.sizeof(ctypes.c_void_p) != 8:
        raise RuntimeError("candidate campaign requires 64-bit Windows Python with assertions")
    executable = executable.resolve()
    if not executable.is_file(): raise RuntimeError("Firefox executable missing")
    ordinary(candidate)
    run = CandidateRun(package, candidate, executable, new_report(report))
    try: run.execute()
    except BaseException:
        run.hold_failed_owners()
        raise
