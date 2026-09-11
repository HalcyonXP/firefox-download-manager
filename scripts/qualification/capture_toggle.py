"""Owned UI preference observations. Only completed fixture output may be archived."""
from .capture import BODY, message
from .firefox import ELEMENT
from .browser_recovery import firefox_fallback, no_native_transfer


def preference(browser, inspector, enabled, *, change=True):
    browser.wait("return !!document.querySelector('#automatic-capture')&&!document.querySelector('#automatic-capture').disabled;")
    current = browser.script("return document.querySelector('#automatic-capture').checked;")
    if type(current) is not bool: raise RuntimeError("capture checkbox state unavailable")
    if current != enabled:
        if not change: raise RuntimeError("capture preference did not persist")
        reference = browser.script("return document.querySelector('#automatic-capture');")
        browser.command("WebDriver:ElementClick", {"id": reference[ELEMENT]})
    browser.wait("const c=document.querySelector('#automatic-capture');return !c.disabled&&!c.indeterminate&&c.checked===arguments[0];", [enabled])
    snapshot = message(browser, inspector, {"action": "snapshot"})
    state = snapshot.get("capturePreference")
    if not isinstance(state, dict) or any(type(item) is not bool for item in state.values()) or state != {"available": True, "ready": True, "enabled": enabled, "busy": False, "failed": False}:
        raise RuntimeError("verified capture preference unavailable")


def exercise_off(run, browser, inspector, fixture, downloads):
    browser.navigate(browser.manager)
    preference(browser, inspector, False)
    armed = message(browser, inspector, {"action": "arm"})
    if not isinstance(armed, dict) or armed.get("enabled") is not True:
        raise RuntimeError("diagnostic listener not armed for Off control")
    output = firefox_fallback(browser, fixture, downloads)
    snapshot = message(browser, inspector, {"action": "snapshot"})
    if type(snapshot.get("taskCount")) is not int or snapshot.get("taskCount") != 0 or snapshot.get("pending") != [] or snapshot.get("records") != []:
        raise RuntimeError("Off preference permitted a native capture")
    no_native_transfer(run.destination, fixture, 1)
    data = output.read_bytes()
    if data != BODY: raise RuntimeError("Off fixture bytes differ")
    # Preserve this independently verified output and remove only its exact completed
    # owned-profile history entry, so later fallback checks cannot accept stale output.
    removed = browser.chrome("""const done=arguments[arguments.length-1];
const {Downloads}=ChromeUtils.importESModule('resource://gre/modules/Downloads.sys.mjs');
Downloads.getList(Downloads.PUBLIC).then(async list=>{const entries=await list.getAll();
if(entries.length!==1)return false;const d=entries[0];
if(d.source.url!==arguments[0]||d.target.path!==arguments[1]||!d.succeeded||!d.stopped||d.error||d.canceled)return false;
await list.remove(d);return (await list.getAll()).length===0;}).then(done,()=>done(false));""", [fixture.url("direct"), str(output)], True)
    if removed is not True: raise RuntimeError("owned fixture history removal refused")
    with (run.plan.path / "firefox-off.bin").open("xb") as archive: archive.write(data)
    output.unlink()
    if any(downloads.iterdir()): raise RuntimeError("unexpected remaining fixture output")
    browser.navigate(browser.manager)
    preference(browser, inspector, True)
    run.browser_checks.append("armed_listener_off_correct_firefox_output_then_verified_on")
