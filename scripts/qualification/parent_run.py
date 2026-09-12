"""Opt-in registration-free SDK exchange/disable controller; never an install test.

Caller retains this object BEFORE execute(), its already-owned DomainPlan and the
independent build expectations. Hashes copied from supplied files are not build
provenance. No CLI, compiler, normal profile, shared registration or production
route is selected here. Unjoined/unknown cleanup leaves the owner and domain held.
"""
import copy
import json
import os
from pathlib import Path
import re
import struct
import time

from .firefox import Firefox
from .firefox_policy import validate as policy, unchanged as unchanged_policy
from .installed import ordinary
from .native import file_sha256
from .parent_input import ADDON, ARCHIVE, BuildExpectation, inspect
from .parent_observation import ObserverClient
from .protection_run import LOAD, load_observation
from .setup_owner import DomainPlan
from .support import ARTIFACTS, new_report, write_report
from .xpi_persistence import preflight

CONTROL = Path(__file__).with_name('parent_control.js').read_text(encoding='utf-8')
ERROR = 'owned parent SDK run refused; retain controller and domain'


def active(value):
    expected = {'state':'active','id':ADDON,'version':'0.0.1','temporary':True,
                'privateAllowed':False,'persistentBackground':False}
    if json.dumps(value, sort_keys=True) != json.dumps(expected, sort_keys=True): raise RuntimeError(ERROR)


def disabled(value, *, allow_absent=False):
    if not isinstance(value, dict) or set(value) != {'state'} or value['state'] not in ('disabled', 'absent'):
        raise RuntimeError(ERROR)
    if value['state'] == 'absent' and not allow_absent: raise RuntimeError(ERROR)


