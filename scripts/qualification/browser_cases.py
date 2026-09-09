"""Real owned-Firefox interactions; neither mocked extension APIs nor profile access."""
from pathlib import Path
import shutil
import time
import uuid

if __package__:
    from .fixture import PREFIX_SIZE, SMALL_SIZE, expected_sha256
    from .native import assignments, exact_prefix, file_sha256, owned_partial_path, retained_file
    from .support import bounded_json
else:
    from fixture import PREFIX_SIZE, SMALL_SIZE, expected_sha256
    from native import assignments, exact_prefix, file_sha256, owned_partial_path, retained_file
    from support import bounded_json

ELEMENT = "element-6066-11e4-a52e-4f735466cecf"
WIDGET = "download-manager_halcyonxp_local-browser-action"
MENU = '#contentAreaContextMenu menuitem[label="Download with Manager"]'
REMOVE_PROMPT = "Remove this task history and any retained partial? Completed downloads will not be deleted."
CANCEL_PROMPT = "Cancel this download? Partial bytes will be retained until you remove the task."


def value(result):
    return result["value"] if isinstance(result, dict) and set(result) == {"value"} else result


def handles(browser):
    result = value(browser.command("WebDriver:GetWindowHandles"))
    if not isinstance(result, list) or len(result) > 8 or not all(isinstance(item, str) for item in result):
        raise RuntimeError("unexpected owned window handles")
    return set(result)


def current_handle(browser):
    return value(browser.command("WebDriver:GetWindowHandle"))


def new_window(browser, before):
    deadline = time.monotonic() + 30
    while True:
        created = handles(browser) - before
        if created:
            if len(created) != 1:
                raise RuntimeError("unexpected additional owned window")
            handle = created.pop()
            browser.command("WebDriver:SwitchToWindow", {"handle": handle})
            return handle
        if time.monotonic() >= deadline:
            detail = browser.chrome("const w=document.getElementById(arguments[0]);return {tabs:gBrowser.tabs.length,managers:gBrowser.browsers.filter(b=>b.currentURI.spec.startsWith(arguments[1])).length,click:w?.getAttribute('data-owned-click'),panel:PanelUI.panel.state};", [WIDGET, browser.manager])
            raise RuntimeError(f"new owned window observation deadline: {detail}")
        time.sleep(0.05)


def restore_window(browser, previous):
    browser.command("WebDriver:CloseWindow")
    browser.command("WebDriver:SwitchToWindow", {"handle": previous})


def creation_values(url, name, destination, checksum):
    values = {} if url is None else {"url": url}
    values.update({"filename": name, "destination": str(destination), "workers": "4", "checksum": checksum})
    return values


def require_private_api_denial(detail):
    if not detail["manager"] or not detail["hasSubmit"] or detail["hasBrowser"]:
        raise RuntimeError("private extension capability was not denied")


def connected(browser):
    browser.wait("return document.querySelector('#connection')?.textContent === 'Helper connected';")


def open_link_menu(browser):
    reference = value(browser.command("WebDriver:FindElement", {"using": "css selector", "value": "#direct"}))
    browser.command("WebDriver:PerformActions", {"actions": [{"type": "pointer", "id": "owned-mouse", "parameters": {"pointerType": "mouse"},
        "actions": [{"type": "pointerMove", "origin": reference, "x": 0, "y": 0}, {"type": "pointerDown", "button": 2}, {"type": "pointerUp", "button": 2}]}]})
    browser.command("WebDriver:ReleaseActions")
    browser.wait("return document.getElementById('contentAreaContextMenu')?.state === 'open';", chrome=True)


def confirm(browser, expected):
    actual = value(browser.command("WebDriver:GetAlertText"))
    if actual != expected:
        raise RuntimeError("unexpected owned confirmation prompt")
    browser.command("WebDriver:AcceptAlert")


