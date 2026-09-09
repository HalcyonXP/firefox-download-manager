"""Owned-profile Firefox Developer Edition artifact slice; never live-profile automation.

Uses Firefox's Marionette protocol because the installed Playwright CLI cannot drive
this unpatched Firefox. No signature preference override, raw profile upload, manual
registration mutation or PID/tree-kill fallback. A failed cleanup preserves its domain.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import shutil
import socket
import subprocess
import tempfile
import time
import uuid
import winreg

if __package__:
    from .fixture import Handler, Fixture, SMALL_SIZE, expected_sha256
    from .native import evidence_identity, file_sha256
    from .support import bounded_json, new_report, write_report
    from .installation import verified_binding
else:
    from fixture import Handler, Fixture, SMALL_SIZE, expected_sha256
    from native import evidence_identity, file_sha256
    from support import bounded_json, new_report, write_report
    from installation import verified_binding

KEY = r"Software\Mozilla\NativeMessagingHosts\com.halcyonxp.firefox_download_manager"
ADDON = "download-manager@halcyonxp.local"
ELEMENT = "element-6066-11e4-a52e-4f735466cecf"
LIMIT = 4 * 1024 * 1024


class AutomationError(RuntimeError):
    def __init__(self, name, detail):
        kind = detail.get("error") if isinstance(detail, dict) else None
        allowed = {"no such alert", "unexpected alert open", "javascript error", "unsupported operation",
                   "element click intercepted", "element not interactable", "invalid argument", "script timeout",
                   "no such window", "no such element", "stale element reference", "timeout", "unknown error"}
        self.kind = kind if isinstance(kind, str) and kind in allowed else "other"
        super().__init__(f"owned Firefox command rejected: {name}; kind={self.kind}")


def closed_apps():
    executable = Path(os.environ["WINDIR"]) / "System32/tasklist.exe"
    for name in ("firefox.exe", "download-manager-native-host.exe"):
        result = subprocess.run([str(executable), "/FI", f"IMAGENAME eq {name}", "/FO", "CSV", "/NH"],
                                capture_output=True, timeout=20, check=True)
        if f'"{name}",'.encode() in result.stdout.lower():
            raise RuntimeError("Firefox/helper remains; no unowned termination is authorized")


def absent_registration():
    for hive in (winreg.HKEY_CURRENT_USER, winreg.HKEY_LOCAL_MACHINE):
        for view in (winreg.KEY_WOW64_32KEY, winreg.KEY_WOW64_64KEY):
            try:
                with winreg.OpenKey(hive, KEY, 0, winreg.KEY_READ | view):
                    raise RuntimeError("existing native registration; browser test refused")
            except FileNotFoundError:
                pass


def setup(package, action, root, environment):
    closed_apps()  # Repeat immediately before every mutation.
    result = subprocess.run([str(package / "download-manager-setup.exe"), action, "--root", str(root)],
                            env=environment, capture_output=True, timeout=90)
    if result.returncode:
        raise RuntimeError("packaged setup refused browser-test mutation: " + action)


def binding_value():
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, KEY, 0, winreg.KEY_READ | winreg.KEY_WOW64_64KEY) as key:
        value, kind = winreg.QueryValueEx(key, "")
        children, values, _ = winreg.QueryInfoKey(key)
    if kind != winreg.REG_SZ or children or values != 1:
        raise RuntimeError("unexpected browser-test registration shape")
    return value


def installed_xpi(root, package, owned_values, record=False):
    value = binding_value()
    if not record and value not in owned_values:
        raise RuntimeError("unrecorded browser-test registration; preserve for reviewed recovery")
    generation = verified_binding(root, package, value)
    if record:
        owned_values.add(value)  # Only after an explicitly completed install and verification.
    return root / generation / "firefox-download-manager.xpi"


def cleanup_domain(browser, fixture, package, root, environment, parent, installed, install_completed, owned_values):
    phase = "browser"
    try:
        try:
            if browser is not None:
                browser.close()
            phase = "installation"
            if installed:
                installed_xpi(root, package, owned_values)  # Never adopt an unrecorded binding for cleanup.
                setup(package, "uninstall", root, environment)
            phase = "registration"
            closed_apps()
            absent_registration()
        finally:
            if fixture is not None:
                try:
                    fixture.close()
                except BaseException:
                    phase = "fixture"
                    raise
        phase = "domain"
        if not install_completed or (root / "installation.json").exists() or (root / "transaction.json").exists():
            raise RuntimeError("installation cleanup authority incomplete")
        shutil.rmtree(parent)  # After all owned browser/native/fixture work joined.
    except BaseException:
        ticket = Path(__file__).resolve().parents[2] / ".git" / ("firefox28-recovery-" + uuid.uuid4().hex + ".private.json")
        with ticket.open("x", encoding="utf-8") as output:
            json.dump({"owned_domain": str(parent), "installation_root": str(root), "cleanup_phase": phase,
                       "scope": "preserved owned qualification domain; not a live profile"}, output)
        raise RuntimeError("owned browser domain preserved; no success report authorized") from None


class SessionHandler(Handler):
    def reply(self, body):
        fixture = self.server.fixture
        if self.path == "/links":
            data = b'<!doctype html><meta charset=utf-8><title>Owned capture fixture</title><a id="direct" href="/range?capture=a%2Fb&amp;x=1&amp;x=2">Direct fixture download</a>'
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(data)))
            self.send_header("Connection", "close")
            self.end_headers()
            if body:
                self.wfile.write(data)
            return
        if self.path.startswith("/range?"):
            valid = self.path == "/range?capture=a%2Fb&x=1&x=2" and self.headers.get("Cookie") is None and self.headers.get("Referer") is None
            with fixture.lock:
                fixture.link_requests += 1
                fixture.link_rejections += int(not valid)
            if not valid:
                self.send_error(403)
                return
        if self.path == "/csp-image":
            with fixture.lock:
                fixture.csp_requests += 1
            self.send_response(204)
            self.send_header("Connection", "close")
            self.end_headers()
            return
        if self.path == "/session/page":
            data = b"<!doctype html><meta charset=utf-8><title>Synthetic session fixture</title><p>Owned qualification only.</p>"
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(data)))
            self.send_header("Set-Cookie", fixture.session_cookie + "; Path=/session/; HttpOnly; SameSite=Lax")
            self.send_header("Connection", "close")
            self.end_headers()
            if body:
                self.wfile.write(data)
            return
        if self.path.startswith("/session/file"):
            valid = (self.path == "/session/file?sig=a%2Fb%2BC&x=2&x=1"
                     and self.headers.get("Cookie") == fixture.session_cookie
                     and self.headers.get("Referer") == fixture.url("session/page"))
            with fixture.lock:
                fixture.session_requests += 1
                fixture.session_rejections += int(not valid)
            if not valid:
                self.send_error(403)
                return
            original = self.path
            try:
                self.path = "/slow"
                super().reply(body)
            finally:
                self.path = original
            return
        super().reply(body)


class BrowserFixture(Fixture):
    def __init__(self):
        # Reuse the bounded owned-socket lifecycle, not a second server implementation.
        self.session_requests = 0
        self.session_rejections = 0
        self.csp_requests = 0
        self.link_requests = self.link_rejections = 0
        self.session_generation = 1
        super().__init__(handler=SessionHandler)

    @property
    def session_cookie(self):
        return f"fixture_session=not-a-real-session-{self.session_generation}"


class Firefox:
    def __init__(self, executable, profile, environment):
        self.profile = profile
        self.environment = environment
        self.executable = executable
        self.connection = None
        self.verified = False
        self.process = None
        self.serial = 0
        self.closed = False
        self.manager = None
        self.signing = None

    def start(self):
        closed_apps()
        self.profile.mkdir(exist_ok=True)
        with socket.socket() as reservation:
            reservation.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
        preferences = {
            "marionette.port": port, "marionette.enabled": True,
            "marionette.prefs.recommended": False,
            "browser.shell.checkDefaultBrowser": False,
            "browser.startup.page": 0, "browser.startup.homepage": "about:blank",
            "browser.aboutwelcome.enabled": False,
            "datareporting.policy.dataSubmissionEnabled": False,
            "toolkit.telemetry.enabled": False,
        }
        # Only an exclusively created test profile, also on restart. No signing,
        # TLS, Safe Browsing, update, proxy or sandbox preference overrides.
        (self.profile / "user.js").write_text("\n".join(
            f"user_pref({json.dumps(k)}, {json.dumps(v)});" for k, v in preferences.items()), encoding="utf-8")
        self.process = subprocess.Popen([str(self.executable), "-no-remote", "-profile", str(self.profile),
                                         "--marionette", "--remote-allow-system-access", "about:blank"],
                                        env=self.environment, stdin=subprocess.DEVNULL,
                                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        deadline = time.monotonic() + 30
        while self.connection is None:
            try:
                self.connection = socket.create_connection(("127.0.0.1", port), timeout=1)
            except OSError:
                if time.monotonic() >= deadline:
                    raise RuntimeError("owned Firefox automation did not become ready") from None
                time.sleep(0.1)
        self.connection.settimeout(30)
        greeting = self.receive()
        if greeting.get("applicationType") != "gecko" or greeting.get("marionetteProtocol") != 3:
            raise RuntimeError("unexpected automation endpoint")
        session = self.command("WebDriver:NewSession", {"capabilities": {"alwaysMatch": {"acceptInsecureCerts": False, "unhandledPromptBehavior": "ignore"}}})
        self.capabilities = session["capabilities"]
        if Path(self.capabilities["moz:profile"]).resolve() != self.profile.resolve():
            raise RuntimeError("automation endpoint does not own the test profile")
        self.verified = True
        if self.capabilities["browserName"] != "firefox" or int(self.capabilities["browserVersion"].split(".")[0]) < 156:
            raise RuntimeError("unsupported Firefox qualification version")
        self.command("WebDriver:SetTimeouts", {"script": 20000, "pageLoad": 30000, "implicit": 0})
        self.signing = self.chrome("return {value:Services.prefs.getBoolPref('xpinstall.signatures.required'), user:Services.prefs.prefHasUserValue('xpinstall.signatures.required')};")
        if self.signing["user"]:
            raise RuntimeError("test profile has a signing preference override")
        return self

    def exact(self, size):
        data = bytearray()
        while len(data) < size:
            chunk = self.connection.recv(size - len(data))
            if not chunk:
                raise RuntimeError("owned automation stream ended")
            data.extend(chunk)
        return bytes(data)

    def receive(self):
        prefix = bytearray()
        while True:
            byte = self.exact(1)
            if byte == b":":
                break
            if not byte.isdigit() or len(prefix) >= 8:
                raise RuntimeError("invalid automation frame prefix")
            prefix.extend(byte)
        if not prefix or prefix[0] == ord("0"):
            raise RuntimeError("invalid automation frame length")
        size = int(prefix)
        if not 0 < size <= LIMIT:
            raise RuntimeError("automation frame exceeds bound")
        return json.loads(self.exact(size))

    def command(self, name, arguments=None):
        if not self.verified and name != "WebDriver:NewSession":
            raise RuntimeError("no owned browser authority")
        self.serial += 1
        encoded = json.dumps([0, self.serial, name, arguments or {}], separators=(",", ":")).encode()
        if len(encoded) > LIMIT:
            raise RuntimeError("automation request exceeds bound")
        self.connection.sendall(str(len(encoded)).encode() + b":" + encoded)
        response = self.receive()
        if not isinstance(response, list) or len(response) != 4 or any(type(v) is not int for v in response[:2]) or response[:2] != [1, self.serial]:
            raise RuntimeError("uncorrelated automation response")
        if response[2] is not None:
            raise AutomationError(name, response[2])
        return response[3]

    def script(self, source, args=None, asynchronous=False):
        result = self.command("WebDriver:ExecuteAsyncScript" if asynchronous else "WebDriver:ExecuteScript",
                              {"script": source, "args": args or [], "newSandbox": True, "sandbox": "default"})
        return result.get("value") if isinstance(result, dict) and set(result) == {"value"} else result

    def chrome(self, source, args=None, asynchronous=False):
        if not self.verified:
            raise RuntimeError("no owned browser authority")
        self.command("Marionette:SetContext", {"value": "chrome"})
        try:
            return self.script(source, args, asynchronous)
        finally:
            self.command("Marionette:SetContext", {"value": "content"})

    def load(self, xpi):
        ok = self.chrome("""const done=arguments[arguments.length-1];
