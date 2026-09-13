"""Opt-in installed parent transport/manual UI slice; no persistent or capture qualification.

Retain the run before execute. Original installation/all-view and BrowserPeer
preflights remain. No signing override, normal-profile access or PID adoption.
"""
import copy
import json
from functools import wraps
import os
from pathlib import Path
import struct
import time
import uuid
from .browser_peer import BrowserPeer
from .firefox import Firefox, AutomationError
from .browser_cases import value
from .firefox_policy import validate as validate_policy
from .fixture import Fixture, SMALL_SIZE, expected_sha256
from .failure_location import failure_location
from .installed import InstalledRun, FAULTS, REMOVED, absent, closed_apps, ordinary, wait
from .setup_owner import SetupOwner
from .native import file_sha256
from .parent_candidate import candidate_input
from .parent_cleanup_observation import CleanupObserverClient as ObserverClient
from .process_lease import ProcessLease
from .support import new_report

CONTROL=Path(__file__).with_name('parent_transport_control.js').read_text(encoding='utf-8')
TAB_STATE=Path(__file__).with_name('parent_tab_state.js').read_text(encoding='utf-8')
TAB_FIELDS={'window_modal','navigation_collapsed','selection_consistent','selected_control','selected_blank'}
TAB_FAILURE_STAGES={'manager-tab-creation','manager-tab-response-shape','manager-tab-response-type',
                    'manager-tab-response-handle','manager-tab-response-distinct','manager-tab-selection'}
ERROR='owned installed parent transport refused; retain owner and domain'


def control(browser, operation, expected):
    value=browser.chrome(CONTROL,[operation],True)
    if type(value) is not dict or set(value)!=set(expected) or any(type(value[k]) is not type(v) or value[k]!=v for k,v in expected.items()):
        raise RuntimeError(ERROR)



def retained_failure(method):
    @wraps(method)
    def invoke(self,*args,**kwargs):
        try: return method(self,*args,**kwargs)
        except BaseException as error:
            self.failed=True
            try:
                self.note_failure(error)
                # Do not start optional diagnostic commands after interruption.
                self.observe_tab_failure(interrupted=not isinstance(error,Exception))
            except BaseException as observation_error:
                if isinstance(error,Exception) and not isinstance(observation_error,Exception):
                    raise observation_error
                raise error
            raise
    return invoke


def independent(actions):
    failure=None
    for action in actions:
        try: action()
        except BaseException as caught:
            if failure is None or (isinstance(failure,Exception) and not isinstance(caught,Exception)): failure=caught
    if failure is not None: raise failure


