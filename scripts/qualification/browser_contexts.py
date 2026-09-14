"""Owned container/private fallback observations; no new extension permissions."""
from .browser_cases import handles, new_window, current_handle
from .capture import BODY, message
from .browser_installed import correct_output, wait

CONTAINER = """const {ContextualIdentityService}=ChromeUtils.importESModule('moz-src:///toolkit/components/contextualidentity/ContextualIdentityService.sys.mjs');
const identity=ContextualIdentityService.create('Owned capture fixture','fingerprint','blue');
if(!Number.isSafeInteger(identity.userContextId)||identity.userContextId<=0)throw new Error('container unavailable');
const tab=gBrowser.addTab('about:blank',{userContextId:identity.userContextId,triggeringPrincipal:Services.scriptSecurityManager.getSystemPrincipal()});
gBrowser.selectedTab=tab;return identity.userContextId;"""
PRIVATE = "OpenBrowserWindow({private:true});return true;"
CONTEXT = """const {PrivateBrowsingUtils}=ChromeUtils.importESModule('resource://gre/modules/PrivateBrowsingUtils.sys.mjs');
return {private:PrivateBrowsingUtils.isBrowserPrivate(gBrowser.selectedBrowser),
container:gBrowser.selectedBrowser.contentPrincipal.originAttributes.userContextId};"""


def valid_context(value, kind, container):
    if (kind not in ("container", "private") or not isinstance(value, dict) or set(value) != {"private", "container"}
            or type(value["private"]) is not bool or type(value["container"]) is not int
            or value != {"private": kind == "private", "container": container}):
        raise RuntimeError("owned browsing context differs")
    if type(container) is not int or (container <= 0 if kind == "container" else container != 0):
        raise RuntimeError("owned context identity refused")


def untouched(snapshot):
    if (not isinstance(snapshot, dict) or snapshot.get("qualification") is not False
            or snapshot.get("connected") is not True or snapshot.get("enabled") is not True
            or snapshot.get("phaseMetadataAvailable") is not True or snapshot.get("blocked") is not False
            or snapshot.get("overflow") is not False or type(snapshot.get("taskCount")) is not int
            or snapshot.get("taskCount") != 0 or snapshot.get("tasks") != []
            or snapshot.get("pending") != [] or snapshot.get("records") != []):
        raise RuntimeError("unsupported context created native work or capture was unavailable")
    preference = snapshot.get("capturePreference")
    if (not isinstance(preference, dict) or any(type(item) is not bool for item in preference.values())
            or preference != {"available": True, "ready": True, "enabled": True, "busy": False, "failed": False}):
        raise RuntimeError("capture preference unavailable for negative control")


def context_observations(snapshot):
    # Private execution is denied by the unchanged manifest: its requests must not
    # appear in the extension. The separate owned chrome-context check establishes it.
    expected = [{"method": "GET", "frame": "main", "store": "other", "private": False}]
    if snapshot.get("contexts") != expected or type(snapshot["contexts"][0].get("private")) is not bool:
        raise RuntimeError("nondefault request context observation differs")


