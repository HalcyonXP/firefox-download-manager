"""Fileless recovery models: no browser, native process or SDK execution."""
import json
import unittest
from unittest.mock import patch
from qualification.firefox import AutomationError
from qualification.parent_cleanup_observation import CleanupObserverClient as Client
from test_parent_transport_observation import N, C, native, snapshot


class Browser:
    def __init__(self):
        self.process=object();self.verified=True;self.closed=False;self.failed=False
        self.operations=[];self.restore_failure=False;self.record=None;self.removed=False
        self.changed=None

    def command(self,name,arguments=None):
        if name=='Marionette:SetContext':
            if arguments=={'value':'content'} and self.restore_failure:
                self.restore_failure=False
                raise AutomationError(name,{'error':'no such window'})
            return None
        if name!='WebDriver:ExecuteAsyncScript': raise AssertionError('unexpected modeled command')
        operation,nonce,collector=arguments['args']
        if nonce!=N or collector!=C: raise AssertionError('changed collector binding')
        self.operations.append(operation)
        if operation=='remove': self.removed=True
        value=snapshot([] if self.record is None else [self.record],self.removed)
        if self.changed is not None: value=self.changed(value)
        return {'value':value}


def client():
    browser=Browser()
    with patch('qualification.parent_observation.uuid.uuid4',return_value=C): result=Client(browser,N)
    result.install()
    return result,browser