class ParentBrowser(Firefox):
    def __init__(self, executable, profile, environment, peer):
        super().__init__(executable,profile,environment,owned_peer=peer,parent_transport_experiment=True)
        self.original=self.parent_lease=self.observer=None
        self.load_attempted=self.disable_attempted=self.disabled_observed=self.browser_close_attempted=False
        self.failed=False
        self.stage='new'
        self.first_failure=None
        self.command_failure=None
        self.control_handle=self.manager_handle=None
        self.tab_attempted=self.control_switch_attempted=False
        self.tab_failure_attempted=False
        self.tab_failure=None

    def observe_tab_failure(self, *, interrupted=False):
        # A single post-failure sample, never authority to retry or select a tab.
        if (self.tab_failure_attempted or self.first_failure is None
                or self.first_failure['stage'] not in TAB_FAILURE_STAGES): return
        self.tab_failure_attempted=True  # Reserve before any uncertain command.
        if interrupted:
            self.tab_failure={'state':'skipped-interruption'}
            return
        self.tab_failure={'state':'unavailable'}
        self.stage='manager-tab-failure-observation'
        result=self.read_tab_state()
        if (type(result) is not dict or set(result)!=TAB_FIELDS|{'version'}
                or type(result['version']) is not int or result['version']!=1
                or any(result[key] is not None and type(result[key]) is not bool for key in TAB_FIELDS)):
            raise RuntimeError(ERROR)
        self.tab_failure={'state':'observed',**result}

    def read_tab_state(self):
        self.command('Marionette:SetContext',{'value':'chrome'})
        result=[]
        # Preserve a script interruption even if context restoration also fails.
        independent((lambda:result.append(self.script(TAB_STATE,[self.control_handle])),
                     lambda:self.command('Marionette:SetContext',{'value':'content'})))
        return result[0]

    def note_failure(self, error):
        if self.first_failure is None:
            self.first_failure={'stage':self.stage,'kind':error.kind if isinstance(error,AutomationError) else 'other'}
            self.first_failure.update(failure_location(error))

    def command(self, name, arguments=None):
        try: return super().command(name, arguments)
        except AutomationError as error:
            if self.command_failure is None:
                allowed={'Marionette:SetContext','WebDriver:ExecuteScript','WebDriver:ExecuteAsyncScript',
                         'WebDriver:SwitchToWindow','WebDriver:GetWindowHandle','WebDriver:GetCurrentURL','WebDriver:NewWindow','Marionette:Quit'}
                self.command_failure={'stage':self.stage,'command':name if name in allowed else 'other','kind':error.kind}
            raise

    @retained_failure
    def start(self):
        if self.failed or self.launch_attempted or self.original is not None or self.parent_lease is not None: raise RuntimeError(ERROR)
        self.stage='browser-start'
        try: super().start()
        finally: self.original=self.process
        self.stage='parent-lease'
        reported=self.chrome('return Services.appinfo.processID;')
        self.parent_lease=ProcessLease(self.original,reported,self.executable)
        self.parent_lease.acquire()  # Retained before acquisition and add-on code.
        self.stage='control-tab-retention'
        self.control_handle=value(self.command('WebDriver:GetWindowHandle'))
        if not self.valid_handle(self.control_handle) or value(self.command('WebDriver:GetCurrentURL'))!='about:blank': raise RuntimeError(ERROR)
        self.stage='observer-install'
        self.observer=ObserverClient(self,str(uuid.uuid4()))
        self.observer.install()
        if self.observer.snapshot()!=0: raise RuntimeError(ERROR)
        control(self,'absent',{'state':'absent'})
        return self

    @retained_failure
    def load(self, xpi):
        if self.failed or self.load_attempted or self.parent_lease is None or self.parent_lease.acquired is not True: raise RuntimeError(ERROR)
        if self.tab_attempted or not self.valid_handle(self.control_handle): raise RuntimeError(ERROR)
        self.stage='manager-tab-creation'
        self.tab_attempted=True
        tab=value(self.command('WebDriver:NewWindow',{'type':'tab'}))
        # Keep each refusal identifiable without recording returned handles or fields.
        self.stage='manager-tab-response-shape'
        if type(tab) is not dict or set(tab)!={'handle','type'}: raise RuntimeError(ERROR)
        self.stage='manager-tab-response-type'
        if tab['type']!='tab': raise RuntimeError(ERROR)
        self.stage='manager-tab-response-handle'
        if not self.valid_handle(tab['handle']): raise RuntimeError(ERROR)
        self.stage='manager-tab-response-distinct'
        if tab['handle']==self.control_handle: raise RuntimeError(ERROR)
        self.manager_handle=tab['handle']
        self.stage='manager-tab-selection'
        self.command('WebDriver:SwitchToWindow',{'handle':self.manager_handle})
        self.stage='candidate-load'
        self.load_attempted=True
        super().load(xpi)
        expected={'state':'active','id':'download-manager@halcyonxp.local','version':'0.3.0',
                  'temporary':True,'privateAllowed':False,'persistentBackground':False}
        control(self,'info',expected)

    @staticmethod
    def valid_handle(handle):
        return type(handle) is str and handle.isascii() and 0<len(handle)<=128 and all(32<ord(c)<127 for c in handle)

    def select_control(self):
        # Extension shutdown may remove its UI tab. Do not run disable from that
        # tab or change close-last-tab preferences. No discovered-window adoption.
        self.stage='control-tab-selection'
        if not self.valid_handle(self.control_handle): raise RuntimeError(ERROR)
        if not self.control_switch_attempted:
            self.control_switch_attempted=True
            self.command('WebDriver:SwitchToWindow',{'handle':self.control_handle})
        # An uncertain selection permits only current-handle/URL observation.
        if (value(self.command('WebDriver:GetWindowHandle'))!=self.control_handle
                or value(self.command('WebDriver:GetCurrentURL'))!='about:blank'): raise RuntimeError(ERROR)

    def diagnostic(self):
        # Bounded facts only; not process authority, policy or a retirement receipt.
        observer=None
        if self.observer is not None:
            e=self.observer.evidence;r=self.observer.retirement
            for record in (e,r):
                if len(record.records)>1 or any(type(raw) is not str or not raw.isascii() or len(raw)>4096 for raw in record.records): raise RuntimeError(ERROR)
            observer={'failed':e.failed,'records':list(e.records),'removal_returned':self.observer.removal_returned,
                      'cleanup':{'failed':r.failed,'records':list(r.records),'closed':r.closed,'removed':r.removed}}
        lease=None if self.parent_lease is None else {'acquired':self.parent_lease.acquired,'released':self.parent_lease.released,
                                                     'failed':self.parent_lease.failed,'retained_handles':len(self.parent_lease.handles)}
        launcher_exit=None if self.original is None else self.original.poll()
        if launcher_exit is not None and type(launcher_exit) is not int: raise RuntimeError(ERROR)
        return {'version':4,'qualification':False,'stage':self.stage,'first_failure':copy.deepcopy(self.first_failure),'command_failure':copy.deepcopy(self.command_failure),
                'tab_failure':copy.deepcopy(self.tab_failure),
                'observer':observer,'parent_lease':lease,'launcher_exit_observed':launcher_exit,
                'failed':self.failed,'load_attempted':self.load_attempted,'disable_attempted':self.disable_attempted,
                'disabled_observed':self.disabled_observed,'browser_close_attempted':self.browser_close_attempted,
                'control_tab_retained':self.control_handle is not None,'manager_tab_retained':self.manager_handle is not None,
                'control_switch_attempted':self.control_switch_attempted,
                'original_process_retained':self.original is not None and self.process is self.original}

    @retained_failure
    def close(self):
        if self.launch_attempted and self.original is None: raise RuntimeError(ERROR)
        if self.original is not None and self.process is not self.original: raise RuntimeError(ERROR)
        if self.load_attempted:
            if not self.disabled_observed: self.select_control()
            if not self.disable_attempted:
                self.stage='disable-dispatch'
                self.disable_attempted=True
                control(self,'disable',{'state':'disabled'})
                self.disabled_observed=True
            if not self.disabled_observed:
                # Unknown command delivery permits fresh metadata observation,
                # never another disable dispatch or a presumed application state.
                self.stage='disabled-observation'
                control(self,'disabled',{'state':'disabled'})
                self.disabled_observed=True
            if not self.sdk_retired():
                self.stage='sdk-retirement-readback' if self.failed else 'sdk-retirement-observation'
                snapshot=self.observer.cleanup_snapshot if self.failed else self.observer.snapshot
                wait(lambda: snapshot()==1 and self.sdk_retired(),20)
        self.stage=('observer-removal-readback' if self.failed and self.observer is not None
                    and self.observer.removal_attempted else 'observer-removal')
        if self.observer is not None:
            remove=self.observer.cleanup_remove if self.failed else self.observer.remove
            if not remove(): raise RuntimeError(ERROR)
        error=None
        if not self.browser_close_attempted:
            self.stage='browser-close'
            self.browser_close_attempted=True
            try: super().close()
            except BaseException as failure:
                error=failure
                self.note_failure(failure)
        elif not self.closed and self.original is not None:
            # Only continued exact wait/closed-app observations; never repeat Quit.
            self.stage='browser-wait'
            self.original.wait(timeout=5)
            self._require_apps_closed()
            self.closed=True
        try:
            if self.parent_lease is not None and not self.parent_lease.cleanup_complete():
                if self.parent_lease.handles and not self.parent_lease.released:
                    self.stage='parent-lease-observation'
                    wait(self.parent_lease.observe,5)
                    self.stage='parent-lease-release'
                    self.parent_lease.release()
                if not self.parent_lease.cleanup_complete(): raise RuntimeError(ERROR)
        except BaseException as failure:
            if error is not None and not isinstance(error,Exception): raise error
            raise failure
        if error is not None: raise error

    def sdk_retired(self):
        if self.observer is None: return False
        evidence=self.observer.retirement if self.failed else self.observer.evidence
        return evidence.resource_retired()

    def observer_removed(self):
        return self.observer is None or (self.observer.cleanup_complete() is True if self.failed else self.observer.removal_returned is True)

    def cleanup_complete(self):
        return (self.launch_attempted is False or (self.launch_attempted is True and self.original is not None
                and self.process is self.original and self.closed is True and type(self.original.poll()) is int)) and (
                self.parent_lease is None or self.parent_lease.cleanup_complete() is True) and (
                self.observer_removed()) and (
                self.load_attempted is False or (self.load_attempted is True and self.disabled_observed is True
                and self.sdk_retired() is True))

    def evidence(self):
        if self.failed is not False or self.cleanup_complete() is not True or self.original is None: raise RuntimeError(ERROR)
        code=self.original.wait(timeout=0)
        if type(code) is not int or code!=0: raise RuntimeError(ERROR)
        return {'parent':self.parent_lease.receipt(),'sdk':self.observer.evidence.require_removed(),
                'launcher_waited':True,'launcher_exit':0,'profile_mode':'parent-transport-experiment',
                'firefox_automation_policy':copy.deepcopy(validate_policy(self.automation_policy,parent_transport_experiment=True))}