def check_output(browser, downloads, name):
    browser.task(name, "completed")
    path = downloads / name
    if path.stat().st_size != SMALL_SIZE or file_sha256(path) != expected_sha256(SMALL_SIZE):
        raise RuntimeError("actual UI output failed independent validation")


def capture_controls(browser, fixture, downloads, add):
    checks = []
    initial = current_handle(browser)
    before = handles(browser)
    browser.chrome("CustomizableUI.addWidgetToArea(arguments[0],CustomizableUI.AREA_NAVBAR);", [WIDGET])
    browser.wait("const e=document.getElementById(arguments[0]);return e && !e.hidden && e.getBoundingClientRect().width>0;", [WIDGET], chrome=True)
    browser.chrome("document.getElementById(arguments[0]).addEventListener('click',e=>document.getElementById(arguments[0]).setAttribute('data-owned-click',String(e.isTrusted)+':'+e.button),{once:true,capture:true});", [WIDGET])
    browser.click("#" + WIDGET, chrome=True)
    new_window(browser, before)
    connected(browser)
    assert browser.script("return document.querySelector('#url').value;") == ""
    add("toolbar-created.bin")
    check_output(browser, downloads, "toolbar-created.bin")
    checks.append("actual-toolbar-opens-manager-and-creates-exact-output")
    restore_window(browser, initial)

    browser.navigate(fixture.url("links"))
    before = handles(browser)
    open_link_menu(browser)
    browser.wait("const e=document.querySelector(arguments[0]);return e && !e.hidden && !e.disabled;", [MENU], chrome=True)
    browser.click(MENU, chrome=True)
    new_window(browser, before)
    connected(browser)
    expected = fixture.url("range?capture=a%2Fb&x=1&x=2")
    assert browser.script("return document.querySelector('#url').value;") == expected
    add("link-created.bin", mode=None)  # Do not replace the URL supplied by actual link capture.
    check_output(browser, downloads, "link-created.bin")
    assert fixture.link_requests > 0 and fixture.link_rejections == 0
    checks.append("actual-link-context-menu-capture-preserves-target-and-creates-exact-output")
    browser.action("link-created.bin", "Remove history")
    confirm(browser, REMOVE_PROMPT)
    browser.wait("return ![...document.querySelectorAll('.task h3')].some(e=>e.textContent==='link-created.bin');")
    assert file_sha256(downloads / "link-created.bin") == expected_sha256(SMALL_SIZE)
    checks.append("actual-remove-history-confirmation-preserves-completed-output")
    restore_window(browser, initial)
    browser.navigate(browser.manager)
    connected(browser)

    fixture.slow_body.clear()
    try:
        add("ui-cancel.bin", "slow")
        browser.task("ui-cancel.bin", "downloading")
        browser.action("ui-cancel.bin", "Cancel")
        confirm(browser, CANCEL_PROMPT)
        browser.task("ui-cancel.bin", "cancelled")
        assert not (downloads / "ui-cancel.bin").exists()
        browser.action("ui-cancel.bin", "Remove task & partial")
        confirm(browser, REMOVE_PROMPT)
        browser.wait("return ![...document.querySelectorAll('.task h3')].some(e=>e.textContent==='ui-cancel.bin');")
        checks.append("actual-cancel-and-remove-confirmations-without-publication")
    finally:
        fixture.slow_body.set()
    return checks


