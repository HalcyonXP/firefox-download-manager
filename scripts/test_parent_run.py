"""Controller models only: no executable image, compiler, browser or registry calls."""
from collections import Counter
from contextlib import contextmanager, ExitStack
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from qualification import parent_run as run


@contextmanager
def fixture(fault=None, interrupt=None, terminal=True, replacement=False, exit_code=0, reported_pid=1709, receipt=True, existing_addon=False, child_topology=False, parent_stays_live=False, lease_snapshot_failure=False):
    for injected in (fault,interrupt):
        assert injected is None or (type(injected) is tuple and len(injected)==2
                                    and type(injected[0]) is str and type(injected[1]) is int and injected[1]>0)
    with tempfile.TemporaryDirectory(prefix='parent-controller-model-') as temporary, ExitStack() as stack:
        root = Path(temporary).resolve()
        plan = run.DomainPlan.record(root, root)
        plan.create()
        expected = run.BuildExpectation('a'*40, False, plan.path, '11111111-1111-4111-8111-111111111111', 'b'*64, 'c'*64)
        events, counts, reports = [], Counter(), []
        def hit(name):
            events.append(name)
            counts[name] += 1
            if (name, counts[name]) == interrupt: raise KeyboardInterrupt()
            if (name, counts[name]) == fault: raise RuntimeError('modeled refusal')
        class Process:
            pid = 1709
            def __init__(self): self.waits = 0
            def wait(self, timeout):
                self.waits += 1
                hit('wait')
                return exit_code
        class Browser:
            def __init__(self, executable, profile, environment, *, fileless_experiment):
                self.profile = profile
                self.process = None
                self.closed = False
                self.automation_policy = {'modeled':True}
                assert fileless_experiment is True
                assert environment['HOME'] == str(plan.path/'home')
                assert (plan.path/'sdk-run.private.json').exists()
                hit('construct')
            def start(self):
                hit('start-no-owner')
                self.process = Process()
                self.profile.mkdir()
                hit('start')
            def chrome(self, source, args=None, asynchronous=False):
                if source == 'return Services.appinfo.processID;':
                    hit('browser-pid')
                    return reported_pid
                assert asynchronous is True
                if source == run.LOAD:
                    hit('load')
                    return {'version':1,'state':'loaded','phase':'identity','terms':[],'complete':True}
                assert source == run.CONTROL
                hit(args[0])
                if args == ['absent']: return None if existing_addon else {'state':'absent'}
                if args == ['disable']: return {'state':'disabled'}
                assert args == ['info']
                return {'state':'active','id':run.ADDON,'version':'0.0.1','temporary':True,'privateAllowed':False,'persistentBackground':False}
            def close(self):
                hit('close')
                self.closed = True
        class Lease:
            def __init__(self, launcher, candidate, image):
                self.launcher, self.candidate = launcher, candidate
                self.handles = []; self.released = self.acquired = self.dirty = False
            def acquire(self):
                assert owner.parent_lease is self
                hit('parent-acquire')
                if lease_snapshot_failure:
                    self.dirty = True
                    raise RuntimeError('snapshot inverse refused')
                if self.candidate != self.launcher.pid and not child_topology: raise RuntimeError('not an owned child')
                self.handles = [1] if self.candidate == self.launcher.pid else [1,2]
                self.acquired = True
            def observe(self):
                hit('parent-wait')
                return owner.browser.closed and not parent_stays_live
            def release(self):
                hit('parent-release'); self.released = True
            def cleanup_complete(self): return not self.dirty and (not self.handles or self.released)
            def receipt(self):
                if not self.released or not self.acquired: raise RuntimeError('parent not retired')
                return {'qualification':False,'parent_pid':self.candidate,
                        'topology':'direct-child' if child_topology else 'same-process'}
        class Observer:
            def __init__(self, browser, nonce):
                self.removal_attempted = False
                self.removed = self.retired = False
                self.evidence = self
            def install(self): hit('install')
            def snapshot(self):
                hit('snapshot')
                self.retired = terminal and counts['disable'] > 0
                return 2 if self.retired else 1
            def require_retired(self):
                if not self.retired: raise RuntimeError('no native receipt')
                return 42
            def require_removed(self):
                self.require_retired()
                if not self.removed: raise RuntimeError('no removal')
                return {'qualification':False, 'pid':42, 'sdk_retirement_observed':True} if receipt else None
            def remove(self):
                if self.removal_attempted: return self.removed
                self.removal_attempted = True
                hit('remove')
                self.removed = True
                return True
        owner = run.ParentRun(plan, expected, 'd'*64, root/'non-executable-browser', 'e'*64, root/'report.json', parent_stdio_experiment=True)
        def unchanged(browser, before, *, fileless_experiment):
            assert fileless_experiment is True and before == {'modeled':True}
            hit('policy')
            if replacement: browser.process = Process()
        def publish(path, value):
            hit('report')
            assert owner.browser.closed and owner.process.waits > 0 and owner.observer.retired and owner.observer.removed
            reports.append(value)
        clock = iter(range(0, 10000, 21))
        replacements = {'ARTIFACTS':root,'preflight':lambda: hit('preflight'),
            'inspect':lambda *a, **k: (hit('input') or {'qualification':False,'source_commit':expected.commit}),
            'file_sha256':lambda p: 'e'*64,'new_report':lambda p:p,'Firefox':Browser,'ObserverClient':Observer,'ProcessLease':Lease,
            'policy':lambda value, **kw:value,'unchanged_policy':unchanged,'write_report':publish}
        for name, value in replacements.items(): stack.enter_context(patch.object(run, name, value))
        stack.enter_context(patch.object(run.time, 'monotonic', lambda: next(clock)))
        yield SimpleNamespace(owner=owner,plan=plan,expected=expected,root=root,events=events,counts=counts,reports=reports)