const {AddonManager}=ChromeUtils.importESModule('resource://gre/modules/AddonManager.sys.mjs');
const file=Components.classes['@mozilla.org/file/local;1'].createInstance(Components.interfaces.nsIFile);
file.initWithPath(arguments[0]);AddonManager.installTemporaryAddon(file).then(a=>done(a.id===arguments[1]),()=>done(false));""", [str(xpi), ADDON], True)
        if not ok:
            raise RuntimeError("temporary packaged XPI installation failed")
        information = self.chrome("""const {ExtensionParent}=ChromeUtils.importESModule('resource://gre/modules/ExtensionParent.sys.mjs');
const {AppConstants}=ChromeUtils.importESModule('resource://gre/modules/AppConstants.sys.mjs');
const ext=ExtensionParent.GlobalManager.getExtension(arguments[0]);
return {base:ext.baseURI.spec, privateAllowed:ext.privateBrowsingAllowed, channel:AppConstants.MOZ_UPDATE_CHANNEL};""", [ADDON])
        if information["channel"] != "aurora" or information["privateAllowed"] is not False:
            raise RuntimeError("unexpected Developer Edition/runtime private policy")
        self.manager = information["base"] + "manager.html"
        self.navigate(self.manager)
        self.wait("return document.querySelector('#connection')?.textContent === 'Helper connected';")

    def navigate(self, url):
        self.command("WebDriver:Navigate", {"url": url})

    def wait(self, source, args=None, timeout=30, chrome=False):
        deadline = time.monotonic() + timeout
        while True:
            value = self.chrome(source, args) if chrome else self.script(source, args)
            if value:
                return value
            if time.monotonic() >= deadline:
                raise RuntimeError("owned Firefox observation deadline")
            time.sleep(0.05)

    def fill(self, values):
        self.script("for(const [id,value] of Object.entries(arguments[0])) {const e=document.getElementById(id);e.value=value;e.dispatchEvent(new Event('input',{bubbles:true}));e.dispatchEvent(new Event('change',{bubbles:true}));}", [values])

    def click(self, selector, chrome=False):
        if chrome:
            self.command("Marionette:SetContext", {"value": "chrome"})
        try:
            reference = self.command("WebDriver:FindElement", {"using": "css selector", "value": selector})
            if "value" in reference:
                reference = reference["value"]
            if chrome:
                self.command("WebDriver:PerformActions", {"actions": [{"type": "pointer", "id": "owned-chrome-mouse", "parameters": {"pointerType": "mouse"},
                    "actions": [{"type": "pointerMove", "origin": reference, "x": 0, "y": 0}, {"type": "pointerDown", "button": 0}, {"type": "pointerUp", "button": 0}]}]})
                self.command("WebDriver:ReleaseActions")
            else:
                self.command("WebDriver:ElementClick", {"id": reference[ELEMENT]})
        finally:
            if chrome:
                self.command("Marionette:SetContext", {"value": "content"})

    def task(self, name, state):
        try:
            return self.wait("const r=[...document.querySelectorAll('.task')].find(r=>r.querySelector('h3').textContent===arguments[0]);return r?.dataset.state===arguments[1] && r.getAttribute('aria-labelledby');", [name, state])
        except RuntimeError:
            print("Bounded task-wait observation:", self.script("return {states:[...document.querySelectorAll('.task')].slice(0,8).map(r=>r.dataset.state), matchingTitle:[...document.querySelectorAll('.task')].some(r=>r.querySelector('h3').textContent===arguments[0]), submitDisabled:document.querySelector('#submit').disabled};", [name]))
            raise

    def action(self, name, label):
        # Locate by stable task title/visible action, then use a trusted WebDriver click.
        reference = self.script("const r=[...document.querySelectorAll('.task')].find(r=>r.querySelector('h3').textContent===arguments[0]);return [...r.querySelectorAll('button')].find(b=>b.textContent===arguments[1]);", [name, label])
        self.command("WebDriver:ElementClick", {"id": reference[ELEMENT]})

    def permissions(self):
        result = self.script("""const done=arguments[arguments.length-1];
