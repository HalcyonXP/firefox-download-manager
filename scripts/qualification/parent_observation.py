"""Plain parent-fixture observations, not process ownership or live-mode authority.

The caller must retain its separately reviewed disposable-profile Firefox owner.
This module launches nothing and does not authorize a profile/preference exception.
"""
import json
from pathlib import Path
import re
import uuid

from .support import unique_object, invalid_constant

SOURCE = Path(__file__).with_name('parent_observer.js').read_text(encoding='utf-8')
UUID = r'[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}'
ERROR = 'owned parent observation refused'
SCOPE = 'owned-parent-stdio-v1'


def _uuid(value):
    return isinstance(value, str) and re.fullmatch(UUID, value) is not None


def _keys(value, keys):
    if not isinstance(value, dict) or set(value) != set(keys.split()): raise RuntimeError(ERROR)


def _pid(value):
    if type(value) is not int or not 0 < value <= 0xffffffff: raise RuntimeError(ERROR)
    return value


def _record(raw, nonce, stage, pid=None):
    if not isinstance(raw, str) or not raw.isascii() or not 0 < len(raw) <= 8192: raise RuntimeError(ERROR)
    try: value = json.loads(raw, object_pairs_hook=unique_object, parse_constant=invalid_constant)
    except (ValueError, RecursionError): raise RuntimeError(ERROR) from None
    _keys(value, 'nonce kind value')
    if value['nonce'] != nonce or value['kind'] != ('ready' if stage == 'echoed' else 'retired'): raise RuntimeError(ERROR)
    body = value['value']
    _keys(body, 'version qualification scope stage pid' + (' attempted echoed launcher successful' if stage == 'retired' else ''))
    if (type(body['version']) is not int or body['version'] != 1 or body['qualification'] is not False
            or body['scope'] != SCOPE or body['stage'] != stage): raise RuntimeError(ERROR)
    actual_pid = _pid(body['pid'])
    if pid is not None and actual_pid != pid: raise RuntimeError(ERROR)
    if stage == 'retired':
        if any(body[key] is not True for key in ('attempted', 'echoed', 'successful')): raise RuntimeError(ERROR)
        launcher = body['launcher']; _keys(launcher, 'spawn_called transport hooks_removed successful')
        if any(launcher[key] is not True for key in ('spawn_called', 'hooks_removed', 'successful')): raise RuntimeError(ERROR)
        transport = launcher['transport']
        _keys(transport, 'startup process_waited exit_code pipes_closed io_settled forced successful')
        if (transport['startup'] != 'started' or type(transport['exit_code']) is not int or transport['exit_code'] != 0
                or transport['forced'] is not False
                or any(transport[key] is not True for key in ('process_waited', 'pipes_closed', 'io_settled', 'successful'))):
            raise RuntimeError(ERROR)
    return actual_pid


class Observation:
    """Monotonic, sticky-failure evidence. A PID/receipt is not an OS process handle."""
    def __init__(self, nonce, collector):
        if not _uuid(nonce) or not _uuid(collector) or nonce == collector: raise RuntimeError(ERROR)
        self.nonce, self.collector = nonce, collector
        self._records = ()
        self._closed = self._removed = self._failed = False
        self.pid = None

    def accept(self, snapshot):
        try:
            if self._failed: raise RuntimeError(ERROR)
            _keys(snapshot, 'version qualification collector state removed failed records')
            if (type(snapshot['version']) is not int or snapshot['version'] != 1 or snapshot['qualification'] is not False
                    or snapshot['collector'] != self.collector or snapshot['state'] not in ('active', 'closed')
                    or snapshot['failed'] is not False or type(snapshot['removed']) is not bool
                    or (snapshot['removed'] and snapshot['state'] != 'closed')
                    or (self._closed and snapshot['state'] != 'closed') or (self._removed and not snapshot['removed'])):
                raise RuntimeError(ERROR)
            raw = snapshot['records']
            if not isinstance(raw, list) or len(raw) > 2 or tuple(raw[:len(self._records)]) != self._records:
                raise RuntimeError(ERROR)
            pid = None
            for index, record in enumerate(raw): pid = _record(record, self.nonce, 'echoed' if index == 0 else 'retired', pid)
            self._records = tuple(raw)
            self.pid = pid
            self._closed = snapshot['state'] == 'closed'
            self._removed = snapshot['removed']
            return len(self._records)
        except BaseException as error:
            self._failed = True
            if not isinstance(error, Exception): raise
            raise RuntimeError(ERROR) from None

    def invalidate(self):
        self._failed = True

    def require_retired(self):
        if self._failed or len(self._records) != 2: raise RuntimeError(ERROR)
        return self.pid

    def require_removed(self):
        pid = self.require_retired()
        if not self._closed or not self._removed: raise RuntimeError(ERROR)
        return {'version':1, 'qualification':False, 'scope':SCOPE, 'pid':pid,
                'exchange_observed':True, 'sdk_retirement_observed':True, 'observer_removal_observed':True}


class ObserverClient:
    """One named automation sandbox; retain this client before install can act.

    Removal is attempted even after uncertain command delivery. A failed removal
    is retained, not retried against a replacement realm or mistaken for joining
    Firefox. The separately retained browser must still be retired by its caller.
    """
    def __init__(self, browser, nonce):
        collector = str(uuid.uuid4())
        self.evidence = Observation(nonce, collector)
        self.browser, self.process = browser, browser.process
        self.collector, self.nonce = collector, nonce
        self.sandbox = 'owned-parent-observer-' + collector
        self.attempted = self.removal_attempted = False
        self.removal_returned = False

    def _invoke(self, operation):
        browser = self.browser
        if (browser.verified is not True or browser.closed is not False or self.process is None
                or browser.process is not self.process): raise RuntimeError(ERROR)
        try:
            browser.command('Marionette:SetContext', {'value':'chrome'})
            value = browser.command('WebDriver:ExecuteAsyncScript', {'script':SOURCE,
                'args':[operation, self.nonce, self.collector], 'newSandbox':False, 'sandbox':self.sandbox})
        finally:
            if browser.process is not self.process or browser.closed is not False: raise RuntimeError(ERROR)
            browser.command('Marionette:SetContext', {'value':'content'})
        # WebDriver wraps the result; an open/extra envelope cannot become evidence.
        _keys(value, 'value')
        return value['value']

    def _observe(self, operation):
        try: return self.evidence.accept(self._invoke(operation))
        except BaseException as error:
            self.evidence.invalidate()
            if not isinstance(error, Exception): raise
            raise RuntimeError(ERROR) from None

    def install(self):
        if self.attempted or self.removal_attempted: raise RuntimeError(ERROR)
        self.attempted = True
        self._observe('install')

    def snapshot(self):
        if not self.attempted or self.removal_attempted: raise RuntimeError(ERROR)
        return self._observe('snapshot')

    def remove(self):
        if not self.removal_attempted:
            self.removal_attempted = True
            if self.attempted:
                try:
                    value = self._invoke('remove')
                    self.evidence.accept(value)
                    self.removal_returned = value['state'] == 'closed' and value['removed'] is True
                except BaseException as error:
                    self.evidence.invalidate()
                    self.removal_returned = False
                    if not isinstance(error, Exception): raise
        return self.removal_returned