class ParentRunTests(unittest.TestCase):
    def test_nominal_orders_exchange_disable_retirement_removal_and_outer_join(self):
        with fixture() as f:
            f.owner.execute()
            self.assertEqual(f.counts['preflight'],3)
            self.assertEqual(f.counts['input'],4)
            self.assertEqual(f.counts['disable'],1)
            self.assertLess(f.events.index('snapshot'),f.events.index('disable'))
            self.assertLess(f.events.index('remove'),f.events.index('close'))
            self.assertLess(f.events.index('wait'),f.events.index('report'))
            self.assertTrue(f.owner.cleanup_complete())
            self.assertFalse(f.reports[0]['qualification'])
            self.assertFalse(f.reports[0]['browser_shutdown_with_active_native_qualified'])

    def test_separate_parent_is_retained_without_replacing_launcher(self):
        with fixture(reported_pid=1710, child_topology=True) as f:
            f.owner.execute()
            self.assertEqual(f.owner.process.pid,1709)
            self.assertEqual(f.owner.browser_pid,1710)
            self.assertTrue(f.owner.parent_lease.released)
            self.assertEqual(f.reports[0]['browser_parent_owner']['topology'],'direct-child')
            self.assertLess(f.events.index('parent-acquire'),f.events.index('load'))
            self.assertLess(f.events.index('parent-release'),f.events.index('report'))

    def test_active_parent_lease_prevents_success_after_launcher_close(self):
        with fixture(parent_stays_live=True) as f:
            with self.assertRaises(RuntimeError): f.owner.execute()
            self.assertTrue(f.owner.browser_joined)
            self.assertFalse(f.owner.cleanup_complete())
            self.assertFalse(f.owner.parent_lease.released)
            self.assertFalse(f.reports)

    def test_parent_acquisition_refusal_precedes_native_loading(self):
        with fixture(fault=('parent-acquire',1)) as f:
            with self.assertRaises(RuntimeError):f.owner.execute()
            self.assertNotIn('load',f.events)
            self.assertFalse(f.owner.load_attempted)
            self.assertIsNotNone(f.owner.parent_lease)

    def test_early_control_failure_still_releases_parent_handles(self):
        with fixture(fault=('policy',1)) as f:
            with self.assertRaises(RuntimeError):f.owner.execute()
            self.assertTrue(f.owner.parent_lease.acquired)
            self.assertTrue(f.owner.parent_lease.released)
            self.assertTrue(f.owner.cleanup_complete())
            self.assertFalse(f.reports)

    def test_failed_snapshot_inverse_remains_owned_without_process_handles(self):
        with fixture(lease_snapshot_failure=True) as f:
            with self.assertRaises(RuntimeError):f.owner.execute()
            self.assertTrue(f.owner.browser_joined)
            self.assertFalse(f.owner.parent_lease.handles)
            self.assertFalse(f.owner.cleanup_complete())
            self.assertFalse(f.reports)

    def test_default_off_exact_mode_and_retained_domain(self):
        with fixture() as f:
            args = (f.plan,f.expected,'d'*64,f.root/'none','e'*64,f.root/'report.json')
            for flag in (False,None,1,'true',[]):
                with self.subTest(flag=flag), self.assertRaises(RuntimeError): run.ParentRun(*args,parent_stdio_experiment=flag)
            f.plan.created = False
            with self.assertRaises(RuntimeError): run.ParentRun(*args,parent_stdio_experiment=True)
            self.assertFalse(f.events)

    def test_all_failure_checkpoints_refuse_report_and_keep_browser_owner(self):
        for phase in ('preflight','input','construct','start','parent-acquire','install','load','info','snapshot','disable','remove','policy','close','parent-wait','parent-release','report'):
            with self.subTest(phase=phase), fixture(fault=(phase,1)) as f:
                with self.assertRaisesRegex(RuntimeError,'retain controller'): f.owner.execute()
                self.assertFalse(f.reports)
                self.assertTrue((f.plan.path/'sdk-failure.private.json').exists())
                if f.owner.browser is not None:
                    self.assertIsNotNone(f.owner.browser)
                    self.assertEqual(f.counts['close'],1)
                self.assertLessEqual(f.counts['load'],1)
                self.assertLessEqual(f.counts['disable'],1)
                self.assertLessEqual(f.counts['remove'],1)

    def test_uncertain_load_and_disable_are_recorded_before_delivery(self):
        for phase in ('load','disable'):
            with self.subTest(phase=phase), fixture(fault=(phase,1)) as f:
                with self.assertRaises(RuntimeError): f.owner.execute()
                self.assertTrue(f.owner.load_attempted)
                self.assertTrue(f.owner.disable_attempted)
                self.assertEqual(f.counts['disable'],1)
                self.assertEqual(f.counts['load'],1)
                self.assertFalse(f.reports)

    def test_missing_native_receipt_never_becomes_joined_from_outer_exit(self):
        with fixture(terminal=False) as f:
            with self.assertRaises(RuntimeError): f.owner.execute()
            self.assertTrue(f.owner.browser_joined)
            self.assertFalse(f.owner.cleanup_complete())
            evidence = json.loads((f.plan.path/'sdk-failure.private.json').read_text(encoding='utf-8'))
            self.assertEqual(evidence['native_retirement'],'unknown')
            self.assertFalse(evidence['cleanup_complete'])
            self.assertFalse(f.reports)

    def test_changed_domain_witness_refuses_before_writing_claim(self):
        with fixture() as f:
            other = f.root/'other-owned-model-directory'
            other.mkdir()
            f.plan.path = other
            with self.assertRaises(RuntimeError): f.owner.execute()
            self.assertEqual(list(other.iterdir()),[])
            self.assertFalse(f.events)

    def test_retired_unstarted_controller_cannot_launch(self):
        with fixture() as f:
            self.assertTrue(f.owner.cleanup())
            with self.assertRaises(RuntimeError): f.owner.execute()
            self.assertEqual(f.counts['construct'],0)
            self.assertFalse(f.reports)

    def test_consumed_controller_and_domain_never_relaunch(self):
        with fixture() as f:
            f.owner.execute()
            with self.assertRaises(RuntimeError): f.owner.execute()
            other = run.ParentRun(f.plan,f.expected,'d'*64,f.root/'none','e'*64,f.root/'new.json',parent_stdio_experiment=True)
            with self.assertRaises(FileExistsError): other.execute()
            self.assertEqual(f.counts['construct'],1)

    def test_missing_retained_receipt_refuses_even_when_other_controls_complete(self):
        with fixture(receipt=False) as f:
            with self.assertRaises(RuntimeError): f.owner.execute()
            self.assertTrue(f.owner.browser_joined)
            self.assertTrue(f.owner.observer_removed)
            self.assertFalse(f.owner.cleanup_errors)
            self.assertFalse(f.owner.cleanup_complete())
            self.assertFalse(f.reports)

    def test_start_without_returned_process_stays_unknown(self):
        with fixture(fault=('start-no-owner',1)) as f:
            with self.assertRaises(RuntimeError): f.owner.execute()
            self.assertIsNone(f.owner.process)
            self.assertTrue(f.owner.browser.closed)
            self.assertFalse(f.owner.cleanup_complete())
            self.assertFalse(f.owner.browser_joined)

    def test_late_preflights_and_input_checks_are_not_omitted(self):
        for fault in [('preflight',2),('preflight',3),('input',2),('input',3),('input',4)]:
            with self.subTest(fault=fault), fixture(fault=fault) as f:
                with self.assertRaises(RuntimeError): f.owner.execute()
                self.assertFalse(f.reports)
                if f.owner.browser is not None: self.assertEqual(f.counts['close'],1)

    def test_present_fixed_id_is_not_replaced_even_in_new_profile(self):
        with fixture(existing_addon=True) as f:
            with self.assertRaises(RuntimeError): f.owner.execute()
            self.assertEqual(f.counts['load'],0)
            self.assertEqual(f.counts['disable'],0)
            self.assertTrue(f.owner.browser_joined)
            self.assertFalse(f.reports)

    def test_existing_profile_is_not_adopted(self):
        with fixture() as f:
            (f.plan.path/'Firefox').mkdir()
            with self.assertRaises(RuntimeError): f.owner.execute()
            self.assertEqual(f.counts['construct'],0)

    def test_interruption_propagates_after_cleanup_including_interrupted_cleanup(self):
        for fault, interruption in ((None,('load',1)),(('load',1),('disable',1)),(('info',1),('remove',1))):
            with self.subTest(interruption=interruption), fixture(fault=fault,interrupt=interruption) as f:
                with self.assertRaises(KeyboardInterrupt): f.owner.execute()
                self.assertEqual(f.counts['close'],1)
                self.assertFalse(f.reports)
                self.assertTrue((f.plan.path/'sdk-failure.private.json').exists())

    def test_replaced_browser_process_is_not_adopted_or_closed(self):
        with fixture(replacement=True) as f:
            with self.assertRaises(RuntimeError): f.owner.execute()
            self.assertIsNot(f.owner.process, f.owner.browser.process)
            self.assertEqual(f.counts['close'],0)
            self.assertFalse(f.owner.cleanup_complete())
            self.assertFalse(f.reports)

    def test_chrome_pid_must_match_retained_process_not_merely_profile(self):
        for pid in (1710,True,None,'1709'):
            with self.subTest(pid=pid), fixture(reported_pid=pid) as f:
                with self.assertRaises(RuntimeError): f.owner.execute()
                self.assertEqual(f.counts['load'],0)
                self.assertTrue(f.owner.browser_joined)
                self.assertFalse(f.reports)

    def test_nonzero_or_boolean_exit_is_not_success(self):
        for code in (1,False,None):
            with self.subTest(code=code), fixture(exit_code=code) as f:
                with self.assertRaises(RuntimeError): f.owner.execute()
                self.assertFalse(f.reports)

    def test_typed_control_receipts(self):
        base = {'state':'active','id':run.ADDON,'version':'0.0.1','temporary':True,'privateAllowed':False,'persistentBackground':False}
        run.active(base)
        for key in ('temporary','privateAllowed','persistentBackground'):
            with self.assertRaises(RuntimeError): run.active({**base,key:int(base[key])})
        for value in ({'state':'absent'},{'state':'disabled','extra':True},True,None):
            with self.assertRaises(RuntimeError): run.disabled(value)
        run.disabled({'state':'absent'},allow_absent=True)


if __name__ == '__main__': unittest.main()
