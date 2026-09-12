"""Opt-in loopback Firefox API evidence, not persistent XPI or native handoff qualification.

Keeps the existing combined driver's closed-app preflight. No registration writes,
normal-profile access, nativeMessaging permission or protection overrides.
"""
import json
import os
from pathlib import Path
import time
import uuid
import zipfile

from .browser_cases import current_handle, value
from .firefox import Firefox, absent_registration, closed_apps
from .fixture import Fixture, Handler
from .native import file_sha256
from .setup_owner import DomainPlan
from .support import ARTIFACTS, new_report, write_report

ROOT = Path(__file__).resolve().parents[2]
SOURCE = Path(__file__).with_name("capture_probe")
BODY = b"owned capture fixture\n" * 64
PAGE = b'''<!doctype html><meta charset=utf-8><title>Owned capture API fixture</title>
<a id="direct" href="/direct">Direct</a>
<a id="redirect" href="/redirect?fixture=a%2Fb&amp;x=1&amp;x=2">Redirect attachment</a>
<a id="navigation" href="/navigation">Navigation</a>
<form method="post" action="/post"><button id="post">POST</button></form>
<iframe src="/frame"></iframe>'''


class CaptureHandler(Handler):
    def do_POST(self):
        # Fixed fixture form has no successful controls/body. Refuse other input.
        if self.headers.get("Content-Length") != "0":
            self.send_error(400)
            return
        self.reply(True)

    def reply(self, body):
        route = self.path.split("?", 1)[0]
        status, data, content_type, attachment, redirect = 200, BODY, "application/octet-stream", False, None
        if self.path == "/page":
            data, content_type = PAGE, "text/html; charset=utf-8"
        elif self.path in ("/navigation", "/post", "/frame"):
            data, content_type = b"<!doctype html><title>Owned navigation</title><p>Owned page</p>", "text/html; charset=utf-8"
        elif self.path == "/redirect?fixture=a%2Fb&x=1&x=2":
            status, data, redirect = 302, b"", "/attachment?fixture=a%2Fb&x=1&x=2"
        elif self.path in ("/direct", "/attachment?fixture=a%2Fb&x=1&x=2"):
            attachment = True
        else:
            status, data = 404, b""
        with self.server.fixture.lock:
            self.server.fixture.requests[(self.command, route if route in
                {"/page", "/navigation", "/post", "/frame", "/direct", "/redirect", "/attachment"} else "other")] += 1
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Connection", "close")
        if attachment:
            self.send_header("Content-Disposition", 'attachment; filename="owned-capture.bin"')
        if redirect:
            self.send_header("Location", redirect)
        self.end_headers()
        if body:
            self.wfile.write(data)


def build_probe(domain):
    """Unique diagnostic identity, bounded source-only ZIP; never uses Manager identity."""
    identity = "capture-probe-" + uuid.uuid4().hex + "@example.invalid"
    manifest = {
        "manifest_version": 3, "name": "Owned loopback capture API probe", "version": "0.0.1",
        "browser_specific_settings": {"gecko": {"id": identity, "strict_min_version": "156.0"}},
        "permissions": ["webRequest", "webRequestBlocking"],
        "host_permissions": ["http://127.0.0.1/*"], "incognito": "not_allowed",
        "background": {"scripts": ["background.js"], "persistent": False},
        "content_scripts": [{"matches": ["http://127.0.0.1/page"], "js": ["click.js"], "run_at": "document_start"}],
        "content_security_policy": {"extension_pages": "default-src 'none'; script-src 'self'; object-src 'none'"},
    }
    payloads = {"manifest.json": json.dumps(manifest).encode(), "inspect.html": b"<!doctype html><title>Owned API probe</title>"}
    for name in ("background.js", "click.js"):
        data = (SOURCE / name).read_bytes()
        if not 0 < len(data) <= 32 * 1024:
            raise RuntimeError("capture probe source exceeds bound")
        payloads[name] = data
    path = domain / "capture-probe.xpi"
    with zipfile.ZipFile(path, "x", compression=zipfile.ZIP_STORED) as archive:
        for name, data in payloads.items():
            archive.writestr(name, data)
    return path, identity


def load_probe(browser, xpi, identity):
    result = browser.chrome("""const done=arguments[arguments.length-1];
const {AddonManager}=ChromeUtils.importESModule('resource://gre/modules/AddonManager.sys.mjs');
const file=Components.classes['@mozilla.org/file/local;1'].createInstance(Components.interfaces.nsIFile);
file.initWithPath(arguments[0]);AddonManager.installTemporaryAddon(file).then(a=>done(a.id===arguments[1]),()=>done(false));""", [str(xpi), identity], True)
    if result is not True:
        raise RuntimeError("owned temporary API probe refused")
    info = browser.chrome("""const {ExtensionParent}=ChromeUtils.importESModule('resource://gre/modules/ExtensionParent.sys.mjs');
const {AppConstants}=ChromeUtils.importESModule('resource://gre/modules/AppConstants.sys.mjs');
const ext=ExtensionParent.GlobalManager.getExtension(arguments[0]);
return {base:ext.baseURI.spec,privateAllowed:ext.privateBrowsingAllowed,channel:AppConstants.MOZ_UPDATE_CHANNEL};""", [identity])
    if info["privateAllowed"] is not False or info["channel"] != "aurora":
        raise RuntimeError("unexpected API probe browser policy")
    return info["base"] + "inspect.html"


