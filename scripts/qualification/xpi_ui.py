"""Read-only Firefox install witnesses and ordinary visible-button interaction."""
import time
from .xpi_policy import approval, persistent_receipt

WATCH = """const key='__ownedManagerInstallWitness';
if(window[key])throw new Error('observer already exists');
const expected=arguments[0];const state={failed:false,signatureRequired:false,otherFailure:false,observer:null};
state.observer={observe(subject,topic){
const info=subject.wrappedJSObject;
if(!info||info.browser!==gBrowser.selectedBrowser||!Array.isArray(info.installs))return;
const installs=info.installs.filter(i=>i.sourceURI?.spec===expected);
if(installs.length!==1)return;
if(state.failed){state.signatureRequired=false;state.otherFailure=true;return;}
const {AddonManager}=ChromeUtils.importESModule('resource://gre/modules/AddonManager.sys.mjs');
state.failed=true;state.signatureRequired=typeof AddonManager.ERROR_SIGNEDSTATE_REQUIRED==='number'&&
installs[0].error===AddonManager.ERROR_SIGNEDSTATE_REQUIRED;
state.otherFailure=!state.signatureRequired;
}};Services.obs.addObserver(state.observer,'addon-install-failed');window[key]=state;return true;"""

SNAPSHOT = """const done=arguments[arguments.length-1],expected=arguments[0];
const {AddonManager}=ChromeUtils.importESModule('resource://gre/modules/AddonManager.sys.mjs');
AddonManager.getAllInstalls().then(installs=>{
const ids=['addon-install-blocked','addon-webext-permissions'];
const id=ids.find(id=>PopupNotifications.getNotification(id,gBrowser.selectedBrowser));
if(!id){done(null);return;}
const node=document.getElementById(id+'-notification');
const button=node?.querySelector('.popup-notification-primary-button');
const item=installs.length===1?installs[0]:null;
const permissions=item?.addon?.userPermissions;
const permissionKeys=permissions?Object.keys(permissions).sort():[];
const permissionSchema=permissionKeys.join(',')==='origins,permissions'||
(permissionKeys.join(',')==='data_collection,origins,permissions'&&Array.isArray(permissions.data_collection)&&permissions.data_collection.length===0);
done({id,open:PopupNotifications.panel.state==='open'&&!!node&&!node.hidden,
enabled:!!button&&!button.disabled,label:button?.getAttribute('label')??null,
source_matches:item?.sourceURI?.spec===expected,install_count:installs.length,
addon_id:item?.addon?.id??null,name:item?.addon?.name??null,
permission_schema:permissionSchema,permissions:permissions?.permissions??null,origins:permissions?.origins??null});
},()=>done(null));"""

RECEIPT = """const done=arguments[arguments.length-1];
const {AddonManager}=ChromeUtils.importESModule('resource://gre/modules/AddonManager.sys.mjs');
const {ExtensionParent}=ChromeUtils.importESModule('resource://gre/modules/ExtensionParent.sys.mjs');
AddonManager.getAddonByID(arguments[0]).then(addon=>{
const extension=ExtensionParent.GlobalManager.getExtension(arguments[0]);
done(addon?{id:addon.id,version:addon.version,active:addon.isActive,
temporary:addon.temporarilyInstalled,scope:addon.scope,
private_allowed:extension?.privateBrowsingAllowed??null}:null);},()=>done(null));"""

PROTECTIONS = """return {
signatures_required:Services.prefs.getBoolPref('xpinstall.signatures.required'),
install_enabled:Services.prefs.getBoolPref('xpinstall.enabled'),
};"""


def protections(browser):
    result = browser.chrome(PROTECTIONS)
    if (not isinstance(result, dict) or set(result) != {"signatures_required", "install_enabled"}
            or any(type(item) is not bool for item in result.values())):
        raise RuntimeError("owned install protection observation unavailable")
    return result


def receipt(browser, identity):
    return browser.chrome(RECEIPT, [identity["addon_id"]], True)


def observe_failure(browser):
    result = browser.chrome("""const s=window.__ownedManagerInstallWitness;
return s?{failed:s.failed,signatureRequired:s.signatureRequired,otherFailure:s.otherFailure}:null;""")
    if (not isinstance(result, dict) or set(result) != {"failed", "signatureRequired", "otherFailure"}
            or any(type(item) is not bool for item in result.values())):
        raise RuntimeError("install failure witness unavailable")
    return result


def approve_visible(browser, source, identity, seen):
    state = browser.chrome(SNAPSHOT, [source], True)
    if state is None: return False
    # Security delays are observed, never changed or skipped.
    if state.get("open") is not True or state.get("enabled") is not True: return False
    selector = approval(state, identity)
    if state["id"] in seen: return False  # An uncertain click is never replayed.
    seen.add(state["id"])
    browser.click(selector, chrome=True)
    return True


def observe_install(browser, page_url, source_url, identity):
    before = protections(browser)
    if receipt(browser, identity) is not None: raise RuntimeError("existing addon in new owned profile")
    if browser.chrome(WATCH, [source_url]) is not True: raise RuntimeError("install witness unavailable")
    browser.navigate(page_url); browser.click("#install")
    seen = set(); deadline = time.monotonic() + 30
    while True:
        failure = observe_failure(browser)
        if failure["failed"]:
            if not failure["signatureRequired"] or failure["otherFailure"]:
                raise RuntimeError("normal installation failed without signature classification")
            if receipt(browser, identity) is not None: raise RuntimeError("contradictory failed installation")
            result = "signature-requirement-observed"; break
        found = receipt(browser, identity)
        if found is not None and found.get("active") is True and found.get("private_allowed") is not None:
            persistent_receipt(found, identity)
            if "addon-webext-permissions" not in seen: raise RuntimeError("ordinary permission approval not observed")
            result = "installed"; break
        approve_visible(browser, source_url, identity, seen)
        if time.monotonic() >= deadline: raise RuntimeError("normal install observation deadline")
        time.sleep(.05)
    if protections(browser) != before: raise RuntimeError("install protections changed")
    return result, seen, before
