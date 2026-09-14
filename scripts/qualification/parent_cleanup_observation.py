"""Cleanup-only readback after automation uncertainty, never repaired qualification.

The original collector, process object, sandbox and strict monotonic schema stay
bound. No reinstall, removal replay, PID discovery or new SDK action is provided.
"""
from .firefox import AutomationError
from .parent_transport_observation import ObserverClient as BaseClient, Observation, ERROR
from .parent_observation import _keys


class CleanupObservation(Observation):
    """Tracks retirement facts but deliberately cannot issue success receipts."""
    def require_retired(self): raise RuntimeError(ERROR)
    def require_removed(self): raise RuntimeError(ERROR)


class CleanupObserverClient(BaseClient):
    def __init__(self, browser, nonce):
        super().__init__(browser,nonce)
        self.retirement=CleanupObservation(nonce,self.collector)

    def _invoke(self, operation):
        # Match the original exact-process/sandbox envelope, but restore context
        # independently: a later restore error cannot replace cancellation.
        browser=self.browser
        if (browser.verified is not True or browser.closed is not False or self.process is None
                or browser.process is not self.process): raise RuntimeError(ERROR)
        failure=None
        value=None
        try:
            browser.command('Marionette:SetContext',{'value':'chrome'})
            value=browser.command('WebDriver:ExecuteAsyncScript',{'script':self.source,
                'args':[operation,self.nonce,self.collector],'newSandbox':False,'sandbox':self.sandbox})
            try:
                _keys(value,'value')
                self.retirement.validate(value['value'])
            except BaseException:
                # Keep known corruption quarantined even if restoration is
                # subsequently interrupted and cancellation takes precedence.
                self.evidence.invalidate();self.retirement.invalidate()
                raise
        except BaseException as error: failure=error
        try:
            if browser.process is not self.process or browser.closed is not False:
                self.evidence.invalidate();self.retirement.invalidate()
                raise RuntimeError(ERROR)
            browser.command('Marionette:SetContext',{'value':'content'})
        except BaseException as error:
            if failure is None or (isinstance(failure,Exception) and not isinstance(error,Exception)): failure=error
        if failure is not None: raise failure
        return value['value']

    def _sample(self, operation):
        try:
            value=self._invoke(operation)  # Same retained process/sandbox/collector guards.
        except BaseException as error:
            self.evidence.invalidate()
            # A rejected automation command (or interruption) leaves a read gap,
            # not permission to reset parsed evidence. Other failures, including
            # an open/malformed WebDriver envelope, quarantine cleanup as well.
            if isinstance(error,Exception) and not isinstance(error,AutomationError):
                self.retirement.invalidate()
            if not isinstance(error,Exception): raise
            raise RuntimeError(ERROR) from None
        try:
            count=self.retirement.accept(value)
            if not self.evidence.failed: self.evidence.accept(value)
            return count
        except BaseException as error:
            self.evidence.invalidate()
            self.retirement.invalidate()
            if not isinstance(error,Exception): raise
            raise RuntimeError(ERROR) from None

    def _observe(self, operation):
        if self.evidence.failed: raise RuntimeError(ERROR)
        return self._sample(operation)

    def cleanup_snapshot(self):
        # This distinct operation never makes ordinary snapshot/qualification
        # usable again. Read-only observation is also allowed after an uncertain
        # removal, against the original collector rather than a replacement.
        if self.browser.failed is not True or not self.attempted or self.retirement.failed:
            raise RuntimeError(ERROR)
        return self._sample('snapshot')

    def remove(self):
        if not self.removal_attempted:
            self.removal_attempted=True
            if self.attempted:
                try:
                    self._sample('remove')
                    self.removal_returned=self.retirement.closed and self.retirement.removed
                except BaseException as error:
                    self.removal_returned=False
                    if not isinstance(error,Exception): raise
        return self.removal_returned

    def cleanup_remove(self):
        if self.browser.failed is not True: raise RuntimeError(ERROR)
        if not self.attempted: return True  # No collector registration was attempted.
        if self.cleanup_complete(): return True
        if not self.removal_attempted: return self.remove()
        # Never dispatch another inverse when delivery/return was uncertain.
        self.cleanup_snapshot()
        return self.cleanup_complete()

    def cleanup_complete(self):
        return not self.attempted or (not self.retirement.failed and self.retirement.closed and self.retirement.removed)