def message(browser, inspector, action):
    # A dedicated tab prevents observation from navigating/cancelling the test
    # request and manufacturing the terminal event we are trying to establish.
    previous = current_handle(browser)
    browser.command("WebDriver:SwitchToWindow", {"handle": inspector})
    try:
        return browser.script("""const done=arguments[arguments.length-1];
window.wrappedJSObject.browser.runtime.sendMessage(arguments[0]).then(done,()=>done(null));""", [action], True)
    finally:
        browser.command("WebDriver:SwitchToWindow", {"handle": previous})


def inspect_case(snapshot, kind):
    """Require positive API observations, not absence after a sleep or guessed fields."""
    if not isinstance(snapshot, dict) or snapshot.get("overflow") is not False:
        raise RuntimeError("capture observations missing or overflowed")
    records = snapshot.get("records")
    if not isinstance(records, list) or not 0 < len(records) <= 64 or not all(isinstance(r, dict) for r in records):
        raise RuntimeError("invalid capture observation shape")
    if kind not in {"direct", "redirect", "navigation", "post", "frame", "allow-direct"}:
        raise RuntimeError("unknown capture case")
    target = "attachment" if kind == "redirect" else "direct" if kind == "allow-direct" else kind
    headers = [r for r in records if r.get("stage") == "headers" and r.get("route") == target]
    if len(headers) != 1:
        raise RuntimeError("expected single target response")
    response = headers[0]
    if response.get("private") is not False or response.get("store") != "default" or response.get("status") != 200:
        raise RuntimeError("browser context fields not established")
    capture = kind in ("direct", "redirect")
    attachment = capture or kind == "allow-direct"
    if response.get("cancel") is not capture or response.get("attachment") is not attachment:
        raise RuntimeError("unexpected browser cancellation decision")
    if response.get("method") != ("POST" if kind == "post" else "GET"):
        raise RuntimeError("browser method not established")
    if response.get("type") != ("sub_frame" if kind == "frame" else "main_frame"):
        raise RuntimeError("browser frame classification not established")
    if response.get("topFrame") is not (kind != "frame"):
        raise RuntimeError("browser frame identity not established")
    terminal = "error" if capture else "completed"
    identifier = response.get("request")
    if type(identifier) is not int or not 1 <= identifier <= 64:
        raise RuntimeError("invalid request identity")
    terminals = [r for r in records if r.get("stage") in {"completed", "error"} and r.get("request") == identifier]
    if len(terminals) != 1 or terminals[0].get("stage") != terminal:
        raise RuntimeError("no unique correlated browser terminal event")
    if capture and terminals[0].get("errorKind") not in {"NS_ERROR_ABORT", "NS_ERROR_BLOCKED_BY_POLICY"}:
        raise RuntimeError("browser cancellation outcome not established")
    if attachment and (response.get("correlated") is not True or not any(
            r.get("stage") == "click" and r.get("route") == ("direct" if kind == "allow-direct" else kind)
            and r.get("trusted") is True for r in records)):
        raise RuntimeError("trusted click correlation not established")
    if kind == "redirect" and not any(r.get("stage") == "redirect" and
            r.get("request") == response.get("request") and r.get("target") == "attachment" for r in records):
        raise RuntimeError("redirect request identity not established")
    return {"case": kind, "method": response["method"], "type": response["type"],
            "private": False, "store": "default", "attachment": attachment, "cancel": capture,
            "trusted_click_correlated": attachment, "terminal": terminal,
            "error_kind": terminals[0].get("errorKind") if capture else None, "events": len(records)}


def require_joined_success(browser, fixtures):
    if (browser is None or not browser.closed or browser.process is None or
            browser.process.returncode != 0 or not fixtures or not all(fixture.closed for fixture in fixtures)):
        raise RuntimeError("API probe requires successful browser exit and joined fixture retirement")