class ParentCleanupObservationTests(unittest.TestCase):
    def test_valid_removed_readback_does_not_erase_prior_observation_failure(self):
        observer,browser=client();browser.record=json.dumps(native());browser.restore_failure=True
        with self.assertRaises(RuntimeError): observer.snapshot()
        browser.failed=True
        self.assertTrue(observer.evidence.failed)
        removed=observer.remove()
        self.assertTrue(browser.removed)
        self.assertEqual(browser.operations.count('remove'),1)
        self.assertTrue(removed,'completed observer inverse is blocked by prior automation observation failure')
        self.assertTrue(observer.evidence.failed)
        with self.assertRaises(RuntimeError): observer.evidence.require_removed()

    def test_read_gap_allows_only_cleanup_and_never_a_success_receipt(self):
        observer,browser=client();browser.record=json.dumps(native());browser.restore_failure=True
        with self.assertRaises(RuntimeError): observer.snapshot()
        before=list(browser.operations)
        with self.assertRaises(RuntimeError): observer.cleanup_snapshot()
        self.assertEqual(browser.operations,before)
        browser.failed=True
        self.assertEqual(observer.cleanup_snapshot(),1)
        self.assertTrue(observer.retirement.resource_retired())
        with self.assertRaises(RuntimeError): observer.snapshot()
        for evidence in (observer.evidence,observer.retirement):
            with self.assertRaises(RuntimeError): evidence.require_retired()
        self.assertTrue(observer.cleanup_remove());self.assertTrue(observer.cleanup_complete())
        with self.assertRaises(RuntimeError): observer.retirement.require_removed()
        self.assertEqual(browser.operations,['install','snapshot','snapshot','remove'])

    def test_uncertain_remove_is_read_back_not_replayed_or_relabelled_returned(self):
        observer,browser=client();browser.record=json.dumps(native());observer.snapshot()
        browser.restore_failure=True
        self.assertFalse(observer.remove());self.assertFalse(observer.removal_returned)
        self.assertFalse(observer.remove());browser.failed=True
        self.assertTrue(observer.cleanup_remove());self.assertTrue(observer.cleanup_complete())
        self.assertFalse(observer.removal_returned)
        self.assertEqual(browser.operations,['install','snapshot','remove','snapshot'])
        self.assertTrue(observer.cleanup_remove())
        self.assertEqual(browser.operations.count('remove'),1)
        with self.assertRaises(RuntimeError): observer.evidence.require_removed()

    def test_parsed_prefix_replacement_and_bad_collector_permanently_quarantine_cleanup(self):
        for changed in (lambda x:{**x,'records':[]},lambda x:{**x,'collector':N},lambda x:{**x,'failed':True}):
            observer,browser=client();browser.record=json.dumps(native());observer.snapshot()
            browser.restore_failure=True
            with self.assertRaises(RuntimeError): observer.snapshot()
            browser.failed=True;browser.changed=changed
            with self.assertRaises(RuntimeError): observer.cleanup_snapshot()
            browser.changed=None;before=list(browser.operations)
            with self.assertRaises(RuntimeError): observer.cleanup_snapshot()
            self.assertEqual(browser.operations,before)
            self.assertTrue(observer.retirement.failed);self.assertFalse(observer.retirement.resource_retired())

    def test_replacement_process_and_open_envelope_do_not_become_read_gaps(self):
        for mode in ('process','envelope'):
            observer,browser=client();browser.record=json.dumps(native())
            if mode=='process': browser.process=object()
            else:
                original=browser.command
                def command(name,args=None):
                    result=original(name,args)
                    return {**result,'extra':False} if name=='WebDriver:ExecuteAsyncScript' else result
                browser.command=command
            with self.assertRaises(RuntimeError): observer.snapshot()
            self.assertTrue(observer.retirement.failed)
            browser.failed=True
            with self.assertRaises(RuntimeError): observer.cleanup_snapshot()

    def test_invalid_return_envelope_is_not_hidden_by_a_later_restore_error(self):
        for mode in ('envelope','collector','record'):
            for cancelled in (False,True):
                observer,browser=client();original=browser.command;browser.restore_failure=True
                if mode=='collector':browser.changed=lambda x:{**x,'collector':N}
                if mode=='record':browser.changed=lambda x:{**x,'records':['{}']}
                def command(name,args=None):
                    if cancelled and name=='Marionette:SetContext' and args=={'value':'content'}:raise KeyboardInterrupt()
                    result=original(name,args)
                    return {**result,'extra':False} if mode=='envelope' and name=='WebDriver:ExecuteAsyncScript' else result
                browser.command=command
                with self.assertRaises(KeyboardInterrupt if cancelled else RuntimeError):observer.snapshot()
                self.assertTrue(observer.evidence.failed)
                self.assertTrue(observer.retirement.failed,'complete invalid envelope must not become a read gap')

    def test_identity_change_during_rejected_or_interrupted_request_quarantines_cleanup(self):
        for cancelled in (False,True):
            observer,browser=client();original=browser.command
            def command(name,args=None):
                if name=='WebDriver:ExecuteAsyncScript':
                    browser.process=object()
                    if cancelled:raise KeyboardInterrupt()
                    raise AutomationError(name,{'error':'unknown error'})
                return original(name,args)
            browser.command=command
            with self.assertRaises(KeyboardInterrupt if cancelled else RuntimeError):observer.snapshot()
            self.assertTrue(observer.evidence.failed);self.assertTrue(observer.retirement.failed)
            browser.failed=True;browser.process=observer.process
            with self.assertRaises(RuntimeError):observer.cleanup_snapshot()

    def test_context_restore_cannot_mask_interruption_and_cleanup_stays_separate(self):
        observer,browser=client();original=browser.command;browser.restore_failure=True
        def command(name,args=None):
            if name=='WebDriver:ExecuteAsyncScript': raise KeyboardInterrupt()
            return original(name,args)
        browser.command=command
        with self.assertRaises(KeyboardInterrupt): observer.snapshot()
        self.assertTrue(observer.evidence.failed);self.assertFalse(observer.retirement.failed)
        browser.command=original;browser.failed=True;browser.record=json.dumps(native())
        self.assertEqual(observer.cleanup_snapshot(),1)
        self.assertTrue(observer.cleanup_remove())
        with self.assertRaises(RuntimeError): observer.evidence.require_removed()

    def test_unattempted_collector_needs_no_inverse_and_missing_sdk_wait_stays_unretired(self):
        browser=Browser();browser.failed=True
        with patch('qualification.parent_observation.uuid.uuid4',return_value=C): observer=Client(browser,N)
        self.assertTrue(observer.cleanup_complete());self.assertTrue(observer.cleanup_remove())
        self.assertEqual(browser.operations,[])
        observer,browser=client();item=native();item['receipt']['launcher']['transport']['process_waited']=False
        browser.record=json.dumps(item);browser.failed=True
        self.assertEqual(observer.cleanup_snapshot(),1)
        self.assertFalse(observer.retirement.resource_retired())
        self.assertTrue(observer.cleanup_remove()) # Collector removal is not SDK retirement.
        self.assertTrue(observer.cleanup_complete());self.assertFalse(observer.retirement.resource_retired())


if __name__=='__main__': unittest.main()