def content_policy(browser, fixture):
    assert fixture.csp_requests == 0
    browser.script("""const marker=document.createElement('span');marker.id='owned-csp-marker';marker.dataset.executed='no';document.body.append(marker);
const observe=e=>{if(e.effectiveDirective.startsWith('script-src'))marker.dataset.script='blocked';if(e.effectiveDirective==='img-src')marker.dataset.image='blocked';if(e.effectiveDirective==='connect-src')marker.dataset.connect='blocked';if(marker.dataset.script&&marker.dataset.image&&marker.dataset.connect)document.removeEventListener('securitypolicyviolation',observe);};
document.addEventListener('securitypolicyviolation',observe);
const script=document.createElement('script');script.textContent="document.getElementById('owned-csp-marker').dataset.executed='yes';";document.body.append(script);
const img=document.createElement('img');img.src=arguments[0];document.body.append(img);
window.wrappedJSObject.fetch(arguments[0]).then(()=>marker.dataset.fetch='allowed',()=>marker.dataset.fetch='rejected');""", [fixture.url("csp-image")])
    browser.wait("const m=document.getElementById('owned-csp-marker');return m?.dataset.script==='blocked' && m.dataset.image==='blocked' && m.dataset.connect==='blocked' && m.dataset.fetch==='rejected';")
    assert browser.script("return document.getElementById('owned-csp-marker').dataset.executed;") == "no"
    assert fixture.csp_requests == 0
    return ["actual-manager-csp-blocks-inline-script-image-and-fetch-before-network"]


def private_capture(browser, fixture, downloads):
    initial = current_handle(browser)
    before = handles(browser)
    count = browser.script("return document.querySelectorAll('.task').length;")
    with fixture.lock:
        requests_before = dict(fixture.requests)
    browser.chrome("OpenBrowserWindow({private:true});")
    new_window(browser, before)
    try:
        browser.wait("const {PrivateBrowsingUtils}=ChromeUtils.importESModule('resource://gre/modules/PrivateBrowsingUtils.sys.mjs');return PrivateBrowsingUtils.isWindowPrivate(window);", chrome=True)
        browser.navigate(fixture.url("links"))
        assert browser.chrome("const e=document.getElementById(arguments[0]);return !e || e.hidden || e.disabled || e.getBoundingClientRect().width===0;", [WIDGET])
        open_link_menu(browser)
        assert browser.chrome("const e=document.querySelector(arguments[0]);return !e || e.hidden || e.disabled || e.getBoundingClientRect().width===0;", [MENU])
        browser.chrome("document.getElementById('contentAreaContextMenu').hidePopup();")
        try:
            browser.navigate(browser.manager)
        except RuntimeError as error:
            if getattr(error, "kind", None) != "unknown error":
                raise
        detail = browser.script("return {errorPage:document.documentURI.startsWith('about:neterror?'),blank:document.documentURI==='about:blank',manager:document.documentURI===arguments[0],hasSubmit:document.querySelector('#submit')!==null,fixtureLink:document.querySelector('#direct')!==null,hasBrowser:typeof window.wrappedJSObject.browser!=='undefined'};", [browser.manager])
        # Firefox156 withholds extension APIs, not the static document. Do not
        # invent a manifest guarantee that moz-extension documents cannot load.
        require_private_api_denial(detail)
        browser.fill({"url": fixture.url("range"), "filename": "private-refused.bin", "destination": str(downloads), "workers": "4", "checksum": expected_sha256(SMALL_SIZE)})
        browser.click("#submit")
        browser.wait("return !document.querySelector('#submit').disabled && document.querySelector('#feedback').textContent==='Download was not submitted. Check the form and optional session permission.';")
        assert not (downloads / "private-refused.bin").exists()
        with fixture.lock:
            assert dict(fixture.requests) == requests_before
    finally:
        restore_window(browser, initial)
    connected(browser)
    assert browser.script("return document.querySelectorAll('.task').length;") == count
    return ["actual-private-window-withholds-extension-api-and-refuses-toolbar-link-and-form-submission"]


