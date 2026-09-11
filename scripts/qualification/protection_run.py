"""Owned, fileless privileged service observation; no native/registration operations."""
import ctypes
import json
import os
from pathlib import Path
import subprocess
import re
import time

from .firefox import Firefox
from .installed import ROOT, ordinary
from .native import file_sha256
from .protection_input import inspect, revision
from .setup_owner import DomainPlan
from .support import ARTIFACTS, new_report, write_report
from .xpi_persistence import preflight

PROTECTIONS = """const names=['xpinstall.signatures.required','extensions.experiments.enabled',
'browser.safebrowsing.malware.enabled','browser.safebrowsing.phishing.enabled',
'browser.safebrowsing.downloads.enabled','browser.safebrowsing.downloads.remote.enabled'];
return Object.fromEntries(names.map(name=>{const kind=Services.prefs.getPrefType(name);
if(kind!==0&&kind!==128)throw new Error('protection preference type refused');
return [name,kind===0?null:Services.prefs.getBoolPref(name)];}));"""
LOAD = """const done=arguments[arguments.length-1];
const {AddonManager}=ChromeUtils.importESModule('resource://gre/modules/AddonManager.sys.mjs');
const file=Components.classes['@mozilla.org/file/local;1'].createInstance(Components.interfaces.nsIFile);
file.initWithPath(arguments[0]);AddonManager.installTemporaryAddon(file).then(a=>done(a.id===arguments[1]),()=>done(false));"""
INFO = """const {ExtensionParent}=ChromeUtils.importESModule('resource://gre/modules/ExtensionParent.sys.mjs');
const {AppConstants}=ChromeUtils.importESModule('resource://gre/modules/AppConstants.sys.mjs');
const ext=ExtensionParent.GlobalManager.getExtension(arguments[0]);
return ext ? {base:ext.baseURI.spec,privateAllowed:ext.privateBrowsingAllowed,channel:AppConstants.MOZ_UPDATE_CHANNEL} : null;"""
OBSERVE = """const text=document.querySelector('#receipt')?.textContent;
if(!text||text.length>1024)return null;
try{const value=JSON.parse(text);return value.stage==='settled'?value:null;}catch{return null;}"""
REPEAT = """const done=arguments[arguments.length-1];
window.wrappedJSObject.browser.managerProtection.start().then(done,()=>done(null));"""


def valid_receipt(value):
    if (not isinstance(value,dict) or set(value)!={'version','qualification','scope','stage','result','attempted','callbacks'}
            or type(value['version']) is not int or value['version'] != 1 or value['qualification'] is not False
            or value['scope']!='fixed-empty-loopback-text' or value['stage']!='settled'
            or value['result']!='not-blocked' or value['attempted'] is not True
            or type(value['callbacks']) is not int or value['callbacks']!=1):
        raise RuntimeError('protection service observation unavailable')
    return value


def require_joined(browser):
    if browser is None or not browser.closed or browser.process is None or browser.process.wait(timeout=0)!=0:
        raise RuntimeError('successful retained Firefox exit required')


def protections(browser):
    value = browser.chrome(PROTECTIONS)
    expected = {'xpinstall.signatures.required','extensions.experiments.enabled','browser.safebrowsing.malware.enabled',
                'browser.safebrowsing.phishing.enabled','browser.safebrowsing.downloads.enabled','browser.safebrowsing.downloads.remote.enabled'}
    if not isinstance(value,dict) or set(value)!=expected or any(v is not None and type(v) is not bool for v in value.values()):
        raise RuntimeError('protection readback refused')
    return value


