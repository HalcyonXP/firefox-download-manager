"""Actual owned permission revocation and normal prompted denial/regrant; never direct grant."""
from .installed import wait

WATCH = """const key='__ownedCandidatePermission';
if(window[key])throw new Error('duplicate permission witness');
const expected=arguments[0],state={count:0,valid:false};
state.observer={observe(subject){const p=subject.wrappedJSObject;
if(p?.browser!==gBrowser.selectedBrowser||p.browser.currentURI.spec!==expected)return;
++state.count;const keys=p.permissions?Object.keys(p.permissions).sort().join(','):'';
state.valid=state.count===1&&p.name==='Download Manager capture candidate'&&
(keys==='origins,permissions'||(keys==='data_collection,origins,permissions'&&Array.isArray(p.permissions.data_collection)&&p.permissions.data_collection.length===0))&&
Array.isArray(p.permissions.permissions)&&p.permissions.permissions.length===0&&
Array.isArray(p.permissions.origins)&&p.permissions.origins.slice().sort().join(',')==='http://*/*,https://*/*';}};
Services.obs.addObserver(state.observer,'webextension-optional-permission-prompt');window[key]=state;return true;"""
SNAPSHOT = """const s=window.__ownedCandidatePermission;
const n=PopupNotifications.getNotification('addon-webext-permissions',gBrowser.selectedBrowser);
const node=document.getElementById('addon-webext-permissions-notification');
const primary=node?.querySelector('.popup-notification-primary-button');
const secondary=node?.querySelector('.popup-notification-secondary-button');
return {count:s?.count??0,valid:s?.valid===true,owner:n?.browser===gBrowser.selectedBrowser&&n.browser.currentURI.spec===arguments[0],
open:PopupNotifications.panel.state==='open'&&!!node&&!node.hidden,
primary:!!primary&&!primary.disabled,secondary:!!secondary&&!secondary.disabled};"""
RETIRE = """const s=window.__ownedCandidatePermission;if(!s)throw new Error('missing permission witness');
Services.obs.removeObserver(s.observer,'webextension-optional-permission-prompt');delete window.__ownedCandidatePermission;return true;"""


def prompt_ready(value):
    expected = {"count": 1, "valid": True, "owner": True, "open": True, "primary": True, "secondary": True}
    return isinstance(value, dict) and value == expected and all(type(value[k]) is type(v) for k, v in expected.items())


def revoke(browser):
    # Public extension API in the owned candidate page, not a privileged preference
    # or permission database edit. This is API-revocation evidence, not Firefox UI evidence.
    result = browser.script("""const done=arguments[arguments.length-1];
window.wrappedJSObject.browser.permissions.remove({origins:['http://*/*','https://*/*']}).then(done,()=>done(null));""", asynchronous=True)
    if result is not True: raise RuntimeError("owned website permission removal refused")
    browser.wait("const b=document.querySelector('#capture-access');return !b.hidden&&!b.disabled;")


def request(browser, allow):
    if type(allow) is not bool: raise RuntimeError("permission choice refused")
    if browser.chrome(WATCH, [browser.manager]) is not True: raise RuntimeError("permission witness unavailable")
    try:
        browser.click("#capture-access")  # Actual candidate user-gesture handler.
        wait(lambda: prompt_ready(browser.chrome(SNAPSHOT, [browser.manager])), 15)
        # Single dispatch; no callback invocation, delay override or uncertain replay.
        button = "primary" if allow else "secondary"
        browser.click(f"#addon-webext-permissions-notification .popup-notification-{button}-button", chrome=True)
        browser.wait("return !PopupNotifications.getNotification('addon-webext-permissions',gBrowser.selectedBrowser);", chrome=True)
        browser.wait("return !document.querySelector('#capture-access-status').textContent.includes('Checking website access');")
    finally:
        if browser.chrome(RETIRE) is not True: raise RuntimeError("permission witness retirement failed")