def input_refusal(browser, fixture, downloads):
    count = browser.script("return document.querySelectorAll('.task').length;")
    browser.fill({"url": fixture.url("range"), "filename": "invalid-input.bin", "destination": str(downloads), "checksum": "not-a-sha256"})
    browser.click("#submit")
    browser.wait("return !document.querySelector('#submit').disabled && (!document.querySelector('#checksum').validity.valid || document.querySelector('#feedback').textContent==='Download was not submitted. Check the form and optional session permission.');")
    assert browser.script("return document.querySelectorAll('.task').length;") == count
    assert not (downloads / "invalid-input.bin").exists()
    browser.fill({"checksum": "", "url": "file:///synthetic-not-a-download"})
    browser.click("#submit")
    browser.wait("return !document.querySelector('#submit').disabled && document.querySelector('#feedback').textContent==='Download was not submitted. Check the form and optional session permission.';")
    assert browser.script("return document.querySelectorAll('.task').length;") == count
    assert not (downloads / "invalid-input.bin").exists()
    browser.fill({"url": "", "checksum": ""})
    return ["actual-creation-controls-refuse-invalid-checksum-and-non-http-input"]


def task_id(browser, name, state):
    labelled = browser.task(name, state)
    identifier = labelled.removeprefix("task-")
    if labelled != "task-" + identifier or str(uuid.UUID(identifier, version=4)) != identifier:
        raise RuntimeError("unexpected actual UI task identifier")
    return identifier


def record_path(local, identifier):
    if str(uuid.UUID(identifier, version=4)) != identifier:
        raise RuntimeError("invalid browser-owned task identifier")
    return local / "HalcyonXP/FirefoxDownloadManager/state/tasks" / (identifier + ".task.json")


def read_task(local, identifier):
    record = bounded_json(record_path(local, identifier))
    assert record["version"] == 4 and record["task"]["task_id"] == identifier
    return record["task"]


def validation_cancel(browser, fixture, downloads, add):
    size = fixture.large_size
    if size != 2 * 1024**3 or shutil.disk_usage(downloads).free < size * 2 + 512 * 1024**2:
        raise RuntimeError("validation-control fixture disk/size precondition refused")
    digest = expected_sha256(size)
    fixture.large_body.clear()
    try:
        add("validation-cancel.bin", "large", checksum=digest)
        # Observe the real rendered phase, then invoke the actual control/confirm path.
        # This does not replace confirm, task state, hashing or the native transport.
        browser.script("""const marker=document.createElement('span');marker.id='owned-validation-marker';document.body.append(marker);
const inspect=()=>{const row=[...document.querySelectorAll('.task')].find(r=>r.querySelector('h3').textContent==='validation-cancel.bin');if(row?.dataset.state!=='validating')return;
observer.disconnect();marker.dataset.observed=row.dataset.state;
const buttons=[...row.querySelectorAll('button')],cancel=buttons.find(b=>b.textContent==='Cancel');
marker.dataset.controls=String(Boolean(cancel&&!cancel.disabled&&!buttons.some(b=>b.textContent==='Pause')));
if(marker.dataset.controls==='true')cancel.click();};
const observer=new MutationObserver(inspect);observer.observe(document.getElementById('tasks'),{subtree:true,childList:true,attributes:true,attributeFilter:['data-state']});inspect();""")
        fixture.large_body.set()
        deadline = time.monotonic() + 30
        while True:
            try:
                confirm(browser, CANCEL_PROMPT)
                break
            except RuntimeError as error:
                if getattr(error, "kind", None) != "no such alert":
                    raise
                if time.monotonic() >= deadline:
                    raise RuntimeError("actual validation Cancel prompt observation deadline") from None
                time.sleep(0.02)
        identifier = task_id(browser, "validation-cancel.bin", "cancelled")
        assert browser.script("const m=document.getElementById('owned-validation-marker');return m.dataset.observed==='validating' && m.dataset.controls==='true';")
        assert not (downloads / "validation-cancel.bin").exists()
        local = Path(browser.environment["LOCALAPPDATA"])
        task = read_task(local, identifier)
        assert task["state"] == "cancelled" and task["expected_sha256"] == digest
        partial = owned_partial_path(task["partial_path"], downloads)
        assert partial.stat().st_size == size
        browser.action("validation-cancel.bin", "Remove task & partial")
        confirm(browser, REMOVE_PROMPT)
        browser.wait("return ![...document.querySelectorAll('.task h3')].some(e=>e.textContent==='validation-cancel.bin');")
        assert not partial.exists() and not record_path(local, identifier).exists()
    finally:
        fixture.large_body.set()
    return ["actual-validating-phase-cancel-confirmed-no-publication-and-remove-2gib-partial"]


