"""Opt-in installed parent transport/manual UI slice; no persistent or capture qualification.

Retain the run before execute. Original installation/all-view and BrowserPeer
preflights remain. No signing override, normal-profile access or PID adoption.
"""
import copy
from functools import wraps
import os
from pathlib import Path
import struct
import time
import uuid
from .browser_peer import BrowserPeer
from .firefox import Firefox
from .firefox_policy import validate as validate_policy
from .fixture import Fixture, SMALL_SIZE, expected_sha256
from .installed import InstalledRun, FAULTS, ordinary, wait
from .native import file_sha256
from .parent_candidate import candidate_input
from .parent_transport_observation import ObserverClient
from .process_lease import ProcessLease
from .support import new_report

CONTROL=Path(__file__).with_name('parent_transport_control.js').read_text(encoding='utf-8')
ERROR='owned installed parent transport refused; retain owner and domain'


def control(browser, operation, expected):
    value=browser.chrome(CONTROL,[operation],True)
    if type(value) is not dict or set(value)!=set(expected) or any(type(value[k]) is not type(v) or value[k]!=v for k,v in expected.items()):
        raise RuntimeError(ERROR)



def retained_failure(method):
    @wraps(method)
    def invoke(self,*args,**kwargs):
        try: return method(self,*args,**kwargs)
        except BaseException:
            self.failed=True
            raise
    return invoke


class ParentBrowser(Firefox):
    def __init__(self, executable, profile, environment, peer):
        super().__init__(executable,profile,environment,owned_peer=peer,parent_transport_experiment=True)
        self.original=self.parent_lease=self.observer=None
        self.load_attempted=self.disable_attempted=self.disabled_observed=self.browser_close_attempted=False
        self.failed=False

    @retained_failure
    def start(self):
        if self.failed or self.launch_attempted or self.original is not None or self.parent_lease is not None: raise RuntimeError(ERROR)
        try: super().start()
        finally: self.original=self.process
        reported=self.chrome('return Services.appinfo.processID;')
        self.parent_lease=ProcessLease(self.original,reported,self.executable)
        self.parent_lease.acquire()  # Retained before acquisition and add-on code.
        self.observer=ObserverClient(self,str(uuid.uuid4()))
        self.observer.install()
        if self.observer.snapshot()!=0: raise RuntimeError(ERROR)
        control(self,'absent',{'state':'absent'})
        return self

    @retained_failure
    def load(self, xpi):
        if self.failed or self.load_attempted or self.parent_lease is None or self.parent_lease.acquired is not True: raise RuntimeError(ERROR)
        self.load_attempted=True
        super().load(xpi)
        expected={'state':'active','id':'download-manager@halcyonxp.local','version':'0.3.0',
                  'temporary':True,'privateAllowed':False,'persistentBackground':False}
        control(self,'info',expected)

    @retained_failure
    def close(self):
        if self.launch_attempted and self.original is None: raise RuntimeError(ERROR)
        if self.original is not None and self.process is not self.original: raise RuntimeError(ERROR)
        if self.load_attempted:
            if not self.disable_attempted:
                self.disable_attempted=True
                control(self,'disable',{'state':'disabled'})
                self.disabled_observed=True
            if not self.disabled_observed:
                # Unknown command delivery permits fresh metadata observation,
                # never another disable dispatch or a presumed application state.
                control(self,'disabled',{'state':'disabled'})
                self.disabled_observed=True
            if not self.observer.evidence.resource_retired():
                wait(lambda: self.observer.snapshot()==1 and self.observer.evidence.resource_retired(),20)
        if self.observer is not None and not self.observer.remove(): raise RuntimeError(ERROR)
        error=None
        if not self.browser_close_attempted:
            self.browser_close_attempted=True
            try: super().close()
            except BaseException as failure: error=failure
        elif not self.closed and self.original is not None:
            # Only continued exact wait/closed-app observations; never repeat Quit.
            self.original.wait(timeout=5)
            self._require_apps_closed()
            self.closed=True
        try:
            if self.parent_lease is not None and not self.parent_lease.cleanup_complete():
                if self.parent_lease.handles and not self.parent_lease.released:
                    wait(self.parent_lease.observe,5)
                    self.parent_lease.release()
                if not self.parent_lease.cleanup_complete(): raise RuntimeError(ERROR)
        except BaseException as failure:
            if error is not None and not isinstance(error,Exception): raise error
            raise failure
        if error is not None: raise error

    def cleanup_complete(self):
        return (self.launch_attempted is False or (self.launch_attempted is True and self.original is not None
                and self.process is self.original and self.closed is True and type(self.original.poll()) is int)) and (
                self.parent_lease is None or self.parent_lease.cleanup_complete() is True) and (
                self.observer is None or self.observer.removal_returned is True) and (
                self.load_attempted is False or (self.load_attempted is True and self.disabled_observed is True
                and self.observer.evidence.resource_retired() is True))

    def evidence(self):
        if self.failed is not False or self.cleanup_complete() is not True or self.original is None: raise RuntimeError(ERROR)
        code=self.original.wait(timeout=0)
        if type(code) is not int or code!=0: raise RuntimeError(ERROR)
        return {'parent':self.parent_lease.receipt(),'sdk':self.observer.evidence.require_removed(),
                'launcher_waited':True,'launcher_exit':0,'profile_mode':'parent-transport-experiment',
                'firefox_automation_policy':copy.deepcopy(validate_policy(self.automation_policy,parent_transport_experiment=True))}


class ParentInstalledRun(InstalledRun):
    def __init__(self, package, report, candidate, candidate_sha256, executable, browser_sha256,
                 *, parent_transport_experiment=False, fault=None):
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

    def execute(self):
        if self.run_attempted or self.final_cleanup_attempted: raise RuntimeError(ERROR)
        self.run_attempted=True
        super().execute()

    def failure_cleanup(self):
        if self.failure_cleanup_attempted: return
        self.failure_cleanup_attempted=True
        super().failure_cleanup()

    def cleanup(self):
        if not self.final_cleanup_attempted:
            self.final_cleanup_attempted=True
            if self.cleanup_complete() is not True: self.failure_cleanup()
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
        setup=(self.setup_start_attempted is False or (self.setup_start_attempted is True and self.process is not None
               and self.owner is not None and self.owner.joined is True and type(self.process.poll()) is int))
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