def archive_output(browser, downloads, root, kind, source):
    if kind not in ("container", "private"): raise RuntimeError("unsupported fixture kind")
    output = downloads / "owned-capture.bin"
    def completed():
        return browser.chrome("""const done=arguments[arguments.length-1];
const {Downloads}=ChromeUtils.importESModule('resource://gre/modules/Downloads.sys.mjs');
Downloads.getList(arguments[2]?Downloads.PRIVATE:Downloads.PUBLIC).then(l=>l.getAll()).then(items=>done(items.length===1&&items.every(d=>
d.source.url===arguments[0]&&d.target.path===arguments[1]&&d.succeeded&&d.stopped&&!d.error&&!d.canceled&&d.currentBytes===arguments[3])),()=>done(false));""",
            [source, str(output), kind == "private", len(BODY)], True) is True
    wait(completed, 15)
    if set(downloads.iterdir()) != {output} or not correct_output(output):
        raise RuntimeError("unsupported context Firefox output differs")
    removed = browser.chrome("""const done=arguments[arguments.length-1];
const {Downloads}=ChromeUtils.importESModule('resource://gre/modules/Downloads.sys.mjs');
Downloads.getList(arguments[2]?Downloads.PRIVATE:Downloads.PUBLIC).then(async list=>{const entries=await list.getAll();
if(entries.length!==1)return false;const d=entries[0];
if(d.source.url!==arguments[0]||d.target.path!==arguments[1]||!d.succeeded||!d.stopped||d.error||d.canceled||d.currentBytes!==arguments[3])return false;
await list.remove(d);return (await list.getAll()).length===0;}).then(done,()=>done(false));""", [source, str(output), kind == "private", len(BODY)], True)
    if removed is not True: raise RuntimeError("owned context history removal refused")
    data = output.read_bytes()
    if data != BODY: raise RuntimeError("owned context fixture changed before archival")
    with (root / f"firefox-{kind}.bin").open("xb") as archive: archive.write(data)
    output.unlink()
    if any(downloads.iterdir()): raise RuntimeError("unexpected remaining context output")
    return len(data)


def context_evidence(cases):
    expected = [{"kind": kind, "private": kind == "private", "container": "other" if kind == "container" else "default",
                 "extension_direct_observations": 1 if kind == "container" else 0, "native_tasks": 0,
                 "native_offers": 0, "firefox_bytes": len(BODY), "window_closed": True}
                for kind in ("container", "private")]
    if not isinstance(cases, list) or len(cases) != 2:
        raise RuntimeError("two completed context observations required")
    for case, wanted in zip(cases, expected, strict=True):
        if not isinstance(case, dict) or case != wanted or any(type(case[key]) is not type(wanted[key]) for key in wanted):
            raise RuntimeError("context evidence differs")
    return cases


def exercise_contexts(run, browser, inspector, fixture, downloads):
    armed = message(browser, inspector, {"action": "arm"}); untouched(armed)
    if armed.get("contexts") != []: raise RuntimeError("context observer not initially empty")
    cases = []; previous_observations = 0
    for count, kind in enumerate(("container", "private"), 1):
        run.stage = f"unsupported-{kind}"
        before = handles(browser)
        created = browser.chrome(CONTAINER if kind == "container" else PRIVATE)
        if kind == "container":
            if type(created) is not int or not 0 < created < 2**31: raise RuntimeError("container creation refused")
            container = created
        else:
            if created is not True: raise RuntimeError("private window creation refused")
            container = 0
        handle = new_window(browser, before)
        browser.navigate(fixture.url("page"))
        observed = browser.chrome(CONTEXT)
        valid_context(observed, kind, container)
        browser.click("#direct")
        size = archive_output(browser, downloads, run.plan.path, kind, fixture.url("direct"))
        snapshot = message(browser, inspector, {"action": "snapshot"})
        untouched(snapshot); context_observations(snapshot)
        with fixture.lock: counts = dict(fixture.requests)
        if (counts.get(("GET", "/direct")) != count or any(method != "GET" for method, _ in counts)
                or any(run.destination.iterdir())):
            raise RuntimeError("unsupported context caused native requests/output")
        if current_handle(browser) != handle: raise RuntimeError("context close target changed")
        browser.command("WebDriver:CloseWindow")
        if handles(browser) != before:
            raise RuntimeError("owned context window closure not observed")
        browser.command("WebDriver:SwitchToWindow", {"handle": inspector})
        run.browser_checks.append(f"armed_{kind}_correct_firefox_output_no_native_work")
        cases.append({"kind": kind, "private": observed["private"], "container": "other" if observed["container"] else "default",
                      "extension_direct_observations": len(snapshot["contexts"]) - previous_observations,
                      "native_tasks": snapshot["taskCount"], "native_offers": len(snapshot["records"]),
                      "firefox_bytes": size, "window_closed": True})
        previous_observations = len(snapshot["contexts"])
    return context_evidence(cases)