def prepare_restart(browser, fixture, local, downloads, add):
    mode, name = "retained-restart", "browser-retained-restart.bin"
    fixture.retained_body.clear()
    add(name, mode)
    browser.wait("const r=[...document.querySelectorAll('.task')].find(r=>r.querySelector('h3').textContent===arguments[0]);return r?.querySelector('progress').value>=arguments[1];", [name, PREFIX_SIZE])
    deadline = time.monotonic() + 30
    while True:
        with fixture.lock:
            waiting = fixture.retained_waiting[mode]
        if waiting == 4:
            break
        if time.monotonic() >= deadline:
            raise RuntimeError("browser retained-worker observation deadline")
        time.sleep(0.02)
    initial = assignments(fixture, mode)
    identifier_before = task_id(browser, name, "downloading")
    count = browser.script("return document.querySelectorAll('.task').length;")
    browser.click("#reconnect")
    connected(browser)
    assert task_id(browser, name, "downloading") == identifier_before
    assert browser.script("return document.querySelectorAll('.task').length;") == count
    assert assignments(fixture, mode) == initial
    exact_prefix([{"start": start, "end": end + 1} for start, end in initial if start < PREFIX_SIZE])
    browser.action(name, "Pause")
    identifier = task_id(browser, name, "paused")
    task = read_task(local, identifier)
    retained_file(task, downloads)
    assert not (downloads / name).exists()
    return {"task_id": identifier, "requests": initial, "ranges": task["completed_ranges"]}


def finish_restart(browser, fixture, local, downloads, retained):
    mode, name = "retained-restart", "browser-retained-restart.bin"
    try:
        identifier = task_id(browser, name, "paused")
        assert identifier == retained["task_id"]
        task = read_task(local, identifier)
        retained_file(task, downloads)
        assert task["completed_ranges"] == retained["ranges"] and task["expected_sha256"] == expected_sha256(SMALL_SIZE)
        assert assignments(fixture, mode) == retained["requests"]  # No automatic worker replay.
        browser.action(name, "Resume")
        fixture.retained_body.set()
        check_output(browser, downloads, name)
        later = assignments(fixture, mode)[len(retained["requests"]):]
        assert later and all(start >= PREFIX_SIZE for start, _ in later)
    finally:
        fixture.retained_body.set()
    return ["actual-ui-port-reconnect-preserves-task-identity-without-duplicate-submission",
            "actual-firefox-helper-restart-explicit-resume-preserves-durable-2mib-prefix-without-refetch"]


def fresh_session(browser, fixture, downloads, add):
    old_id = task_id(browser, "browser-session-recovery.bin", "failed")
    fixture.session_generation = 2  # Old cookie bytes are now rejected by the fixture.
    before = fixture.session_requests
    browser.navigate(fixture.url("session/page"))
    browser.navigate(browser.manager)
    connected(browser)
    add("browser-fresh-session.bin", "session/file?sig=a%2Fb%2BC&x=2&x=1", session=True)
    check_output(browser, downloads, "browser-fresh-session.bin")
    assert task_id(browser, "browser-fresh-session.bin", "completed") != old_id
    assert task_id(browser, "browser-session-recovery.bin", "failed") == old_id
    assert not (downloads / "browser-session-recovery.bin").exists()
    assert fixture.session_requests > before and fixture.session_rejections == 0
    assert browser.permissions() == {"cookies": True, "origins": ["http://127.0.0.1/*"]}
    return ["actual-fresh-add-after-restart-uses-renewed-cookie-without-replacing-retained-task-context"]
