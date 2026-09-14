"""Fileless installed-parent failure continuation; no setup/browser execution."""
from pathlib import Path
from types import SimpleNamespace
import unittest
import tempfile
from unittest.mock import Mock, patch
from qualification import parent_installed as p
from qualification.parent_supervisor import ParentSupervisor
from qualification.browser_peer import BrowserPeer, IMAGES
from qualification.setup_owner import SetupOwner


def run():
    r=p.ParentInstalledRun(Path('unused'),Path('unused'),Path('unused'),'a'*64,Path('unused'),'b'*64,
                           parent_transport_experiment=True)
    r.run_attempted=True;r.setup_start_attempted=True;r.install_requested=True
    r.process=Mock(pid=91);r.process.poll.return_value=None;r.process.wait.return_value=0
    r.owner=SimpleNamespace(process=r.process,joined=False,quiesce=Mock(),retire=Mock())
    def retired(): r.owner.joined=True;r.process.poll.return_value=0
    r.owner.retire.side_effect=retired
    r.close_resources=Mock();r.record_diagnostic=Mock()
    def uninstalled():
        if r.uninstall_requested: raise AssertionError('replayed Uninstall')
        r.uninstall_requested=True;r.uninstalled=True
    r.uninstall=Mock(side_effect=uninstalled)
    return r