def run(executable, report):
    if not __debug__:
        raise RuntimeError("probe requires enabled assertions")
    executable, report = executable.resolve(), new_report(report)
    closed_apps()
    absent_registration()
    if not executable.is_file():
        raise RuntimeError("Firefox executable missing")
    ARTIFACTS.mkdir(exist_ok=True)
    plan = DomainPlan.record(ARTIFACTS, ROOT / ".git")
    browser = None
    fixtures = []
    checks = []
    failure = cleanup_failure = False
    identity = {"driver_sha256": file_sha256(Path(__file__)), "firefox_driver_sha256": file_sha256(Path(__file__).with_name("firefox.py")),
                "firefox_exe_sha256": file_sha256(executable),
                "background_sha256": file_sha256(SOURCE / "background.js"), "click_sha256": file_sha256(SOURCE / "click.js")}
    try:
        plan.create()
        profile, downloads = plan.path / "browser", plan.path / "downloads"
        for name in ("browser", "downloads", "local", "roaming", "profile"):
            (plan.path / name).mkdir()
        environment = {**os.environ, "LOCALAPPDATA": str(plan.path / "local"), "APPDATA": str(plan.path / "roaming"),
                       "USERPROFILE": str(plan.path / "profile"), "HOME": str(plan.path / "profile"),
                       "PATH": str(Path(os.environ["WINDIR"]) / "System32"), "MOZ_CRASHREPORTER_DISABLE": "1"}
        xpi, addon = build_probe(plan.path)
        identity["probe_xpi_sha256"] = file_sha256(xpi)
        fixture = Fixture(handler=CaptureHandler, owners=fixtures)
        browser = Firefox(executable, profile, environment)
        browser.start()
        # Confine any browser fallback output BEFORE any fixture request. These
        # preferences affect only owned test downloads, not security protections.
        result = browser.chrome("""Services.prefs.setIntPref('browser.download.folderList',2);
Services.prefs.setBoolPref('browser.download.useDownloadDir',true);
Services.prefs.setStringPref('browser.download.dir',arguments[0]);
return Services.prefs.getStringPref('browser.download.dir')===arguments[0] &&
Services.prefs.getIntPref('browser.download.folderList')===2;""", [str(downloads)])
        if result is not True:
            raise RuntimeError("owned browser download destination not established")
        browser.navigate(load_probe(browser, xpi, addon))
        inspector = current_handle(browser)
        test_tab = value(browser.command("WebDriver:NewWindow", {"type": "tab"}))["handle"]
        browser.command("WebDriver:SwitchToWindow", {"handle": test_tab})
        for kind in ("direct", "redirect", "navigation", "post", "frame", "allow-direct"):
            if message(browser, inspector, {"action": "reset", "cancel": kind != "allow-direct"}) != {"ready": True}:
                raise RuntimeError("API observer not ready")
            browser.navigate(fixture.url("page"))
            if kind != "frame":
                browser.click("#direct" if kind == "allow-direct" else "#" + kind)
            # Poll the extension's bounded data until the target terminal event;
            # inspector navigation is not proof that the earlier request ended.
            deadline = time.monotonic() + 10
            while True:
                snapshot = message(browser, inspector, {"action": "snapshot"})
                with (plan.path / f"{kind}.private.json").open("w", encoding="utf-8") as output:
                    json.dump(snapshot, output)
                try:
                    check = inspect_case(snapshot, kind)
                    break
                except RuntimeError:
                    if time.monotonic() >= deadline:
                        raise
                    time.sleep(.05)
            checks.append(check)
            if kind != "allow-direct" and any(downloads.iterdir()):
                raise RuntimeError("unexpected competing fixture output")
        # Unarmed control: let Firefox finish the same attachment and independently
        # verify its output. This distinguishes interception from a broken fixture.
        expected = downloads / "owned-capture.bin"
        deadline = time.monotonic() + 10
        while not expected.is_file():
            if time.monotonic() >= deadline:
                raise RuntimeError("Firefox fallback output not observed")
            time.sleep(.05)
        if set(downloads.iterdir()) != {expected} or expected.read_bytes() != BODY:
            raise RuntimeError("Firefox fallback output differs or remains incomplete")
        checks[-1]["output_sha256"] = file_sha256(expected)
        checks[-1]["output_size"] = len(BODY)
    except BaseException as error:
        failure = True
        if plan.created:
            try:
                with (plan.path / "failure.private.json").open("x", encoding="utf-8") as output:
                    json.dump({"qualification": False, "failure": type(error).__name__,
                               "detail": str(error)[:256] if isinstance(error, RuntimeError) else "withheld"}, output)
            except OSError:
                pass  # Diagnostic failure cannot skip owned retirement.
    finally:
        if browser is not None:
            try:
                browser.close()
            except BaseException:
                cleanup_failure = True
        for fixture in fixtures:
            try:
                fixture.close()
            except BaseException:
                cleanup_failure = True
    if cleanup_failure:
        # Keep the exact objects alive; do not kill discovered Firefox processes.
        # Manual closure of the owned window can resolve ownership, never failure.
        print("API probe failed cleanup; retaining owned actors. Close only the owned test Firefox window.", flush=True)
        while True:
            try:
                if browser is not None:
                    browser.close()
                for fixture in fixtures:
                    fixture.close()
                break
            except BaseException:
                time.sleep(1)
    closed_apps()
    absent_registration()
    if failure or cleanup_failure:
        raise RuntimeError("API probe failed; private owned domain preserved")
    require_joined_success(browser, fixtures)
    write_report(report, {"format": "firefox-capture-api-probe", "version": 1, "qualification": False,
        "scope": "temporary diagnostic XPI; loopback request API only; no native handoff or persistent installation",
        "identity": identity, "checks": checks, "joined": True, "browser_exit": 0, "registration_unchanged_absent": True})
