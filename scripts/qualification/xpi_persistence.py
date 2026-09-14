"""Opt-in normal install UI and clean-restart observation; no registration writes."""
import ctypes
import json
import os
from pathlib import Path
import subprocess
import time

from .browser_peer import process_inventory
from .capture import require_joined_success
from .firefox import Firefox
from .fixture import Fixture, Handler
from .installed import closed_apps, package_input, preflight_module, ordinary
from .native import file_sha256
from .setup_owner import DomainPlan
from .support import ARTIFACTS, new_report, write_report
from .xpi_policy import ADDON, inspect_xpi, persistent_receipt
from .xpi_ui import observe_install, protections, receipt

ROOT = Path(__file__).resolve().parents[2]


def preflight():
    policy = preflight_module(); closed_apps(policy); policy.all_views_absent()
    if any(process_inventory().values()): raise RuntimeError("closed application inventory required")


def require_final_owners(outcome, browsers, fixtures):
    if outcome not in ("installed", "signature-requirement-observed") or len(browsers) != (2 if outcome == "installed" else 1):
        raise RuntimeError("browser lifecycle count differs")
    for retired in browsers: require_joined_success(retired, fixtures)


def handler_for(data):
    class InstallHandler(Handler):
        def reply(self, body):
            if self.command not in ("GET", "HEAD") or self.path not in ("/page", "/manager.xpi"):
                self.send_error(404); return
            payload = data if self.path == "/manager.xpi" else b'<!doctype html><title>Owned XPI installation</title><a id="install" href="/manager.xpi">Install reviewed Manager XPI</a>'
            with self.server.fixture.lock:
                self.server.fixture.requests[(self.command, self.path)] += 1
                if sum(self.server.fixture.requests.values()) > 16:
                    self.send_error(429); return
            self.send_response(200)
            self.send_header("Content-Type", "application/x-xpinstall" if self.path == "/manager.xpi" else "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(payload))); self.send_header("Connection", "close")
            self.end_headers()
            if body: self.wfile.write(payload)
    return InstallHandler


class PersistenceRun:
    def __init__(self, package, executable, report):
        self.package, self.executable, self.report = package, executable, report
        self.browsers, self.fixtures = [], []
        self.plan = None
        self.stage = "preflight"

    def open(self, profile, environment):
        preflight()
        browser = Firefox(self.executable, profile, environment)
        self.browsers.append(browser)  # Before process/profile creation, including failed start.
        browser.start()
        if browser.chrome("return ChromeUtils.importESModule('resource://gre/modules/AppConstants.sys.mjs').AppConstants.MOZ_UPDATE_CHANNEL;") != "aurora":
            raise RuntimeError("Developer Edition channel required")
        return browser

    @staticmethod
    def close(browser):
        browser.close()
        if not browser.closed or browser.process is None or browser.process.wait(timeout=0) != 0:
            raise RuntimeError("successful retained Firefox exit required")

    def retire(self):
        failed = False
        for owner in [*reversed(self.browsers), *self.fixtures]:
            try: owner.close()
            except BaseException: failed = True
        return not failed

    def execute(self):
        preflight(); descriptor = package_input(self.package)
        xpi = self.package / "firefox-download-manager.xpi"; ordinary(xpi)
        with xpi.open("rb") as source: data = source.read(8 * 1024 * 1024 + 1)
        identity = inspect_xpi(data); browser_hash = file_sha256(self.executable)
        ARTIFACTS.mkdir(exist_ok=True)
        self.plan = DomainPlan.record(ARTIFACTS, ROOT / ".git")
        result = None
        try:
            self.plan.create()
            for folder in ("local", "roaming", "home"): (self.plan.path / folder).mkdir()
            environment = {**os.environ, "LOCALAPPDATA": str(self.plan.path / "local"), "APPDATA": str(self.plan.path / "roaming"),
                           "USERPROFILE": str(self.plan.path / "home"), "HOME": str(self.plan.path / "home"),
                           "PATH": str(Path(os.environ["WINDIR"]) / "System32"), "MOZ_CRASHREPORTER_DISABLE": "1"}
            profile = self.plan.path / "Firefox"
            fixture = Fixture(handler=handler_for(data), owners=self.fixtures)
            self.stage = "initial-browser"
            browser = self.open(profile, environment)
            self.stage = "normal-install-ui"
            result, seen, before = observe_install(browser, fixture.url("page"), fixture.url("manager.xpi"), identity)
            self.close(browser)
            if result == "installed":
                installed = profile / "extensions" / f"{ADDON}.xpi"; ordinary(installed)
                if installed.stat().st_size != len(data) or file_sha256(installed) != identity["xpi_sha256"]: raise RuntimeError("installed XPI bytes differ")
                self.stage = "restart-without-install"
                browser = self.open(profile, environment)  # No load/install call on restart.
                browser.wait("return !!WebExtensionPolicy.getByID(arguments[0])?.active;", [ADDON], chrome=True)
                persistent_receipt(receipt(browser, identity), identity)
                if protections(browser) != before: raise RuntimeError("restart protections changed")
                self.close(browser)
                if installed.stat().st_size != len(data) or file_sha256(installed) != identity["xpi_sha256"]: raise RuntimeError("restarted XPI bytes differ")
            for fixture in self.fixtures: fixture.close()
            self.stage = "final-verification"
            preflight()
            require_final_owners(result, self.browsers, self.fixtures)
            if package_input(self.package) != descriptor or file_sha256(xpi) != identity["xpi_sha256"] or file_sha256(self.executable) != browser_hash:
                raise RuntimeError("source artifact identity changed")
            write_report(self.report, {"format": "firefox-xpi-persistence-observation", "version": 1, "qualification": False, "m5_install_ready": False, "scope": "owned normal XPI UI/persistence observation; no native handoff",
                "outcome": result, "persistent_install_observed": result == "installed", "temporary_loading_used": False,
                "approval_clicks_attempted": sorted(seen), "permission_step_followed_by_active_receipt": result == "installed", "initial_owned_protections": before, "protections_unchanged": True,
                "identity": identity, "firefox_exe_sha256": browser_hash, "package_source_commit": descriptor["commit"],
                "harness_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True, timeout=15).strip(),
                "harness_worktree_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, timeout=15)),
                "successful_browser_exits": len(self.browsers), "joined": True, "registration_unchanged_absent": True})
        except BaseException as error:
            try:
                frames = []; trace = error.__traceback__
                while trace is not None and len(frames) < 12:
                    frames.append({"function": trace.tb_frame.f_code.co_name, "line": trace.tb_lineno}); trace = trace.tb_next
                if self.plan.created:
                    with (self.plan.path / "failure.private.json").open("x", encoding="utf-8") as out:
                        json.dump({"stage": self.stage, "frames": frames}, out)
            except BaseException: pass  # Recording failure never skips retained-owner retirement.
            if not self.retire():
                print("XPI observation cleanup failed; retaining exact browser/fixture owners. Close only the owned test window.", flush=True)
                while not self.retire(): time.sleep(1)
            raise RuntimeError("XPI observation refused; owned domain preserved; no acceptance claimed") from None
        return result


def run(package, executable, report):
    if not __debug__ or os.name != "nt" or ctypes.sizeof(ctypes.c_void_p) != 8:
        raise RuntimeError("owned XPI observation requires assertions and 64-bit Windows")
    return PersistenceRun(package.resolve(), executable.resolve(), new_report(report)).execute()
