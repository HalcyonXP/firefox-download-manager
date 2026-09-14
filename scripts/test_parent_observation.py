"""Observer/receipt/controller models; no browser, native process or profile access."""
import copy
import json
import unittest
from types import SimpleNamespace

from qualification.parent_observation import Observation, ObserverClient

NONCE = '11111111-1111-4111-8111-111111111111'
COLLECTOR = '22222222-2222-4222-8222-222222222222'
OTHER = '33333333-3333-4333-8333-333333333333'


def records(pid=42):
    ready = {'nonce':NONCE, 'kind':'ready', 'value':{'version':1,'qualification':False,
             'scope':'owned-parent-stdio-v1','stage':'echoed','pid':pid}}
    retired = {'nonce':NONCE,'kind':'retired','value':{**ready['value'],'stage':'retired',
               'attempted':True,'echoed':True,'successful':True,'launcher':{'spawn_called':True,
               'hooks_removed':True,'successful':True,'transport':{'startup':'started',
               'process_waited':True,'exit_code':0,'pipes_closed':True,'io_settled':True,
               'forced':False,'successful':True}}}}
    return [ready, retired]


def snapshot(values=None, **changes):
    return {'version':1,'qualification':False,'collector':COLLECTOR,'state':'active','removed':False,
            'failed':False,'records':[json.dumps(value) for value in (values or [])],**changes}


