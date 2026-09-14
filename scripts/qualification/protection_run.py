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
LOAD = Path(__file__).with_name('protection_loader.js').read_text(encoding='utf-8')
LOAD_TERMS = {'invalid-extension','experiment-apis','privilege-required','manifest-version','csp','schema',
              'unexpected-property','async-returns','enum','signature','incognito'}


def load_observation(value):
    if (not isinstance(value,dict) or set(value)!={'version','state','phase','terms','complete'}
            or type(value['version']) is not int or value['version']!=1
            or value['state'] not in ('loaded','refused')
            or value['phase'] not in ('bootstrap','file-init','install','identity')
            or type(value['complete']) is not bool or not isinstance(value['terms'],list)
            or len(value['terms'])>len(LOAD_TERMS)
            or any(not isinstance(term,str) or term not in LOAD_TERMS for term in value['terms'])
            or sorted(set(value['terms']))!=value['terms']
            or (value['state']=='loaded' and (value['phase']!='identity' or value['terms'] or not value['complete']))):
        raise RuntimeError('temporary load observation refused')
    return value


def cleanup_observation(browser):
    result={'browser_created':browser is not None,'browser_started':False,'browser_joined':False,'browser_exit':None}
    if browser is not None and browser.process is not None:
        result['browser_started']=True
        if browser.closed:
            code=browser.process.wait(timeout=0)
            if type(code) is not int: raise RuntimeError('retained browser wait receipt refused')
            result.update(browser_joined=True,browser_exit=code)
    return result

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
    if (not isinstance(value,dict) or set(value)!={'version','qualification','scope','stage','result','attempted','callbacks','metadata_reads'}
            or type(value['version']) is not int or value['version'] != 2 or value['qualification'] is not False
            or value['scope']!='fixed-empty-loopback-context' or value['stage']!='settled'
            or value['result']!='not-blocked' or value['attempted'] is not True
            or type(value['callbacks']) is not int or value['callbacks']!=1
            or type(value['metadata_reads']) is not int or value['metadata_reads']!=15):
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
    def __init__(self, directory, executable, report, *, fileless_experiment=False):
        if type(fileless_experiment) is not bool: raise RuntimeError('explicit fileless experiment mode required')
        self.fileless_experiment = fileless_experiment
        self.directory, self.executable, self.report = directory, executable, report
        self.browser = None
        self.plan = None
        self.stage = "preflight"
        self.before = self.after = self.load_result = None

    def open(self, profile, environment):
        preflight()
        self.browser = Firefox(self.executable, profile, environment, fileless_experiment=self.fileless_experiment)
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
            self.before = before = protections(browser)
            self.stage = "temporary-load"
            self.load_result = load_observation(browser.chrome(LOAD,[str(xpi),identity['addon_id']],True))
            if self.load_result['state']!='loaded':
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
            self.after = protections(browser)
            if self.after!=before: raise RuntimeError('protection settings changed')
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
                                      'owned_automation_policy':browser.automation_policy,
                                      'profile_mode':'fileless-experiment' if self.fileless_experiment else 'default',
                                      'temporary_load':self.load_result,
                                      'wrong_page_refused':True,'repeated_start_same_receipt':True,'closed_context_refused':True,
                                      'firefox_exe_sha256':browser_hash,'successful_browser_exits':1,'joined':True,
                                      'registration_unchanged_absent':True,'native_publication_qualified':False})
        except BaseException as error:
            frames = []
            try:
                trace = error.__traceback__
                while trace is not None and len(frames)<12:
                    frames.append({'function':trace.tb_frame.f_code.co_name,'line':trace.tb_lineno});trace=trace.tb_next
            except BaseException: pass  # Diagnostic collection cannot skip retirement.
            if self.before is not None and self.browser is not None and not self.browser.closed:
                try: self.after = protections(self.browser)
                except BaseException: self.after = None
            if not self.retire():
                print('Protection probe cleanup incomplete; retaining exact owned Firefox owner.',flush=True)
                while not self.retire(): time.sleep(1)
            # Observation failure cannot skip retirement, and absence alone is not a join.
            try:
                if self.plan.created:
                    with (self.plan.path/'failure.private.json').open('x',encoding='utf-8') as output:
                        json.dump({'version':2,'stage':self.stage,'frames':frames,
                                   'temporary_load':self.load_result,'cleanup':cleanup_observation(self.browser),
                                   'owned_automation_policy':getattr(self.browser,'automation_policy',None),
                                   'profile_mode':'fileless-experiment' if self.fileless_experiment else 'default',
                                   'initial_owned_protections':self.before,'final_owned_protections':self.after,
                                   'protections_unchanged':None if self.before is None or self.after is None else self.before==self.after},output)
            except BaseException: pass  # Failed recording must not claim success or lose an owner.
            raise RuntimeError('protection probe refused; owned domain preserved; no acceptance claimed') from None


def run(directory, executable, report, *, fileless_experiment=False):
    if not __debug__ or os.name!='nt' or ctypes.sizeof(ctypes.c_void_p)!=8:
        raise RuntimeError('protection probe requires assertions and 64-bit Windows')
    return ProtectionRun(directory.absolute(),executable.absolute(),new_report(report),fileless_experiment=fileless_experiment).execute()