class ProtectionRun:
    def __init__(self, directory, executable, report):
        self.directory, self.executable, self.report = directory, executable, report
        self.browser = None
        self.plan = None
        self.stage = "preflight"

    def open(self, profile, environment):
        preflight()
        self.browser = Firefox(self.executable, profile, environment)
        self.browser.start()  # Owner retained even if startup fails.
        return self.browser

    def retire(self):
        if self.browser is None: return True
        try:
            self.browser.close()
            return self.browser.closed
        except BaseException: return False

    def execute(self):
        preflight()
        xpi, identity = inspect(self.directory)
        ordinary(self.executable)
        commit, browser_hash = revision(), file_sha256(self.executable)
        if identity['source_commit']!=commit or subprocess.check_output(['git','status','--porcelain'],cwd=ROOT,timeout=15):
            raise RuntimeError('clean exact probe source required')
        self.plan = DomainPlan.record(ARTIFACTS, ROOT/'.git')
        try:
            self.plan.create()
            for folder in ('local','roaming','home'): (self.plan.path/folder).mkdir()
            environment = {**os.environ, 'LOCALAPPDATA':str(self.plan.path/'local'), 'APPDATA':str(self.plan.path/'roaming'),
                           'USERPROFILE':str(self.plan.path/'home'), 'HOME':str(self.plan.path/'home'),
                           'PATH':str(Path(os.environ['WINDIR'])/'System32'), 'MOZ_CRASHREPORTER_DISABLE':'1'}
            self.stage = 'browser-start'
            browser = self.open(self.plan.path/'Firefox',environment)
            before = protections(browser)
            self.stage = "temporary-load"
            if browser.chrome(LOAD,[str(xpi),identity['addon_id']],True) is not True:
                raise RuntimeError('owned temporary protection probe unavailable')
            info = browser.chrome(INFO,[identity['addon_id']])
            if (not isinstance(info,dict) or set(info)!={'base','privateAllowed','channel'} or info['privateAllowed'] is not False
                    or info['channel']!='aurora' or not isinstance(info['base'],str)
                    or not re.fullmatch(r'moz-extension://[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}/', info['base'])):
                raise RuntimeError('owned protection extension context refused')
            page = info['base']+'probe.html'
            self.stage = 'wrong-page-refusal'
            browser.navigate(page+'?refused=1')
            browser.wait("return document.querySelector('#receipt')?.textContent==='unavailable';",timeout=20)
            self.stage = "fixed-query"
            browser.navigate(page)
            browser.wait('return location.href===arguments[0];',[page])
            observed = valid_receipt(browser.wait(OBSERVE,timeout=20))
            if valid_receipt(browser.script(REPEAT,asynchronous=True))!=observed:
                raise RuntimeError('protection observation changed on repeat')
            self.stage = 'context-retirement'
            browser.navigate('about:blank')
            browser.navigate(page)
            browser.wait("return document.querySelector('#receipt')?.textContent==='unavailable';",timeout=20)
            if protections(browser)!=before: raise RuntimeError('protection settings changed')
            self.stage = "joined-shutdown"
            browser.close()
            require_joined(browser)
            preflight()
            if (inspect(self.directory)[1]!=identity or revision()!=commit or file_sha256(self.executable)!=browser_hash
                    or subprocess.check_output(['git','status','--porcelain'],cwd=ROOT,timeout=15)):
                raise RuntimeError('protection probe source changed')
            write_report(self.report, {'format':'firefox-protection-service-probe','version':1,'qualification':False,'m5_install_ready':False,
                                      'scope':'fileless fixed loopback service query; no native publication authority','identity':identity,
                                      'harness_revision':commit,'harness_worktree_dirty':False,'temporary_loading_used':True,
                                      'initial_owned_protections':before,'protections_unchanged':True,'receipt':observed,
                                      'wrong_page_refused':True,'repeated_start_same_receipt':True,'closed_context_refused':True,
                                      'firefox_exe_sha256':browser_hash,'successful_browser_exits':1,'joined':True,
                                      'registration_unchanged_absent':True,'native_publication_qualified':False})
        except BaseException as error:
            try:
                frames = []; trace = error.__traceback__
                while trace is not None and len(frames)<12:
                    frames.append({'function':trace.tb_frame.f_code.co_name,'line':trace.tb_lineno});trace=trace.tb_next
                if self.plan.created:
                    with (self.plan.path/'failure.private.json').open('x',encoding='utf-8') as output:
                        json.dump({'stage':self.stage,'frames':frames},output)
            except BaseException: pass  # Recording must never skip retained-owner retirement.
            if not self.retire():
                print('Protection probe cleanup incomplete; retaining exact owned Firefox owner.',flush=True)
                while not self.retire(): time.sleep(1)
            raise RuntimeError('protection probe refused; owned domain preserved; no acceptance claimed') from None


def run(directory, executable, report):
    if not __debug__ or os.name!='nt' or ctypes.sizeof(ctypes.c_void_p)!=8:
        raise RuntimeError('protection probe requires assertions and 64-bit Windows')
    return ProtectionRun(directory.absolute(),executable.absolute(),new_report(report)).execute()