class ParentObservationTests(unittest.TestCase):
    def test_exchange_retirement_and_observer_removal_are_distinct(self):
        value = Observation(NONCE, COLLECTOR)
        self.assertEqual(value.accept(snapshot()), 0)
        self.assertEqual(value.accept(snapshot(records()[:1])), 1)
        with self.assertRaises(RuntimeError): value.require_retired()
        self.assertEqual(value.accept(snapshot(records())), 2)
        self.assertEqual(value.require_retired(), 42)
        with self.assertRaises(RuntimeError): value.require_removed()
        final = snapshot(records(), state='closed', removed=True)
        self.assertEqual(value.accept(final), 2)
        self.assertEqual(value.require_removed()['pid'], 42)
        self.assertIs(value.require_removed()['qualification'], False)
        value.accept(final)  # Identical polling is not another notification.
        final['records'].clear()
        self.assertEqual(value.require_removed()['pid'], 42)  # No mutable caller records retained.

    def test_prefix_rewrite_truncation_reorder_duplicates_and_pid_change_refuse(self):
        ready, retired = records()
        altered = copy.deepcopy(retired); altered['value']['pid'] = 43
        cases = [[], [retired], [ready, ready], [ready, altered], [ready, retired, retired]]
        for case in cases:
            value = Observation(NONCE, COLLECTOR); value.accept(snapshot([ready]))
            with self.subTest(case=case), self.assertRaises(RuntimeError): value.accept(snapshot(case))
            with self.assertRaises(RuntimeError): value.accept(snapshot(records(), state='closed', removed=True))
        value = Observation(NONCE, COLLECTOR); value.accept(snapshot([ready]))
        changed = snapshot([ready]); changed['records'][0] = json.dumps(ready, sort_keys=True)
        with self.assertRaises(RuntimeError): value.accept(changed)

    def test_closed_snapshot_booleans_and_collector_identity(self):
        for key, other in [('version',True),('qualification',0),('collector',OTHER),('state','uncertain'),
                           ('failed',0),('failed',True),('removed',1),('records',{}),('extra',False)]:
            with self.subTest(key=key), self.assertRaises(RuntimeError):
                Observation(NONCE,COLLECTOR).accept(snapshot(**{key:other}))
        value = Observation(NONCE,COLLECTOR); value.accept(snapshot(state='closed',removed=True))
        with self.assertRaises(RuntimeError): value.accept(snapshot())
        with self.assertRaises(RuntimeError): Observation(NONCE,COLLECTOR).accept(snapshot(removed=True))

    def test_strict_json_and_pid_types_never_become_a_process_handle(self):
        raw = json.dumps(records()[0])
        for text in ['x'*8193, 'π', '{"nonce":"'+NONCE+'","nonce":"'+NONCE+'"}', raw.replace('42','NaN'), '[]']:
            with self.assertRaises(RuntimeError): Observation(NONCE,COLLECTOR).accept(snapshot(records=[text]))
        for pid in [None,True,0,-1,1.5,0x100000000]:
            with self.subTest(pid=pid), self.assertRaises(RuntimeError): Observation(NONCE,COLLECTOR).accept(snapshot(records(pid)))
        foreign = records(); foreign[0]['nonce'] = OTHER
        with self.assertRaises(RuntimeError): Observation(NONCE,COLLECTOR).accept(snapshot(foreign))

    def test_master_success_cannot_substitute_for_nested_retirement_observations(self):
        paths = [(['attempted'],False),(['echoed'],False),(['successful'],False),(['pid'],43),
                 (['launcher','spawn_called'],False),(['launcher','hooks_removed'],False),
                 (['launcher','successful'],False),(['launcher','transport','startup'],'indeterminate'),
                 (['launcher','transport','process_waited'],False),(['launcher','transport','pipes_closed'],False),
                 (['launcher','transport','io_settled'],False),(['launcher','transport','forced'],True),
                 (['launcher','transport','exit_code'],False),(['launcher','transport','exit_code'],1),
                 (['launcher','transport','successful'],False),(['launcher','transport','extra'],0)]
        for path, other in paths:
            values = records(); cursor = values[1]['value']
            for key in path[:-1]: cursor = cursor[key]
            cursor[path[-1]] = other
            with self.subTest(path=path), self.assertRaises(RuntimeError):
                Observation(NONCE,COLLECTOR).accept(snapshot(values,state='closed',removed=True))

    def test_commands_keep_one_sandbox_restore_context_and_never_reinstall(self):
        browser = self.browser()
        client = ObserverClient(browser,NONCE); client.install()
        browser.values = records(); self.assertEqual(client.snapshot(),2)
        self.assertTrue(client.remove()); self.assertTrue(client.remove())
        result = client.evidence.require_removed(); self.assertEqual(result['pid'],42)
        with self.assertRaises(RuntimeError): client.install()
        with self.assertRaises(RuntimeError): client.snapshot()
        scripts = [args for name,args in browser.calls if name=='WebDriver:ExecuteAsyncScript']
        self.assertEqual([args['args'][0] for args in scripts],['install','snapshot','remove'])
        self.assertEqual({args['sandbox'] for args in scripts},{client.sandbox})
        self.assertTrue(all(args['newSandbox'] is False for args in scripts))
        self.assertTrue(all(args['args'][1:]==[NONCE,client.collector] for args in scripts))
        self.assertEqual([args['value'] for name,args in browser.calls if name=='Marionette:SetContext'],['chrome','content']*3)

    def test_uncertain_command_delivery_retains_cleanup_but_cannot_recover_success(self):
        for failed in ['install','snapshot']:
            browser = self.browser(fail=failed); client = ObserverClient(browser,NONCE)
            if failed=='snapshot': client.install()
            with self.assertRaisesRegex(RuntimeError, 'owned parent observation refused'):
                (client.install if failed=='install' else client.snapshot)()
            browser.values = records()
            self.assertFalse(client.remove())
            with self.assertRaises(RuntimeError): client.evidence.require_removed()
            self.assertEqual(sum(name=='WebDriver:ExecuteAsyncScript' and args['args'][0]=='remove' for name,args in browser.calls),1)
            self.assertEqual(browser.calls[-1],('Marionette:SetContext',{'value':'content'}))

    def test_interrupted_delivery_preserves_cancellation_and_sticky_uncertainty(self):
        browser = self.browser()
        command = browser.command
        def interrupted(name, args):
            if name == 'WebDriver:ExecuteAsyncScript' and args['args'][0] == 'install':
                raise KeyboardInterrupt()
            return command(name, args)
        browser.command = interrupted
        client = ObserverClient(browser, NONCE)
        with self.assertRaises(KeyboardInterrupt): client.install()
        browser.values = records()
        self.assertFalse(client.remove())
        with self.assertRaises(RuntimeError): client.evidence.require_removed()

    def test_failed_remove_is_memoized_and_replaced_browser_owner_is_refused(self):
        browser = self.browser(fail='remove'); client = ObserverClient(browser,NONCE); client.install()
        self.assertFalse(client.remove()); self.assertFalse(client.remove())
        self.assertEqual(sum(name=='WebDriver:ExecuteAsyncScript' and args['args'][0]=='remove' for name,args in browser.calls),1)
        browser = self.browser(); client = ObserverClient(browser,NONCE); client.install(); before=len(browser.calls)
        browser.process = object()
        self.assertFalse(client.remove()); self.assertEqual(len(browser.calls),before)

    @staticmethod
    def browser(fail=None):
        browser = SimpleNamespace(process=object(),verified=True,closed=False,calls=[],values=[])
        def command(name,args):
            browser.calls.append((name,args))
            if name=='Marionette:SetContext': return None
            operation, _, collector = args['args']
            if operation==fail: raise OSError('modeled lost automation reply')
            return {'value':snapshot(browser.values,collector=collector,
                state='closed' if operation=='remove' else 'active', removed=operation=='remove')}
        browser.command=command
        return browser


if __name__=='__main__': unittest.main()
