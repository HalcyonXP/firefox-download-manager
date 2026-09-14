"""Fileless retained-control models; no browser, setup, GUI or process launch."""
import json
import unittest
from unittest.mock import Mock,patch
from qualification.parent_retirement_control import RetirementControl,request,serve


def command(sequence,name):
    return (json.dumps({'version':1,'sequence':sequence,'command':name})+'\n').encode('ascii')


class Owner:
    def __init__(self,held=True):
        self.held=held;self.executions=0;self.cleanups=0;self.continuations=0
        self.on_execute=lambda:None;self.on_continue=lambda:None
    def execute(self): self.executions+=1;self.on_execute()
    def cleanup(self): self.cleanups+=1;return not self.held
    def cleanup_complete(self): return not self.held
    def continue_retirement(self):
        self.continuations+=1;self.on_continue();return not self.held


def control(owner=None):
    owner=Owner() if owner is None else owner;outputs=[]
    def write(raw): outputs.append(json.loads(raw));return True
    return RetirementControl(owner,write),owner,outputs


class RetirementControlTests(unittest.TestCase):
    def test_failed_hold_keeps_control_usable_without_restarting(self):
        c,o,out=control();o.on_continue=lambda:setattr(o,'held',False)
        commands=iter([command(1,'start'),command(2,'status'),command(3,'continue'),command(4,'finish')])
        self.assertEqual(serve(c,lambda:next(commands),lambda:self.fail('unexpected broken channel')),1)
        self.assertEqual([x['status']['held'] for x in out],[True,True,False,False])
        self.assertTrue(all(x['status']['failed'] for x in out))
        self.assertEqual((o.executions,o.cleanups,o.continuations),(1,1,1));self.assertIs(c.owner,o)
        with self.assertRaises(RuntimeError):c.handle(command(5,'start'))
        self.assertEqual(o.executions,1)

    def test_real_failed_installed_cleanup_continues_original_setup(self):
        from test_parent_retirement import run
        r=run();process=r.process;owner=r.owner
        r.close_resources.side_effect=[RuntimeError('first read gap'),RuntimeError('second read gap'),None]
        r.execute=Mock(side_effect=r.failure_cleanup)
        c,_,out=control(r)
        c.handle(command(1,'start'))
        self.assertTrue(out[-1]['status']['held']);owner.retire.assert_not_called()
        c.handle(command(2,'continue'))
        self.assertFalse(out[-1]['status']['held']);self.assertTrue(out[-1]['status']['failed'])
        self.assertIs(r.process,process);self.assertIs(r.owner,owner)
        r.execute.assert_called_once();r.uninstall.assert_called_once();owner.retire.assert_called_once()
        c.handle(command(3,'finish'));self.assertEqual(c.finish(),1)

    def test_continuation_requires_failed_started_cleanup_hold(self):
        for prepared in (False,True):
            c,o,_=control(Owner(False))
            if prepared:c.handle(command(1,'start'))
            with self.assertRaises(RuntimeError):c.handle(command(2 if prepared else 1,'continue'))
            self.assertEqual(o.continuations,0)
        c,o,_=control();c.handle(command(1,'start'));c.handle(command(2,'continue'))
        self.assertEqual(o.continuations,1);self.assertIsNone(c.exit_code())

    def test_finish_before_dispatch_cleans_up_without_starting(self):
        c,o,out=control(Owner(False))
        try: c.handle(command(1,'finish'))
        except BaseException as error: self.fail('initial Finish did not retire its owner: '+type(error).__name__)
        self.assertEqual((o.executions,o.cleanups,o.continuations),(0,1,0))
        self.assertFalse(out[-1]['status']['started']);self.assertTrue(out[-1]['status']['failed'])
        self.assertTrue(out[-1]['status']['cleanup_attempted']);self.assertFalse(out[-1]['status']['held'])
        self.assertEqual(c.finish(),1)
        with self.assertRaises(RuntimeError):c.handle(command(2,'start'))
        self.assertEqual(o.executions,0)

    def test_undispatched_finish_cannot_discard_a_held_owner(self):
        c,o,out=control()
        with self.assertRaises(RuntimeError):c.handle(command(1,'finish'))
        self.assertEqual((o.executions,o.cleanups),(0,1));self.assertIsNone(c.exit_code())
        self.assertFalse(out)

    def test_finish_requires_retirement_and_is_not_replayed(self):
        c,o,_=control();c.handle(command(1,'start'))
        with self.assertRaises(RuntimeError):c.handle(command(2,'finish'))
        self.assertIsNone(c.exit_code())
        c,o,_=control(Owner(False));c.handle(command(1,'start'));c.handle(command(2,'finish'))
        self.assertEqual(c.finish(),0)
        with self.assertRaises(RuntimeError):c.handle(command(3,'finish'))

    def test_closed_typed_requests_duplicate_keys_and_sequence_replay(self):
        good=command(1,'start');self.assertEqual(request(good)['sequence'],1)
        bad=(good.rstrip(),good.replace(b'"sequence": 1',b'"sequence": true'),
             good.replace(b'"version": 1',b'"version": 1, "version": 1'),
             good.replace(b'"start"',b'"restart"'),b'{"version":NaN}\n',b'x'*257+b'\n')
        for raw in bad:
            c,o,_=control()
            with self.assertRaises((RuntimeError,ValueError)):c.handle(raw)
            self.assertTrue(c.channel_failed);self.assertTrue(c.supervisor.failed)
            with self.assertRaises(RuntimeError):c.handle(good)
            self.assertEqual(o.executions,0)
        c,o,_=control();c.handle(good)
        with self.assertRaises(RuntimeError):c.handle(good)
        self.assertEqual(o.executions,1)

    def test_eof_or_bad_output_cannot_drop_held_owner_or_repeat_cleanup(self):
        for fail_output in (False,True):
            c,o,_=control();commands=iter([command(1,'start'),b'']);notices=[]
            if fail_output:c.write=Mock(side_effect=OSError('modeled sink failure'))
            def notice():notices.append(True);raise RuntimeError('modeled notice failure')
            def settle(_):
                self.assertIs(c.owner,o);self.assertIsNone(c.exit_code());o.held=False
            with patch('qualification.parent_retirement_control.time.sleep',side_effect=settle) as sleep:
                self.assertEqual(serve(c,lambda:next(commands),notice),1)
            self.assertEqual(o.cleanups,1);self.assertEqual(o.continuations,0);self.assertEqual(notices,[True])
            sleep.assert_called_once_with(1)
            if fail_output:c.write.assert_called_once()

    def test_cancellation_during_continuation_is_retained_until_join(self):
        c,o,out=control();cancelled=KeyboardInterrupt();c.handle(command(1,'start'))
        o.on_continue=Mock(side_effect=cancelled);c.handle(command(2,'continue'))
        self.assertTrue(out[-1]['status']['cancelled']);self.assertIsNone(c.exit_code())
        o.on_continue=lambda:setattr(o,'held',False);c.handle(command(3,'continue'));c.handle(command(4,'finish'))
        with self.assertRaises(KeyboardInterrupt) as caught:c.finish()
        self.assertIs(caught.exception,cancelled);self.assertEqual(c.exit_code(),1)

    def test_owner_replacement_or_reinitialization_is_not_adopted(self):
        c,o,_=control();other=Owner(False)
        with self.assertRaises(RuntimeError):c.__init__(other,lambda _:True)
        with self.assertRaises(AttributeError):c.owner=other
        self.assertIs(c.owner,o)
        c.supervisor.owners=[('sdk',other)]
        with self.assertRaises(RuntimeError):c.handle(command(1,'start'))
        c.fail(RuntimeError('changed ownership'))
        self.assertTrue(c.quarantined);self.assertIsNone(c.exit_code())
        self.assertEqual((o.executions,o.cleanups,other.executions,other.cleanups),(0,0,0,0))

    def test_foreign_thread_object_cannot_dispatch_or_clean_up(self):
        c,o,_=control()
        with patch('qualification.parent_retirement_control.threading.current_thread',return_value=object()):
            with self.assertRaises(RuntimeError):c.handle(command(1,'start'))
            c.fail(RuntimeError('foreign thread'))
        self.assertEqual((o.executions,o.cleanups),(0,0));self.assertTrue(c.supervisor.failed)
        self.assertTrue(c.quarantined);self.assertIsNone(c.exit_code())

    def test_synchronous_progress_is_bounded_and_precedes_terminal_status(self):
        c,o,out=control(Owner(False));o.on_execute=lambda:(c.progress('preflight'),c.progress('manual-download'))
        c.handle(command(1,'start'))
        self.assertEqual([x['kind'] for x in out],['phase','phase','status'])
        self.assertTrue(all(x['sequence']==1 for x in out))
        with self.assertRaises(RuntimeError):c.progress('uninstall')
        c,o,out=control(Owner(False));o.on_execute=lambda:[c.progress('setup') for _ in range(33)]
        c.handle(command(1,'start'))
        self.assertEqual(len(out),33);self.assertTrue(out[-1]['status']['failed'])

    def test_total_phase_budget_reserves_every_terminal_response(self):
        c,o,out=control();burst=lambda:[c.progress('setup') for _ in range(32)]
        o.on_execute=o.on_continue=burst
        c.handle(command(1,'start'))
        for sequence in range(2,33):c.handle(command(sequence,'continue'))
        self.assertEqual(sum(x['kind']=='phase' for x in out),96)
        self.assertEqual(sum(x['kind']=='status' for x in out),32)
        self.assertEqual(len(out),128);self.assertTrue(out[-1]['status']['held'])

    def test_uncertain_progress_output_does_not_attempt_another_write(self):
        c,o,_=control(Owner(False));c.write=Mock(side_effect=OSError('modeled uncertain phase write'))
        o.on_execute=lambda:c.progress('setup')
        with self.assertRaises((RuntimeError,OSError)):c.handle(command(1,'start'))
        self.assertTrue(c.output_failed);c.write.assert_called_once();self.assertEqual(o.cleanups,1)
        self.assertEqual(c.exit_code(),1)

    def test_reentrant_sink_refuses_without_recursive_active_owner_cleanup(self):
        c,o,_=control(Owner(False));active=[]
        def write(_):
            with self.assertRaises(RuntimeError):c.handle(command(2,'status'))
            self.assertEqual(o.cleanups,0 if active else 1)
            return True
        c.write=write
        def execute():
            active.append(True);c.progress('setup');active.clear()
        o.on_execute=execute;c.handle(command(1,'start'))
        self.assertTrue(c.supervisor.failed);self.assertEqual(o.executions,1);self.assertEqual(o.cleanups,1)

    def test_finish_rechecks_current_owner_after_terminal_sink(self):
        c,o,_=control(Owner(False));c.handle(command(1,'start'))
        c.write=lambda _:(setattr(o,'held',True) or True)
        c.handle(command(2,'finish'))
        with self.assertRaises(RuntimeError):c.finish()
        self.assertIsNone(c.exit_code())

if __name__=='__main__':unittest.main()