class ParentRun:
    """One consumed domain and explicit SDK mode, not a reusable browser launcher.

    The first case observes native retirement BEFORE closing Firefox. It does not
    prove delivery during browser shutdown or native work continuing across it.
    """
    def __init__(self, plan, expected, archive_sha256, executable, browser_sha256, report,
                 *, parent_stdio_experiment=False):
        if parent_stdio_experiment is not True: raise RuntimeError('explicit parent stdio experiment mode required')
        if not __debug__ or os.name != 'nt' or struct.calcsize('P') != 8:
            raise RuntimeError('parent SDK run requires assertions and 64-bit Windows')
        if type(plan) is not DomainPlan or plan.created is not True or type(expected) is not BuildExpectation or plan.path != expected.domain:
            raise RuntimeError('retained parent fixture domain/build required')
        if (not isinstance(executable, Path) or not isinstance(browser_sha256, str)
                or re.fullmatch('[a-f0-9]{64}', browser_sha256) is None): raise RuntimeError(ERROR)
        self.plan, self.expected, self.archive_sha256 = plan, expected, archive_sha256
        self.executable, self.browser_sha256, self.report = executable, browser_sha256, report
        self.browser = self.observer = self.process = None
        self.started = self.load_attempted = self.disable_attempted = False
        self.browser_start_attempted = False
        self.browser_close_attempted = self.cleanup_attempted = False
        self.cleanup_errors = []
        self.before = self.identity = self.load_result = self.native_receipt = None
        self.stage = 'planned'
        self.browser_joined = False
        self.browser_exit = self.browser_pid = None
        self.observer_removed = False

    def _inputs(self):
        identity = inspect(self.plan.path, self.expected, self.archive_sha256)
        ordinary(self.executable)
        if file_sha256(self.executable) != self.browser_sha256: raise RuntimeError('parent Firefox image changed')
        if self.identity is not None and identity != self.identity: raise RuntimeError('parent input identity changed')
        return identity

    def _disable(self, *, cleanup=False):
        if self.disable_attempted: return
        self.disable_attempted = True  # An uncertain action is never replayed.
        disabled(self.browser.chrome(CONTROL, ['disable'], True), allow_absent=cleanup)

    def _wait(self, count):
        deadline = time.monotonic() + 20
        while True:
            observed = self.observer.snapshot()
            if observed == count: return
            if observed > count or time.monotonic() >= deadline: raise RuntimeError('parent fixture observation deadline')
            time.sleep(0.05)

    def _close_browser(self):
        if self.browser is None or self.browser_close_attempted: return
        self.browser_close_attempted = True
        if self.process is None:
            self.process = self.browser.process  # May have been acquired during failed start.
        elif self.browser.process is not self.process:
            raise RuntimeError('retained parent Firefox process replaced')
        self.browser.close()
        if self.process is None:
            if self.browser.closed is not True: raise RuntimeError(ERROR)
            return
        if self.browser.process is not self.process or self.browser.closed is not True: raise RuntimeError(ERROR)
        code = self.process.wait(timeout=0)
        if type(code) is not int: raise RuntimeError(ERROR)
        self.browser_joined, self.browser_exit = True, code
        if code != 0: raise RuntimeError('successful retained Firefox exit required')

    def cleanup(self):
        """Best effort once, without dropping owners or converting errors to success."""
        if self.cleanup_attempted: return self.cleanup_complete()
        self.cleanup_attempted = True
        interruptions = []
        def attempt(stage, action):
            try: action()
            except BaseException as error:
                self.cleanup_errors.append(stage)
                if not isinstance(error, Exception): interruptions.append(error)
        if self.browser is not None and self.load_attempted and not self.disable_attempted:
            attempt('disable', lambda: self._disable(cleanup=True))
        if self.observer is not None:
            if self.load_attempted and self.native_receipt is None and not self.observer.removal_attempted:
                attempt('native-retirement', lambda: self._wait(2))
            def remove():
                self.observer_removed = self.observer.remove() is True
                if not self.observer_removed: raise RuntimeError(ERROR)
                if self.load_attempted: self.native_receipt = self.observer.evidence.require_removed()
            attempt('observer-remove', remove)
        # Never skip the retained browser owner because a control/observer failed.
        attempt('browser-close', self._close_browser)
        if interruptions: raise interruptions[0]
        return self.cleanup_complete()

    def cleanup_complete(self):
        browser_settled = self.browser is None or (self.browser.closed is True and
            (self.browser_joined or (not self.browser_start_attempted and self.browser.process is None)))
        # Loading can invoke the SDK before the load response. Missing receipt is
        # UNKNOWN native lifetime, even after observed browser exit/absence.
        native_settled = not self.load_attempted or self.native_receipt is not None
        return not self.cleanup_errors and browser_settled and native_settled and (self.observer is None or self.observer_removed)

    def _failure(self):
        value = {'version':1, 'qualification':False,'stage':self.stage,'load_attempted':self.load_attempted,
                 'disable_attempted':self.disable_attempted,'browser_start_attempted':self.browser_start_attempted,
                 'browser_close_attempted':self.browser_close_attempted,
                 'browser_joined':self.browser_joined,'browser_exit':self.browser_exit,'browser_pid':self.browser_pid,
                 'native_retirement':'observed' if self.native_receipt is not None else 'unknown' if self.load_attempted else 'not-invoked',
                 'observer_removed':self.observer_removed,'cleanup_complete':self.cleanup_complete(),
                 'cleanup_errors':list(self.cleanup_errors)}
        try:
            with (self.plan.path / 'sdk-failure.private.json').open('x', encoding='utf-8') as out: json.dump(value,out)
        except OSError: pass  # Failed recording cannot permit a success report.

    def execute(self):
        if self.started or self.cleanup_attempted: raise RuntimeError('parent SDK controller already consumed')
        self.started = True
        if self.plan.created is not True or self.plan.path != self.expected.domain:
            raise RuntimeError('retained parent fixture domain changed')
        ordinary(self.plan.path)
        if self.plan.path.parent != ARTIFACTS: raise RuntimeError(ERROR)
        # Exclusive claim BEFORE preflight, profile creation or any application.
        with (self.plan.path / 'sdk-run.private.json').open('x', encoding='utf-8') as out:
            json.dump({'version':1,'qualification':False,'mode':'parent-stdio-experiment'},out)
        try:
            self.stage = 'preflight'
            preflight()
            self.report = new_report(self.report)
            self.identity = self._inputs()
            profile = self.plan.path / 'Firefox'
            ordinary(profile)
            if profile.exists(): raise RuntimeError('new parent SDK profile required')
            self.stage = 'environment'
            for folder in ('local','roaming','home'): (self.plan.path / folder).mkdir()
            environment = {**os.environ, 'LOCALAPPDATA':str(self.plan.path/'local'),
                'APPDATA':str(self.plan.path/'roaming'),'USERPROFILE':str(self.plan.path/'home'),
                'HOME':str(self.plan.path/'home'),'PATH':str(Path(os.environ['WINDIR'])/'System32'),
                'MOZ_CRASHREPORTER_DISABLE':'1'}
            preflight()  # Original guards immediately before new browser ownership.
            self._inputs()
            self.stage = 'browser-start'
            self.browser = Firefox(self.executable, profile, environment, fileless_experiment=True)
            self.browser_start_attempted = True
            self.browser.start()  # Store owner BEFORE acquiring its process.
            self.process = self.browser.process
            if self.process is None: raise RuntimeError(ERROR)
            pid = self.process.pid
            reported = self.browser.chrome('return Services.appinfo.processID;')
            if (type(pid) is not int or not 0 < pid <= 0xffffffff or type(reported) is not int or reported != pid):
                raise RuntimeError('parent Firefox process correlation refused')
            self.browser_pid = pid
            with (self.plan.path/'browser-owner.private.json').open('x', encoding='utf-8') as out:
                json.dump({'version':1,'qualification':False,'pid':pid,'image_sha256':self.browser_sha256},out)
            self.before = copy.deepcopy(policy(self.browser.automation_policy, fileless_experiment=True))
            self.stage = 'observer-install'
            self.observer = ObserverClient(self.browser, self.expected.nonce)
            self.observer.install()
            self.stage = 'temporary-load'
            self._inputs()
            if self.browser.chrome(CONTROL, ['absent'], True) != {'state':'absent'}:
                raise RuntimeError('existing parent fixture add-on refused')
            self.load_attempted = True
            self.load_result = load_observation(self.browser.chrome(LOAD, [str(self.plan.path/'probe'/ARCHIVE), ADDON], True))
            if self.load_result['state'] != 'loaded': raise RuntimeError(ERROR)
            self.stage = 'exchange'
            active(self.browser.chrome(CONTROL, ['info'], True))
            self._wait(1)
            self.stage = 'explicit-disable'
            self._disable()
            self.stage = 'native-retirement'
            self._wait(2)
            self.observer.evidence.require_retired()
            self.stage = 'observer-remove'
            self.observer_removed = self.observer.remove() is True
            if not self.observer_removed: raise RuntimeError(ERROR)
            self.native_receipt = self.observer.evidence.require_removed()
            self.stage = 'policy-readback'
            unchanged_policy(self.browser, self.before, fileless_experiment=True)
            self.stage = 'browser-close'
            self._close_browser()
            if not self.browser_joined or self.browser_exit != 0: raise RuntimeError(ERROR)
            self.stage = 'postflight'
            preflight()
            self._inputs()
            self.stage = 'report'
            if not self.cleanup_complete(): raise RuntimeError(ERROR)
            write_report(self.report, {'format':'parent-stdio-sdk-disable','version':1,'qualification':False,
                'm5_install_ready':False,'temporary_loading_used':True,'profile_mode':'parent-stdio-experiment',
                'identity':self.identity,'firefox_exe_sha256':self.browser_sha256,'owned_automation_policy':self.before,
                'receipt':self.native_receipt,'explicit_disable_observed':True,'browser_joined':True,'browser_exit':0,
                'browser_pid':self.browser_pid,
                'registration_unchanged_absent':True,'native_retired_before_browser_close':True,
                'browser_shutdown_with_active_native_qualified':False})
        except BaseException as error:
            try: self.cleanup()
            finally: self._failure()
            if not isinstance(error, Exception): raise
            raise RuntimeError(ERROR) from None