class ParentInstalledRun(InstalledRun):
    @property
    def process(self): return getattr(self,'_setup_process',None)

    @process.setter
    def process(self, value):
        if self.process is not None and value is not self.process: raise RuntimeError(ERROR)
        self._setup_process=value

    def __init__(self, package, report, candidate, candidate_sha256, executable, browser_sha256,
                 *, parent_transport_experiment=False, fault=None):
        if hasattr(self,'_setup_process'): raise RuntimeError(ERROR)
        if parent_transport_experiment is not True or not __debug__ or os.name!='nt' or struct.calcsize('P')!=8: raise RuntimeError(ERROR)
        if fault is not None and (type(fault) is not tuple or len(fault)!=2 or type(fault[0]) is not str or fault[0] not in FAULTS
                or type(fault[1]) is not int or fault[1]<=0): raise RuntimeError(ERROR)
        super().__init__(package,report,None)
        self.fault=fault
        self.occurrences={}
        self.candidate,self.candidate_sha256=candidate,candidate_sha256
        self.executable,self.browser_sha256=executable,browser_sha256
        self.sdk_checks=[]
        self.input_identity=None
        self.run_attempted=self.final_cleanup_attempted=self.failure_cleanup_attempted=False
        self.cleanup_failure=None

    def execute(self):
        if self.run_attempted or self.final_cleanup_attempted: raise RuntimeError(ERROR)
        self.run_attempted=True
        super().execute()

    def close_resources(self):
        def attempt(label, action):
            try: action()
            except BaseException:
                if label not in self.cleanup_errors: self.cleanup_errors.append(label)
                raise
        def browser_close(browser):
            if browser.cleanup_complete() is not True: browser.close()
            if browser.cleanup_complete() is not True: raise RuntimeError(ERROR)
            if browser.original is not None: browser.original.wait(timeout=0)
        def fixture_close(fixture):
            fixture.close()
            if fixture.closed is not True: raise RuntimeError(ERROR)
        actions=[lambda b=b:attempt('browser',lambda:browser_close(b)) for b in self.browsers]
        actions.extend(lambda f=f:attempt('fixture',lambda:fixture_close(f)) for f in self.fixtures)
        if self.hosts:
            def refuse(): raise RuntimeError(ERROR)
            actions.append(lambda:attempt('unexpected-native-owner',refuse))
        independent(actions) # Unlike generic cleanup, interruption remains interruption.

    def observe_uninstalled(self):
        # A continued observation of the ONE original Uninstall, not dispatch.
        if not self.uninstall_requested or self.binding is None or self.owner is None:
            raise RuntimeError(ERROR)
        if self.owner.process is not self.process: raise RuntimeError(ERROR)
        if self.owner._observe()!=('complete',None): raise RuntimeError(ERROR)
        if self.text(310)!=REMOVED: raise RuntimeError(ERROR)
        closed_apps(self.preflight,self.process)
        self.preflight.all_views_absent()
        for path in (self.binding.group,self.binding.generation,self.install/'installation.json',self.install/'transaction.json'):
            absent(path)
        self.uninstalled=True

    def _failure_step(self):
        errors=[]
        def refuse(): raise RuntimeError(ERROR)
        def attempt(label, action):
            try: action();return True
            except BaseException as error:
                errors.append(error)
                if label not in self.cleanup_errors: self.cleanup_errors.append(label)
                if self.cleanup_failure is None:
                    self.cleanup_failure={'step':label}
                    try: self.cleanup_failure.update(failure_location(error))
                    except BaseException as observation_error: errors.append(observation_error)
                return False
        resources_clean=attempt('resources',self.close_resources)
        if self.owner is None and self.process is not None:
            if self.install_requested:
                attempt('setup-owner-unresolved',refuse)
            else:
                def create_owner():
                    self.owner=SetupOwner(self.process,self.observation,lambda:self.button(305),self.quit_manager)
                attempt('setup-owner-unresolved',create_owner)
        if self.owner is not None and not self.owner.joined:
            def bound_owner():
                if self.owner.process is not self.process: raise RuntimeError(ERROR)
            if attempt('setup-identity',bound_owner):
                settled=attempt('setup-or-manager',self.owner.quiesce)
                if settled and resources_clean and self.install_requested and not self.uninstalled:
                    action=self.observe_uninstalled if self.uninstall_requested else self.uninstall
                    attempt('uninstall-unconfirmed',action)
                # Keep the ORIGINAL setup UI/owner for later verified removal;
                # a transient browser observation cannot discard this authority.
                if settled and resources_clean and (not self.install_requested or self.uninstalled):
                    attempt('setup-join-unconfirmed',self.owner.retire)
        if errors:
            raise next((e for e in errors if not isinstance(e,Exception)),errors[0])

    def failure_cleanup(self):
        if self.failure_cleanup_attempted: return
        self.failure_cleanup_attempted=True
        independent((self._failure_step,lambda:self.record_diagnostic('parent-cleanup.private.json')))

    def continue_retirement(self):
        if not self.run_attempted or not self.final_cleanup_attempted or not self.failure_cleanup_attempted:
            raise RuntimeError(ERROR)
        self._failure_step()
        return self.cleanup_complete()

    def cleanup(self):
        if not self.final_cleanup_attempted:
            self.final_cleanup_attempted=True
            if self.cleanup_complete() is not True:
                if self.failure_cleanup_attempted:
                    # One failure-only continuation before returning a hold.
                    # This cannot repair the original execution or its evidence.
                    try: self._failure_step()
                    except Exception: return False
                else: self.failure_cleanup()
        return self.cleanup_complete()

    def checkpoint(self, name):
        super().checkpoint(name)
        self.occurrences[name]=self.occurrences.get(name,0)+1
        if self.fault==(name,self.occurrences[name]): raise RuntimeError('owned parent transport fault')

    def inputs(self):
        ordinary(self.executable)
        if file_sha256(self.executable)!=self.browser_sha256: raise RuntimeError(ERROR)
        xpi,meta=candidate_input(self.candidate,self.candidate_sha256)
        if self.input_identity is None: self.input_identity=copy.deepcopy(meta)
        elif meta!=self.input_identity: raise RuntimeError(ERROR)
        return xpi,meta

    def prepare(self):
        self.report=new_report(self.report)
        self.inputs()
        super().prepare()

    def record_diagnostic(self, name):
        if name not in ('parent-failure.private.json','parent-cleanup.private.json'): raise RuntimeError(ERROR)
        if self.plan is not None and self.plan.created:
            if len(self.browsers)>2: raise RuntimeError(ERROR)
            with (self.plan.path/name).open('x',encoding='utf-8') as out:
                json.dump({'version':2,'qualification':False,'browsers':[b.diagnostic() for b in self.browsers],
                           'cleanup_failure':copy.deepcopy(self.cleanup_failure)},out)

    def failure_record(self, error):
        # Neither diagnostic sink may prevent the other or mask cancellation.
        independent((lambda:super(ParentInstalledRun,self).failure_record(error),
                     lambda:self.record_diagnostic('parent-failure.private.json')))

    def scope(self): return 'owned installed parent transport and manual Firefox UI'

    def transfer(self, identity):
        xpi,metadata=self.inputs()
        self.candidate_metadata=metadata
        peer=BrowserPeer(self.owner,self.binding,self.current_binding)
        fixture=Fixture(large_size=0,owners=self.fixtures)
        label=None
        for index in range(2):
            self.stage='parent-browser-start'
            browser=ParentBrowser(self.executable,self.plan.path/f'ParentFirefox{index}',
                                  {**self.environment,'MOZ_CRASHREPORTER_DISABLE':'1'},peer)
            self.browsers.append(browser)  # Before profile/process/handle/observer effects.
            self.inputs()
            browser.start()
            self.stage='parent-candidate-load'
            browser.load(xpi)
            self.checkpoint('bridge-started')
            if index==0:
                count=browser.script("return document.querySelectorAll('.task').length;")
                if type(count) is not int or count!=0: raise RuntimeError(ERROR)
                browser.fill({'url':fixture.url('range'),'filename':'owned-parent-installed.bin',
                              'destination':str(self.destination),'checksum':expected_sha256(SMALL_SIZE)})
                browser.click('#submit')
            self.stage='parent-manual-completion'
            current=browser.task('owned-parent-installed.bin','completed')
            count=browser.script("return document.querySelectorAll('.task').length;")
            if (type(count) is not int or count!=1 or type(current) is not str or not 0<len(current)<=128
                    or (label is not None and current!=label)): raise RuntimeError(ERROR)
            label=current
            output=self.destination/'owned-parent-installed.bin'
            ordinary(output)
            if output.stat().st_size!=SMALL_SIZE or file_sha256(output)!=expected_sha256(SMALL_SIZE): raise RuntimeError(ERROR)
            self.checkpoint('completed')
            self.stage='parent-explicit-disable-retirement'
            browser.close()
            self.sdk_checks.append(browser.evidence())
            if self.owner._observe()[1]!=identity or not self.ui.tray(self.manager_window): raise RuntimeError(ERROR)
        self.checks.append('parent_sdk_manual_ui_one_8mib_task_independent_output_two_temporary_lifetimes')
        self.checkpoint('reconnected')
        return output,SMALL_SIZE,expected_sha256(SMALL_SIZE),{'helper_execution':'sdk_owned_not_independently_architecture_queried'}

    def additional_evidence(self):
        self.inputs()
        if len(self.sdk_checks)!=2 or len(self.browsers)!=2 or self.cleanup_complete() is not True: raise RuntimeError(ERROR)
        return {'qualification':False,'temporary_xpi':True,'capture_ready':False,
                'parent_transport_checks':self.sdk_checks,'parent_xpi_sha256':self.candidate_sha256,
                'parent_candidate_source':self.candidate_metadata['source_commit'],'firefox_exe_sha256':self.browser_sha256}

    def cleanup_complete(self):
        setup=((self.setup_start_attempted is False and self.process is None and self.owner is None)
               or (self.setup_start_attempted is True and self.process is not None
               and self.owner is not None and self.owner.process is self.process
               and self.owner.joined is True and type(self.process.poll()) is int))
        return (setup and not self.hosts and all(b.cleanup_complete() is True for b in self.browsers)
                and all(f.closed is True for f in self.fixtures) and all(not t.is_alive() for t in self.retained_threads()))

    def hold_failed_owners(self):
        # Unknown setup creation/partial process leases/missing SDK retirement
        # remain held even when ordinary process inventory happens to be empty.
        announced=False
        while not self.cleanup_complete():
            try: super().hold_failed_owners()
            except BaseException: pass
            if self.cleanup_complete(): break
            try:
                if not announced:
                    print('Parent transport cleanup unresolved; retaining exact owners and domain.',flush=True)
                    announced=True
                time.sleep(1)
            except BaseException: pass
