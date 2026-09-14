"""Supervisor ownership models; no compiler, browser, registry or process launch."""
import unittest
from qualification.parent_supervisor import ParentSupervisor, serve
from unittest.mock import patch


class Owner:
    def __init__(self, settled=True, error=None):
        self.settled, self.error = settled, error
        self.cleanups = 0
    def cleanup(self):
        self.cleanups += 1
        if self.error is not None:
            raise self.error
        return self.settled
    def cleanup_complete(self):
        return self.settled


class SupervisorTests(unittest.TestCase):
    def test_owners_retained_before_actions_and_reverse_cleanup(self):
        supervisor=ParentSupervisor(); compiler=Owner(); sdk=Owner(); order=[]
        for name, owner in [('compiler',compiler),('sdk',sdk)]:
            original=owner.cleanup
            def cleanup(name=name,original=original):
                order.append(name);return original()
            owner.cleanup=cleanup
        def action(s):
            self.assertIs(s.retain('compiler',compiler),compiler)
            self.assertIs(s.owners[0][1],compiler)
            self.assertIs(s.retain('sdk',sdk),sdk)
        status=supervisor.execute(action)
        self.assertFalse(status['held']);self.assertFalse(status['qualification'])
        self.assertEqual(order,['sdk','compiler']);self.assertEqual(supervisor.finish(),0)
        supervisor.cleanup();self.assertEqual(order,['sdk','compiler'])

    def test_unknown_lifetime_prevents_exit_even_after_action_exception(self):
        supervisor=ParentSupervisor(); owner=Owner(False)
        def action(s):
            s.retain('sdk',owner)
            raise RuntimeError('model')
        status=supervisor.execute(action)
        self.assertTrue(status['held']);self.assertTrue(status['failed'])
        self.assertIsNone(supervisor.exit_code())
        with self.assertRaises(RuntimeError):supervisor.finish()
        self.assertIs(supervisor.owners[0][1],owner)
        owner.settled=True
        self.assertEqual(supervisor.finish(),1)  # Retirement never repairs acceptance.

    def test_cleanup_failure_does_not_skip_other_owners(self):
        supervisor=ParentSupervisor(); first=Owner(); last=Owner(False,RuntimeError('model'))
        def action(s):s.retain('compiler',first);s.retain('sdk',last)
        supervisor.execute(action)
        self.assertEqual((first.cleanups,last.cleanups),(1,1))
        self.assertIsNone(supervisor.exit_code())

    def test_cancel_is_delayed_while_held_and_propagated_after_join(self):
        supervisor=ParentSupervisor(); owner=Owner(False)
        def action(s):s.retain('sdk',owner);raise KeyboardInterrupt()
        status=supervisor.execute(action)
        self.assertTrue(status['cancelled']);self.assertTrue(status['held'])
        with self.assertRaises(RuntimeError):supervisor.finish()
        owner.settled=True
        with self.assertRaises(KeyboardInterrupt):supervisor.finish()
        self.assertEqual(supervisor.exit_code(),1)

    def test_uncertain_cleanup_is_sticky_even_without_action_error(self):
        supervisor=ParentSupervisor(); owner=Owner(False)
        supervisor.execute(lambda s:s.retain('compiler',owner))
        self.assertIsNone(supervisor.exit_code());owner.settled=True
        self.assertEqual(supervisor.exit_code(),1)

    def test_cleanup_and_settlement_failures_are_independently_sticky(self):
        for cleanup,settled in ((False,True),(True,False)):
            supervisor=ParentSupervisor();owner=Owner(settled)
            owner.cleanup=lambda:cleanup
            result=supervisor.execute(lambda s:s.retain('sdk',owner))
            self.assertTrue(result['failed'])
            owner.settled=True
            self.assertEqual(supervisor.finish(),1)

    def test_closed_supervisor_never_relaunches_or_adopts_an_owner(self):
        supervisor=ParentSupervisor();supervisor.cleanup()
        with self.assertRaises(RuntimeError):supervisor.execute(lambda _:None)
        with self.assertRaises(RuntimeError):supervisor.retain('sdk',Owner())
        self.assertEqual(supervisor.exit_code(),1)
        supervisor=ParentSupervisor();owner=Owner();supervisor.retain('sdk',owner)
        with self.assertRaises(RuntimeError):supervisor.retain('sdk',Owner())
        self.assertEqual(supervisor.owners,[('sdk',owner)])
        with self.assertRaises(RuntimeError):supervisor.retain('other',Owner())

    def test_settlement_must_be_exact_true_and_observation_failure_holds(self):
        for value in (None,1,'true'):
            supervisor=ParentSupervisor();owner=Owner(value)
            supervisor.execute(lambda s:s.retain('sdk',owner))
            self.assertIsNone(supervisor.exit_code())
        supervisor=ParentSupervisor();owner=Owner()
        def observe():raise RuntimeError('model')
        owner.cleanup_complete=observe
        self.assertTrue(supervisor.execute(lambda s:s.retain('sdk',owner))['held'])
        self.assertIsNone(supervisor.exit_code())

    def test_control_channel_keeps_failed_unresolved_owner_until_retired(self):
        supervisor=ParentSupervisor();owner=Owner(False);responses=[];commands=iter(['start','finish','status','finish'])
        def write(status):
            responses.append(status)
            if len(responses)==3:owner.settled=True
        code=serve(supervisor,lambda s:s.retain('sdk',owner),lambda:next(commands),write,lambda _:self.fail('unexpected disconnect'))
        self.assertEqual(code,1);self.assertTrue(all(r['held'] for r in responses))
        self.assertIs(supervisor.owners[0][1],owner)
        self.assertEqual(owner.cleanups,1)

    def test_eof_and_failed_warning_cannot_drop_an_unresolved_owner(self):
        supervisor=ParentSupervisor();owner=Owner(False);commands=iter(['start',None]);sleeps=[]
        def notice(_):raise RuntimeError('notice failed')
        def sleep(seconds):
            sleeps.append(seconds)
            self.assertIs(supervisor.owners[0][1],owner)
            self.assertIsNone(supervisor.exit_code())
            owner.settled=True  # Model the retained owner finally observing retirement.
        with patch('qualification.parent_supervisor.time.sleep',side_effect=sleep):
            code=serve(supervisor,lambda s:s.retain('sdk',owner),lambda:next(commands),lambda _:None,notice)
        self.assertEqual(sleeps,[1]);self.assertEqual(code,1)

    def test_disconnect_before_start_and_duplicate_start_do_not_execute_again(self):
        supervisor=ParentSupervisor();calls=[]
        self.assertEqual(serve(supervisor,lambda _:calls.append(True),lambda:None,lambda _:None,lambda _:None),1)
        self.assertFalse(calls)
        supervisor=ParentSupervisor();commands=iter(['start','start'])
        self.assertEqual(serve(supervisor,lambda _:calls.append(True),lambda:next(commands),lambda _:None,lambda _:None),1)
        self.assertEqual(calls,[True])

if __name__=='__main__':unittest.main()