window.wrappedJSObject.browser.permissions.getAll().then(p=>done({cookies:p.permissions.includes('cookies'),origins:(p.origins??[]).filter(o=>/^https?:/.test(o)).sort()}),()=>done(null));""", asynchronous=True)
        if not isinstance(result, dict):
            raise RuntimeError("owned extension permission observation failed")
        return result

    def accept_session_prompt(self):
        deadline = time.monotonic() + 15
        while True:
            accepted = self.chrome("""const n=PopupNotifications.getNotification('addon-webext-permissions');
if(!n) return false;
if(n.browser.currentURI.spec!==arguments[0]) throw Error('unexpected prompt owner');
const panel=n.owner.panel;const text=panel.textContent;
if(!text.includes('127.0.0.1')) return false;
const button=panel.querySelector('popupnotification')?.button;if(!button) return false;
button.click();return true;""", [self.manager])
            if accepted:
                return
            if time.monotonic() >= deadline:
                raise RuntimeError("expected owned optional-permission prompt not observed")
            time.sleep(0.05)

    def close(self):
        if self.closed:
            return
        if self.process is None:
            self.closed = True
            return
        configuration_error = False
        if self.verified and self.connection is not None:
            if self.signing is not None:
                try:
                    current = self.chrome("return {value:Services.prefs.getBoolPref('xpinstall.signatures.required'), user:Services.prefs.prefHasUserValue('xpinstall.signatures.required')};")
                    configuration_error = current != self.signing
                except (OSError, RuntimeError):
                    configuration_error = True
            try:
                self.command("Marionette:Quit", {"flags": ["eForceQuit"]})
            except (OSError, RuntimeError):
                # Firefox can close the transport while quitting; process absence,
                # not this exception, determines whether cleanup is authorized.
                pass
        if self.connection is not None:
            self.connection.close()
        deadline = time.monotonic() + 25
        while True:
            try:
                closed_apps()
                break
            except RuntimeError:
                if time.monotonic() >= deadline:
                    raise RuntimeError("browser cleanup unresolved; no process kill fallback") from None
                time.sleep(0.25)
        if self.process is not None:
            self.process.wait(timeout=5)  # Only the retained launcher handle.
        self.closed = True
        if configuration_error:
            raise RuntimeError("owned signing preference readback changed or failed")


def qualify(package, executable, report):
    if not __debug__:
        raise RuntimeError("qualification requires enabled assertions")
    repository = Path(__file__).resolve().parents[2]
    package, executable, report = package.resolve(), executable.resolve(), new_report(report)
    closed_apps()
    absent_registration()
    subprocess.run([str(package / "download-manager-setup.exe"), "verify"], check=True, timeout=30, capture_output=True)
    def identity():
        return {**evidence_identity(package), "firefox_harness_sha256": file_sha256(Path(__file__)),
                "browser_cases_sha256": file_sha256(Path(__file__).with_name("browser_cases.py")),
                "installation_checks_sha256": file_sha256(Path(__file__).with_name("installation.py")),
                "xpi_sha256": file_sha256(package / "firefox-download-manager.xpi"),
                "firefox_exe_sha256": file_sha256(executable)}
    before = identity()
    parent = Path(tempfile.mkdtemp(prefix="dm28 Firefox ")).resolve()
    local, roaming, profile, downloads = parent / "Local Data", parent / "Roaming", parent / "Browser Profile", parent / "Profile/Downloads"
    environment = {**os.environ, "LOCALAPPDATA": str(local), "APPDATA": str(roaming),
                   "USERPROFILE": str(downloads.parent), "HOME": str(downloads.parent),
                   "PATH": str(Path(os.environ["WINDIR"]) / "System32"), "MOZ_CRASHREPORTER_DISABLE": "1"}
    root = local / "Host With Spaces"
    fixture = browser = None
    installed = install_completed = False
    owned_values = set()
    checks = []
    ownership = repository / ".git" / ("firefox28-owner-" + uuid.uuid4().hex + ".private.json")
    with ownership.open("x", encoding="utf-8") as output:
        json.dump({"owned_domain": str(parent), "installation_root": str(root), "inputs": before,
                   "scope": "recorded before test allocation/installation; not a cleanup or release approval"}, output)
    try:
        for directory in (local, roaming, profile, downloads):
            directory.mkdir(parents=True)
        fixture = BrowserFixture()
        closed_apps()
        absent_registration()
        setup(package, "install", root, environment)
        installed = True
        install_completed = True
        xpi = installed_xpi(root, package, owned_values, record=True)
        browser = Firefox(executable, profile, environment)
        browser.start()
        browser.load(xpi)
        checks.append("packaged-temporary-xpi-actual-native-messaging-manager-page")
        if browser.script("return document.querySelectorAll('.task').length;") != 0:
            raise RuntimeError("fresh application domain was not empty")
        browser.fill({"setting-destination": str(downloads), "setting-workers": "2", "setting-retry": "0"})
        browser.click("#save-settings")
        browser.wait("return !document.querySelector('#save-settings').disabled;")
        settings = bounded_json(local / "HalcyonXP/FirefoxDownloadManager/state/settings.json")
        if settings["settings"]["default_workers"] != 2:
            raise RuntimeError("Firefox settings update was not persisted")
        checks.append("actual-ui-settings-persisted-in-owned-domain")
        digest = expected_sha256(SMALL_SIZE)
        if __package__:
            from .browser_cases import creation_values, capture_controls, content_policy, private_capture, input_refusal, validation_cancel, prepare_restart, finish_restart, fresh_session
        else:
            from browser_cases import creation_values, capture_controls, content_policy, private_capture, input_refusal, validation_cancel, prepare_restart, finish_restart, fresh_session
        def add(name, mode="range", checksum=digest, session=False):
            browser.wait("return !document.querySelector('#submit').disabled;")
            needs_prompt = session and browser.permissions() != {"cookies": True, "origins": ["http://127.0.0.1/*"]}
            # URL change deliberately proposes a filename; apply the explicit name afterward.
            browser.fill(creation_values(None if mode is None else fixture.url(mode), name, downloads, checksum))
            if not browser.script("return document.querySelector('#filename').value===arguments[0];", [name]):
                raise RuntimeError("creation filename changed before submission")
            if session:
                browser.script("document.querySelector('details').open=true;")
                browser.fill({"session-referrer": fixture.url("session/page")})
                browser.click("#session-enabled")
            browser.click("#submit")
            if needs_prompt:
                browser.accept_session_prompt()
            browser.wait("return !document.querySelector('#submit').disabled && document.querySelector('#feedback').textContent===`Added ${arguments[0]}. The helper now owns this download.`;", [name])
        checks.extend(capture_controls(browser, fixture, downloads, add))
        checks.extend(content_policy(browser, fixture))
        checks.extend(private_capture(browser, fixture, downloads))
        checks.extend(input_refusal(browser, fixture, downloads))
        checks.extend(validation_cancel(browser, fixture, downloads, add))
        add("browser-checksum.bin")
        browser.task("browser-checksum.bin", "completed")
        if file_sha256(downloads / "browser-checksum.bin") != digest:
            raise RuntimeError("Firefox-created output did not match independent digest")
        checks.append("actual-ui-sha256-completion-exact-output")
        add("browser-mismatch.bin", checksum="f" * 64)
        browser.task("browser-mismatch.bin", "failed")
        if (downloads / "browser-mismatch.bin").exists() or not browser.script("return document.querySelector('#tasks').textContent.includes('CHECKSUM_MISMATCH');"):
            raise RuntimeError("checksum mismatch UI/publication boundary failed")
        checks.append("actual-ui-checksum-mismatch-no-publication")
        fixture.slow_body.clear()
        try:
            add("browser-pause.bin", "slow")
            browser.task("browser-pause.bin", "downloading")
            browser.action("browser-pause.bin", "Pause")
            browser.task("browser-pause.bin", "paused")
        finally:
            fixture.slow_body.set()
        browser.action("browser-pause.bin", "Resume")
        browser.task("browser-pause.bin", "completed")
        if file_sha256(downloads / "browser-pause.bin") != digest:
            raise RuntimeError("Firefox pause/resume output mismatch")
        checks.append("actual-ui-pause-resume-exact-output")
        if browser.permissions() != {"cookies": False, "origins": []}:
            raise RuntimeError("unexpected initial optional authority")
        browser.navigate(fixture.url("session/page"))
        browser.navigate(browser.manager)
        browser.wait("return document.querySelector('#connection')?.textContent === 'Helper connected';")
        add("browser-session.bin", "session/file?sig=a%2Fb%2BC&x=2&x=1", session=True)
        browser.task("browser-session.bin", "completed")
        if file_sha256(downloads / "browser-session.bin") != digest or fixture.session_rejections or not fixture.session_requests:
            raise RuntimeError("actual session handoff/target did not validate")
        if browser.script("return document.querySelector('#session-enabled').checked || document.querySelector('#session-referrer').value!=='' || document.querySelector('#session-authorization').value!=='';"):
            raise RuntimeError("session inputs were not reset")
        if browser.permissions() != {"cookies": True, "origins": ["http://127.0.0.1/*"]}:
            raise RuntimeError("optional grant exceeded or missed selected authority")
        checks.append("actual-optional-prompt-httponly-cookie-referrer-exact-signed-target-output-reset")
        browser.script("document.querySelector('details').open=true;")
        browser.click("#revoke-session")
        browser.wait("return document.querySelector('#feedback').textContent.startsWith('Optional permissions revoked.');")
        if browser.permissions() != {"cookies": False, "origins": []}:
            raise RuntimeError("optional authority remained after UI revocation")
        checks.append("actual-ui-optional-permission-revocation-verified-by-browser-api")
        fixture.slow_body.clear()
        try:
            add("browser-session-recovery.bin", "session/file?sig=a%2Fb%2BC&x=2&x=1", session=True)
            browser.task("browser-session-recovery.bin", "downloading")
            browser.action("browser-session-recovery.bin", "Pause")
            browser.task("browser-session-recovery.bin", "paused")
        finally:
            fixture.slow_body.set()
        retained = prepare_restart(browser, fixture, local, downloads, add)
        version = browser.capabilities["browserVersion"]
        signing = browser.signing
        browser.close()
        browser = Firefox(executable, profile, environment)
        browser.start()
        browser.load(installed_xpi(root, package, owned_values))
        browser.task("browser-session-recovery.bin", "paused")
        browser.action("browser-session-recovery.bin", "Resume")
        browser.task("browser-session-recovery.bin", "failed")
        if not browser.script("const r=[...document.querySelectorAll('.task')].find(r=>r.querySelector('h3').textContent==='browser-session-recovery.bin');return r.textContent.includes('AUTH_REQUIRED');") or (downloads / "browser-session-recovery.bin").exists():
            raise RuntimeError("session-loss restart did not refuse retained task")
        if browser.signing != signing or browser.capabilities["browserVersion"] != version:
            raise RuntimeError("browser configuration/version changed on owned restart")
        checks.append("actual-firefox-and-helper-restart-session-loss-explicit-resume-refused")
        checks.extend(finish_restart(browser, fixture, local, downloads, retained))
        checks.extend(fresh_session(browser, fixture, downloads, add))
        browser.close()
        browser = None
        # Existing qualified packaging coverage calls this same-version upgrade,
        # not compatibility with a nonexistent earlier release.
        task_before = {p.name: file_sha256(p) for p in downloads.iterdir()}
        installed_xpi(root, package, owned_values)
        setup(package, "install", root, environment)
        installed_xpi(root, package, owned_values, record=True)
        setup(package, "cleanup", root, environment)
        installed_xpi(root, package, owned_values)
        setup(package, "uninstall", root, environment)
        installed = False
        absent_registration()
        if task_before != {p.name: file_sha256(p) for p in downloads.iterdir()}:
            raise RuntimeError("upgrade/removal changed completed output")
        checks.append("same-artifact-upgrade-cleanup-uninstall-preserve-output")
        if before != identity():
            raise RuntimeError("browser/artifact/harness identity changed during qualification")
        evidence = {**before, "os": platform.platform(), "harness_process_machine": platform.machine(),
                    "firefox_version": version, "firefox_channel": "aurora", "signing_preference_unchanged": signing,
                    "runtime_private_browsing_allowed": False, "checks": checks,
                    "scope": "actual isolated Firefox artifact capture/controls/CSP/private-capture/session slice; not every restart/support-matrix condition or release approval"}
    finally:
        if fixture is not None:
            print("Bounded fixture counts:", sorted((list(k), v) for k, v in fixture.requests.items()))
        cleanup_domain(browser, fixture, package, root, environment, parent, installed, install_completed, owned_values)
    write_report(report, evidence)
    print("Actual Firefox artifact slice passed; full qualification remains separate.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--firefox", type=Path, default=Path(r"C:\Program Files\Firefox Developer Edition\firefox.exe"))
    parser.add_argument("--report", type=Path, required=True)
    arguments = parser.parse_args()
    qualify(arguments.package, arguments.firefox, arguments.report)