class ParentRetirementTests(unittest.TestCase):
    def test_unresolved_browser_keeps_original_setup_available_for_verified_removal(self):
        r=run();owner=r.owner;r.close_resources.side_effect=RuntimeError('modeled browser observation gap')
        try:r.failure_cleanup()
        except RuntimeError:pass
        self.assertIs(r.owner,owner)
        r.owner.quiesce.assert_called_once()
        r.uninstall.assert_not_called()
        r.owner.retire.assert_not_called() # Old cleanup prematurely discarded this UI/authority.
        self.assertFalse(r.cleanup_complete())

    def test_consumed_failure_continues_existing_cleanup_without_setup_or_uninstall_replay(self):
        r=run();r.close_resources.side_effect=RuntimeError('modeled read gap')
        with self.assertRaises(RuntimeError):r.failure_cleanup()
        r.cleanup();original=r.process;owner=r.owner
        r.close_resources.side_effect=None
        self.assertTrue(r.continue_retirement())
        self.assertIs(r.process,original);self.assertIs(r.owner,owner)
        self.assertTrue(r.continue_retirement())
        r.uninstall.assert_called_once();r.owner.retire.assert_called_once()
        r.record_diagnostic.assert_called_once_with('parent-cleanup.private.json')
        with patch.object(p.InstalledRun,'execute',side_effect=AssertionError('relaunch')) as execute:
            with self.assertRaises(RuntimeError):r.execute()
        execute.assert_not_called()

    def test_supervisor_gets_one_failure_continuation_not_a_repaired_success(self):
        r=run();r.close_resources.side_effect=[RuntimeError('modeled first read gap'),None]
        supervisor=ParentSupervisor();supervisor.retain('sdk',r)
        status=supervisor.execute(lambda _:r.failure_cleanup())
        self.assertTrue(status['failed']);self.assertFalse(status['held'])
        self.assertEqual(supervisor.finish(),1)
        r.uninstall.assert_called_once();r.owner.retire.assert_called_once()
        r.record_diagnostic.assert_called_once()
        self.assertEqual(r.close_resources.call_count,2)
        supervisor.cleanup();self.assertEqual(r.close_resources.call_count,2)

    def test_unchanged_browser_peer_survives_manager_join_until_browser_cleanup_finishes(self):
        r=run();child={'id':97};binding=object();calls=[]
        def observation():
            return ('Operation 1: complete','Owned Manager process: 97' if child['id'] else 'Manager exit observed; retained child joined.')
        def quit_manager(identity):
            self.assertEqual(identity,97);calls.append('manager-quit');child['id']=None
        def close_setup():calls.append('setup-close');r.process.poll.return_value=0
        r.owner=SetupOwner(r.process,observation,close_setup,quit_manager);r.owner.sequence=1
        def inventory():
            result={name:set() for name in IMAGES};result['download-manager-setup.exe']={91}
            if child['id']:result['download-manager-native-host.exe']={child['id']}
            return result
        peer=BrowserPeer(r.owner,binding,lambda:binding,inventory)
        attempts=[]
        def resources():
            attempts.append(1)
            if len(attempts)==1:raise RuntimeError('modeled first SDK read gap')
            peer.require_browser_closed();calls.append('browser-closed-preflight')
        r.close_resources.side_effect=resources
        supervisor=ParentSupervisor();supervisor.retain('sdk',r)
        status=supervisor.execute(lambda _:r.failure_cleanup())
        self.assertTrue(status['failed']);self.assertFalse(status['held'])
        self.assertEqual(calls,['manager-quit','browser-closed-preflight','setup-close'])
        r.uninstall.assert_called_once();r.process.wait.assert_called_once()

    def test_uncertain_uninstall_is_observed_before_retiring_original_setup(self):
        r=run()
        def uncertain(): r.uninstall_requested=True;raise RuntimeError('modeled uncertain return')
        r.uninstall.side_effect=uncertain
        with self.assertRaises(RuntimeError):r.failure_cleanup()
        r.owner.retire.assert_not_called();r.cleanup()
        r.observe_uninstalled=Mock(side_effect=lambda:setattr(r,'uninstalled',True))
        self.assertTrue(r.continue_retirement())
        r.uninstall.assert_called_once();r.observe_uninstalled.assert_called_once();r.owner.retire.assert_called_once()

    def test_uninstall_readback_requires_bound_completion_registration_and_owned_path_absence(self):
        for mode in ('valid','no-request','wrong-process','status','registration','remaining-file'):
            with self.subTest(mode=mode),tempfile.TemporaryDirectory() as directory:
                root=Path(directory).resolve();r=run();r.install=root
                r.binding=SimpleNamespace(group=root/'group',generation=root/'group'/'generation')
                r.uninstall_requested=mode!='no-request'
                r.owner._observe=Mock(return_value=('complete',None))
                r.text=Mock(return_value='pending' if mode=='status' else p.REMOVED)
                r.preflight=SimpleNamespace(all_views_absent=Mock())
                if mode=='registration':r.preflight.all_views_absent.side_effect=RuntimeError('modeled registration remains')
                if mode=='wrong-process':r.owner.process=Mock(pid=r.process.pid)
                if mode=='remaining-file':(root/'installation.json').write_text('owned metadata fixture',encoding='utf-8')
                with patch.object(p,'closed_apps') as closed:
                    if mode=='valid':
                        r.observe_uninstalled();self.assertTrue(r.uninstalled)
                        closed.assert_called_once_with(r.preflight,r.process)
                    else:
                        with self.assertRaises((RuntimeError,AssertionError)):r.observe_uninstalled()
                        self.assertFalse(r.uninstalled)
                r.uninstall.assert_not_called();r.owner.retire.assert_not_called()

    def test_new_or_successful_controller_cannot_enter_failure_continuation(self):
        r=run()
        with self.assertRaises(RuntimeError):r.continue_retirement()
        r.final_cleanup_attempted=True
        with self.assertRaises(RuntimeError):r.continue_retirement()
        r.close_resources.assert_not_called();r.owner.retire.assert_not_called()

    def test_reinitialization_cannot_discard_the_original_plan_or_process(self):
        r=run();plan=object();r.plan=plan;process=r.process;owner=r.owner
        with self.assertRaises(RuntimeError):
            r.__init__(Path('unused'),Path('unused'),Path('unused'),'a'*64,Path('unused'),'b'*64,parent_transport_experiment=True)
        self.assertIs(r.plan,plan);self.assertIs(r.process,process);self.assertIs(r.owner,owner)
        r.setup_start_attempted=False;r.owner.joined=True;r.process.poll.return_value=0
        self.assertFalse(r.cleanup_complete())

    def test_process_replacement_is_rejected_without_rebinding_an_owner(self):
        r=run();original=r.process
        with self.assertRaises(RuntimeError):r.process=Mock(pid=original.pid)
        self.assertIs(r.process,original)
        r.owner.process=Mock(pid=original.pid)
        with self.assertRaises(RuntimeError):r.failure_cleanup()
        r.owner.quiesce.assert_not_called();r.owner.retire.assert_not_called()
        r.owner.joined=True;r.process.poll.return_value=0
        self.assertFalse(r.cleanup_complete())

    def test_cancellation_survives_independent_resource_manager_and_record_failures(self):
        r=run();cancelled=KeyboardInterrupt();r.close_resources.side_effect=cancelled
        r.owner.quiesce.side_effect=RuntimeError('modeled manager read failure')
        r.record_diagnostic.side_effect=OSError('modeled sink failure')
        with self.assertRaises(KeyboardInterrupt) as caught:r.failure_cleanup()
        self.assertIs(caught.exception,cancelled)
        r.owner.quiesce.assert_called_once();r.record_diagnostic.assert_called_once()
        r.owner.retire.assert_not_called()

    def test_resource_interruption_still_attempts_independent_fixture_join(self):
        r=run();del r.close_resources;cancelled=KeyboardInterrupt()
        browser=SimpleNamespace(cleanup_complete=lambda:False,close=Mock(side_effect=cancelled))
        fixture=SimpleNamespace(closed=False)
        fixture.close=Mock(side_effect=lambda:setattr(fixture,'closed',True))
        r.browsers=[browser];r.fixtures=[fixture]
        with self.assertRaises(KeyboardInterrupt) as caught:r.close_resources()
        self.assertIs(caught.exception,cancelled);fixture.close.assert_called_once()
        self.assertEqual(r.cleanup_errors,['browser'])


if __name__=='__main__':unittest.main()
