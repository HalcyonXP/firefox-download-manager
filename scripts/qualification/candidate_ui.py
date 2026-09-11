"""Candidate UI/readback helpers. No diagnostic messages, task seeding or installation bypass."""
import uuid
from .capture import BODY
from .installed import ordinary, wait
from .candidate_fixture import filename
from .browser_installed import correct_output

STATE = """const done=arguments[arguments.length-1],api=window.wrappedJSObject.browser;
Promise.all([api.permissions.contains({permissions:['webRequest','webRequestBlocking'],origins:['http://*/*','https://*/*']}),
api.storage.local.get(['automatic-capture-v1','pending-handoffs-v1'])]).then(([access,stored])=>{
const c=document.querySelector('#automatic-capture');const rows=[...document.querySelectorAll('#tasks .task')];
if(rows.length>24){done(null);return;}
done({connected:document.querySelector('#connection')?.textContent==='Helper connected',
access,access_ui:document.querySelector('#capture-access-status').textContent.includes(access?'Website access verified.':'Website access is missing.'),enabled:c.checked,settled:!c.disabled&&!c.indeterminate,
candidate:!document.querySelector('#capture-access-status').hidden,
saved:stored['automatic-capture-v1']??null,journal:stored['pending-handoffs-v1']??null,
tasks:rows.map(r=>({id:r.getAttribute('aria-labelledby'),name:r.querySelector('h3').textContent,state:r.dataset.state}))});},()=>done(null));"""


def state(browser, expected, enabled=True, access=True, saved=None):
    observed = browser.script(STATE, asynchronous=True)
    if (not isinstance(observed, dict) or set(observed) != {"connected", "access", "access_ui", "enabled", "settled", "candidate", "saved", "journal", "tasks"}
            or observed["connected"] is not True or observed["settled"] is not True or observed["candidate"] is not True
            or observed["access_ui"] is not True or observed["enabled"] is not enabled or observed["access"] is not access):
        raise RuntimeError("candidate UI/authority unavailable")
    if observed["saved"] != saved or (saved is not None and (type(observed["saved"].get("enabled")) is not bool or type(observed["saved"].get("version")) is not int)):
        raise RuntimeError("capture preference differs")
    journal = observed["journal"]
    if journal is not None and (journal != {"version": 1, "pending": []} or type(journal.get("version")) is not int):
        raise RuntimeError("candidate handoff journal is not settled")
    tasks = observed["tasks"]
    if not isinstance(tasks, list) or len(tasks) != len(expected): raise RuntimeError("candidate task count differs")
    found = {}
    for task in tasks:
        if not isinstance(task, dict) or set(task) != {"id", "name", "state"} or task["state"] != "completed":
            raise RuntimeError("candidate task is not completed")
        identifier = task["id"]
        if not isinstance(identifier, str) or not identifier.startswith("task-"):
            raise RuntimeError("candidate task identity unavailable")
        raw = identifier.removeprefix("task-")
        if str(uuid.UUID(raw, version=4)) != raw or task["name"] in found or raw in found.values(): raise RuntimeError("candidate task identity differs")
        found[task["name"]] = raw
    if set(found) != set(expected) or any(expected[k] is not None and expected[k] != found[k] for k in expected):
        raise RuntimeError("candidate task identity changed")
    return found


def preference(browser, enabled):
    browser.wait("const c=document.querySelector('#automatic-capture');return !c.disabled&&!c.indeterminate;")
    current = browser.script("return document.querySelector('#automatic-capture').checked;")
    if type(current) is not bool: raise RuntimeError("candidate preference unavailable")
    if current != enabled: browser.click("#automatic-capture")
    browser.wait("const c=document.querySelector('#automatic-capture');return !c.disabled&&!c.indeterminate&&c.checked===arguments[0];", [enabled])
    return {"version": 1, "enabled": enabled}


DOWNLOAD = """const done=arguments[arguments.length-1];
const {Downloads}=ChromeUtils.importESModule('resource://gre/modules/Downloads.sys.mjs');
Downloads.getList(arguments[2]?Downloads.PRIVATE:Downloads.PUBLIC).then(async list=>{
const entries=await list.getAll();if(entries.length!==1)return false;const d=entries[0];
if(d.source.url!==arguments[0]||d.target.path!==arguments[1]||!d.succeeded||!d.stopped||d.error||d.canceled||d.currentBytes!==arguments[3])return false;
if(arguments[4]){await list.remove(d);return (await list.getAll()).length===0;}return true;
}).then(done,()=>done(false));"""


def archive(browser, downloads, root, case, source, private=False):
    path = downloads / filename(case)
    args = [source, str(path), private, len(BODY), False]
    wait(lambda: browser.chrome(DOWNLOAD, args, True) is True, 15)
    ordinary(path)
    if set(downloads.iterdir()) != {path} or not correct_output(path):
        raise RuntimeError("candidate Firefox output differs")
    if browser.chrome(DOWNLOAD, [*args[:4], True], True) is not True:
        raise RuntimeError("candidate completed-history removal refused")
    with (root / ("firefox-" + filename(case))).open("xb") as target: target.write(BODY)
    if not correct_output(path): raise RuntimeError("candidate output changed before retirement")
    path.unlink()
    if any(downloads.iterdir()): raise RuntimeError("candidate Firefox output remains")


def empty_downloads(browser, downloads):
    if any(downloads.iterdir()) or browser.chrome("""const done=arguments[arguments.length-1];
const {Downloads}=ChromeUtils.importESModule('resource://gre/modules/Downloads.sys.mjs');
Downloads.getList(Downloads.PUBLIC).then(l=>l.getAll()).then(items=>done(items.length===0),()=>done(false));""", asynchronous=True) is not True:
        raise RuntimeError("competing or unresolved Firefox download")
