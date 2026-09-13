"""Retained installed-parent lifetime models; no browser/setup/registration execution."""
import hashlib
import tempfile
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch
from qualification import parent_installed as p
from qualification.firefox import AutomationError
from test_firefox_policy import baseline
from qualification.parent_supervisor import ParentSupervisor


class ParentInstalledTests(unittest.TestCase):
    def browser(self):
        calls=[];b=object.__new__(p.ParentBrowser)
        b.original=b.process=Mock(pid=97);b.original.poll.return_value=0;b.original.wait.return_value=0
        b.automation_policy=baseline();b.automation_policy['preferences']['extensions.experiments.enabled'].update(value=True,user=True)
        b.failed=False;b.stage='model';b.first_failure=None;b.command_failure=None;b.launch_attempted=True;b.closed=False;b.load_attempted=True;b.disable_attempted=b.disabled_observed=b.browser_close_attempted=False
        b.control_handle='control';b.manager_handle='manager';b.control_switch_attempted=False;b.tab_attempted=True
        b.tab_failure_attempted=False;b.tab_failure=None
        b.read_tab_state=Mock(return_value={'version':1,**{key:None for key in p.TAB_FIELDS}})
        def command(name,args=None):
            if name=='WebDriver:SwitchToWindow': calls.append('control-select');return None
            if name=='WebDriver:GetWindowHandle': return {'value':'control'}
            if name=='WebDriver:GetCurrentURL': return {'value':'about:blank'}
            raise AssertionError('unexpected model command')
        b.command=Mock(side_effect=command)
        b.chrome=Mock(side_effect=lambda *a:(calls.append(a[1][0]) or {'state':'disabled'}))
        b._require_apps_closed=Mock(side_effect=lambda:calls.append('closed-apps'))
        evidence=SimpleNamespace(ready=False,records=(),failed=False)
        evidence.resource_retired=lambda:evidence.ready
        evidence.require_removed=lambda:{'qualified':False,'sdk':True}
        retirement=SimpleNamespace(records=(),failed=False,closed=False,removed=False)
        retirement.resource_retired=lambda:evidence.ready
        observer=SimpleNamespace(evidence=evidence,retirement=retirement,removal_returned=False,removal_attempted=False)
        def snapshot(): calls.append('snapshot');evidence.ready=True;return 1
        def remove():
            calls.append('observer-remove');observer.removal_attempted=True;observer.removal_returned=True
            retirement.closed=retirement.removed=True;return True
        observer.snapshot=observer.cleanup_snapshot=snapshot;observer.remove=remove
        observer.cleanup_remove=lambda: observer.removal_returned or remove()
        observer.cleanup_complete=lambda: observer.removal_returned
        b.observer=observer
        lease=SimpleNamespace(handles=[1,2],released=False,acquired=True,failed=False)
        lease.cleanup_complete=lambda:lease.released
        lease.observe=lambda:(calls.append('parent-observe') or True)
        def release(): calls.append('parent-release');lease.released=True
        lease.release=release;lease.receipt=lambda:{'parent':True};b.parent_lease=lease
        return b,calls

    def closed(self, calls):
        def close(b): calls.append('browser-close');b.closed=True
        return close

    def test_disable_sdk_pipe_observation_removal_launcher_parent_waits_before_evidence(self):
        b,calls=self.browser()
        with patch.object(p.Firefox,'close',self.closed(calls)): b.close()
        self.assertEqual(calls,['control-select','disable','snapshot','observer-remove','browser-close','parent-observe','parent-release'])
        self.assertTrue(b.cleanup_complete());self.assertEqual(b.evidence()['launcher_exit'],0)
        b.original.wait.assert_called_with(timeout=0)

    def test_uncertain_disable_is_not_replayed_while_exact_observation_can_settle(self):
        b,calls=self.browser();b.chrome.side_effect=RuntimeError('uncertain returned disable')
        with patch.object(p.Firefox,'close',self.closed(calls)):
            with self.assertRaises(RuntimeError): b.close()
            self.assertFalse(b.cleanup_complete());self.assertFalse(b.browser_close_attempted)
            b.chrome.side_effect=lambda _,args,*rest:(calls.append(args[0]) or {'state':'disabled'})
            b.close()
        self.assertNotIn('disable',calls);self.assertIn('disabled',calls);self.assertTrue(b.cleanup_complete())
        self.assertTrue(b.failed)
        with self.assertRaises(RuntimeError): b.evidence()

    def test_real_observer_readback_closes_failed_browser_without_qualifying_or_replaying(self):
        import json
        from test_parent_cleanup_observation import Browser, N, C, native
        for phase in ('snapshot','remove'):
            b,calls=self.browser();b.verified=True;window_command=b.command
            peer=Browser();failed=[False]
            def command(name,args=None):
                if name.startswith('WebDriver:') and name!='WebDriver:ExecuteAsyncScript': return window_command(name,args)
                if name=='WebDriver:ExecuteAsyncScript' and args['args'][0]==phase and not failed[0]:
                    failed[0]=True;peer.restore_failure=True
                return peer.command(name,args)
            b.command=command
            with patch('qualification.parent_observation.uuid.uuid4',return_value=C): b.observer=p.ObserverClient(b,N)
            b.observer.install();peer.record=json.dumps(native())
            def bounded(condition,*_):
                if not condition(): raise RuntimeError('modeled pending observation')
            with patch.object(p,'wait',side_effect=bounded),patch.object(p.Firefox,'close',self.closed(calls)):
                with self.assertRaises(RuntimeError): b.close()
                self.assertFalse(b.closed);self.assertFalse(b.cleanup_complete())
                b.close()
            self.assertTrue(b.failed);self.assertTrue(b.cleanup_complete())
            self.assertTrue(b.observer.evidence.failed)
            self.assertEqual(peer.operations.count('remove'),1)
            self.assertEqual(calls.count('disable'),1);self.assertEqual(calls.count('browser-close'),1)
            if phase=='remove': self.assertFalse(b.observer.removal_returned)
            with self.assertRaises(RuntimeError): b.evidence()
            observation=b.diagnostic();self.assertEqual(observation['version'],4)
            self.assertTrue(observation['observer']['cleanup']['removed'])
            self.assertTrue(observation['observer']['failed'])

    def test_missing_sdk_retirement_preserves_readable_browser_and_exact_owner(self):
        b,calls=self.browser()
        with patch.object(p,'wait',side_effect=RuntimeError('modeled observation budget')),patch.object(p.Firefox,'close') as close:
            with self.assertRaises(RuntimeError): b.close()
            close.assert_not_called()
        self.assertIs(b.process,b.original);self.assertFalse(b.browser_close_attempted);self.assertFalse(b.cleanup_complete())

    def test_same_pid_replacement_and_unknown_creation_refuse_before_effects(self):
        for mode in ('replacement','unknown'):
            b,calls=self.browser()
            if mode=='replacement': b.process=Mock(pid=b.original.pid)
            else: b.original=None
            with self.subTest(mode=mode),self.assertRaises(RuntimeError): b.close()
            self.assertEqual(calls,[]);self.assertFalse(b.cleanup_complete())

    def test_no_repeated_quit_or_uncertain_handle_close(self):
        b,calls=self.browser();b.observer.evidence.ready=True
        def failure(): calls.append('parent-release');raise RuntimeError('uncertain release')
        b.parent_lease.release=failure
        # ProcessLease itself supplies the one-attempt-per-handle inverse guard.
        with patch.object(p.Firefox,'close',self.closed(calls)):
            with self.assertRaises(RuntimeError): b.close()
        self.assertTrue(b.browser_close_attempted);self.assertFalse(b.cleanup_complete())
        b.parent_lease.handles=[]
        with patch.object(p.Firefox,'close',side_effect=AssertionError('no replay')) as repeated:
            with self.assertRaises(RuntimeError): b.close()
        repeated.assert_not_called()
        self.assertEqual(calls.count('browser-close'),1);self.assertEqual(calls.count('parent-release'),1)

    def test_cancellation_is_not_masked_by_later_parent_lease_error(self):
        b,calls=self.browser();b.parent_lease.release=Mock(side_effect=RuntimeError('lease failed'))
        with patch.object(p.Firefox,'close',side_effect=KeyboardInterrupt):
            with self.assertRaises(KeyboardInterrupt): b.close()
        b.parent_lease.release.assert_called_once()

    def test_failed_success_receipt_cannot_become_qualification_after_resource_cleanup(self):
        b,calls=self.browser();b.observer.evidence.require_removed=Mock(side_effect=RuntimeError('SDK unsuccessful'))
        with patch.object(p.Firefox,'close',self.closed(calls)): b.close()
        self.assertTrue(b.cleanup_complete())
        with self.assertRaises(RuntimeError): b.evidence()

    def test_driver_refuses_implicit_mode_and_holds_partial_owners(self):
        for flag in (False,None,1,'true'):
            with self.subTest(flag=flag),self.assertRaises(RuntimeError):
                p.ParentInstalledRun(Path('unused'),Path('unused'),Path('unused'),'a'*64,Path('unused'),'b'*64,
                                     parent_transport_experiment=flag)
        r=object.__new__(p.ParentInstalledRun);r.setup_start_attempted=True;r.process=None;r.owner=None;r.fixtures=[];r.browsers=[];r.hosts=[]
        self.assertFalse(r.cleanup_complete())
        r.process=Mock();r.process.poll.return_value=0;r.owner=SimpleNamespace(joined=True,process=r.process)
        self.assertTrue(r.cleanup_complete())
        b,_=self.browser();r.browsers=[b];self.assertFalse(r.cleanup_complete())


    def test_repeated_start_never_replaces_retained_original(self):
        b,_=self.browser();original=b.original;b.process=Mock(pid=original.pid)
        with patch.object(p.Firefox,'start',side_effect=AssertionError('must not enter start')) as start:
            with self.assertRaises(RuntimeError): b.start()
        start.assert_not_called();self.assertIs(b.original,original)

    def test_control_and_numeric_exit_require_exact_types(self):
        b,calls=self.browser();b.chrome.return_value={'state':'active','temporary':1};b.chrome.side_effect=None
        with self.assertRaises(RuntimeError): p.control(b,'info',{'state':'active','temporary':True})
        b.chrome.side_effect=lambda *a:{'state':'disabled'}
        with patch.object(p.Firefox,'close',self.closed(calls)): b.close()
        b.original.wait.return_value=False
        with self.assertRaises(RuntimeError): b.evidence()
        b.closed=1;self.assertFalse(b.cleanup_complete())

    def test_faults_select_exact_phase_and_occurrence(self):
        r=object.__new__(p.ParentInstalledRun);r.fault=('bridge-started',2);r.occurrences={}
        r.checkpoint('bridge-started');r.checkpoint('completed')
        with self.assertRaises(RuntimeError): r.checkpoint('bridge-started')
        self.assertEqual(r.occurrences,{'bridge-started':2,'completed':1})


    def test_actual_transfer_control_flow_retains_two_browsers_before_start_and_reads_output(self):
        for fault in (None,('bridge-started',2)):
            with self.subTest(fault=fault),tempfile.TemporaryDirectory() as directory:
                root=Path(directory).resolve();r=object.__new__(p.ParentInstalledRun)
                r.plan=SimpleNamespace(path=root);r.destination=root/'output';r.destination.mkdir()
                r.executable=Path('unused');r.environment={};r.browsers=[];r.fixtures=[];r.sdk_checks=[];r.checks=[]
                r.fault=fault;r.occurrences={};r.binding=object();r.current_binding=Mock()
                r.owner=Mock();r.owner._observe.return_value=(None,97);r.manager_window=object();r.ui=Mock();r.ui.tray.return_value=True
                r.inputs=Mock(return_value=(Path('metadata-only.xpi'),{'source_commit':'a'*40}))
                events=[]
                class Browser:
                    def __init__(self,*args): self.closed=False;self.index=len(r.browsers);events.append(('construct',self.index))
                    def start(self): self_test.assertIs(r.browsers[-1],self);events.append(('start',self.index))
                    def load(self,xpi): self_test.assertEqual(xpi,Path('metadata-only.xpi'));events.append(('load',self.index))
                    def script(self,code): return 1 if (r.destination/'owned-parent-installed.bin').exists() else 0
                    def fill(self,values): self_test.assertEqual(values['destination'],str(r.destination));events.append(('fill',self.index))
                    def click(self,selector): (r.destination/'owned-parent-installed.bin').write_bytes(b'abc');events.append(('click',self.index))
                    def task(self,name,state): self_test.assertEqual(state,'completed');return 'same-task'
                    def close(self): self.closed=True;events.append(('close',self.index))
                    def evidence(self): self_test.assertTrue(self.closed);events.append(('evidence',self.index));return {'model':True}
                class Fixture:
                    def __init__(self,*,owners,large_size): owners.append(self)
                    def url(self,path): return 'http://127.0.0.1/range'
                self_test=self
                with patch.object(p,'ParentBrowser',Browser),patch.object(p,'BrowserPeer',return_value=object()), \
                     patch.object(p,'Fixture',Fixture),patch.object(p,'SMALL_SIZE',3), \
                     patch.object(p,'expected_sha256',return_value=hashlib.sha256(b'abc').hexdigest()):
                    if fault is None:
                        output,size,sha,_=r.transfer(97)
                        self.assertEqual(output.read_bytes(),b'abc');self.assertEqual(size,3);self.assertEqual(sha,hashlib.sha256(b'abc').hexdigest())
                        self.assertEqual(len(r.sdk_checks),2);self.assertTrue(all(b.closed for b in r.browsers))
                        self.assertEqual(events.count(('click',0)),1);self.assertNotIn(('click',1),events)
                        self.assertLess(events.index(('evidence',0)),events.index(('construct',1)))
                    else:
                        with self.assertRaises(RuntimeError): r.transfer(97)
                        self.assertEqual(len(r.browsers),2);self.assertTrue(r.browsers[0].closed);self.assertFalse(r.browsers[1].closed)
                        self.assertEqual(r.occurrences['bridge-started'],2)


    def test_installed_cancellation_survives_failed_failure_record_and_cleanup_still_runs(self):
        r=p.InstalledRun(Path('unused'),Path('unused'))
        r.failure_record=Mock(side_effect=RuntimeError('record failed'));r.failure_cleanup=Mock()
        with patch('qualification.installed.preflight_module',side_effect=KeyboardInterrupt):
            with self.assertRaises(KeyboardInterrupt): r.execute()
        r.failure_cleanup.assert_called_once()

    def test_setup_attempt_flag_precedes_process_call_and_refuses_replay(self):
        r=p.InstalledRun(Path('unused'),Path('unused'));r.environment={}
        def launch(*args,**kwargs):
            self.assertTrue(r.setup_start_attempted)
            raise RuntimeError('synthetic unknown process creation')
        with patch('qualification.installed.Controls',return_value=object()),patch('qualification.installed.subprocess.Popen',side_effect=launch) as spawn:
            with self.assertRaises(RuntimeError): r.start_setup(None)
            with self.assertRaises(RuntimeError): r.start_setup(None)
        self.assertEqual(spawn.call_count,1);self.assertIsNone(r.process)


    def test_supervisor_keeps_failure_and_cancellation_after_exact_cleanup(self):
        r=p.ParentInstalledRun(Path('unused'),Path('unused'),Path('unused'),'a'*64,Path('unused'),'b'*64,parent_transport_experiment=True)
        settled=False
        r.cleanup_complete=lambda:settled
        def cleanup():
            nonlocal settled
            settled=True
        r.failure_cleanup=Mock(side_effect=cleanup)
        supervisor=ParentSupervisor();supervisor.retain('sdk',r)
        with patch.object(p.InstalledRun,'execute',side_effect=KeyboardInterrupt) as execute:
            status=supervisor.execute(lambda _:r.execute())
            self.assertTrue(status['cancelled']);self.assertTrue(status['failed']);self.assertFalse(status['held'])
            with self.assertRaises(KeyboardInterrupt): supervisor.finish()
            with self.assertRaises(RuntimeError): r.execute()
            self.assertEqual(execute.call_count,1)
        r.failure_cleanup.assert_called_once();self.assertTrue(r.cleanup())

    def test_supervisor_cannot_finish_unknown_creation_or_replay_cleanup(self):
        r=p.ParentInstalledRun(Path('unused'),Path('unused'),Path('unused'),'a'*64,Path('unused'),'b'*64,parent_transport_experiment=True)
        r.cleanup_complete=lambda:False;r.failure_cleanup=Mock()
        supervisor=ParentSupervisor();supervisor.retain('sdk',r)
        with patch.object(p.InstalledRun,'execute',side_effect=RuntimeError('synthetic unknown start')):
            status=supervisor.execute(lambda _:r.execute())
        self.assertTrue(status['held']);self.assertTrue(status['failed']);self.assertIsNone(supervisor.exit_code())
        with self.assertRaises(RuntimeError): supervisor.finish()
        self.assertFalse(r.cleanup());r.failure_cleanup.assert_called_once()


    def test_disable_uses_retained_neutral_tab_not_the_extension_tab(self):
        b,calls=self.browser();selected=['manager'];original=b.command.side_effect
        def command(name,args=None):
            if name=='WebDriver:SwitchToWindow': selected[0]=args['handle']
            return original(name,args)
        b.command.side_effect=command
        def control(browser,operation,expected):
            if selected[0]=='manager':
                raise AutomationError('WebDriver:ExecuteAsyncScript',{'error':'no such window','message':'private model text'})
            calls.append(operation)
        with patch.object(p,'control',side_effect=control),patch.object(p.Firefox,'close',self.closed(calls)):
            b.close()
        self.assertEqual(selected[0],'control');self.assertTrue(b.cleanup_complete())

    def test_uncertain_control_selection_is_observed_not_replayed_or_adopted(self):
        b,calls=self.browser();b.command.side_effect=AutomationError('WebDriver:SwitchToWindow',{'error':'no such window'})
        with self.assertRaises(AutomationError):b.close()
        self.assertEqual(b.first_failure['stage'],'control-tab-selection')
        self.assertEqual(b.first_failure['kind'],'no such window')
        self.assertTrue(b.first_failure['locations'])
        self.assertFalse(b.first_failure['trace_truncated'])
        self.assertFalse(b.disable_attempted)
        b.command.side_effect=lambda name,args=None:{'value':'wrong'}
        with self.assertRaises(RuntimeError):b.close()
        self.assertEqual(sum(c.args[0]=='WebDriver:SwitchToWindow' for c in b.command.call_args_list),1)
        self.assertEqual(b.first_failure['kind'],'no such window');self.assertFalse(b.disable_attempted)

    def test_control_handle_with_changed_url_refuses_before_disable(self):
        b,_=self.browser();b.command.side_effect=lambda name,args=None:({'value':'control'} if name=='WebDriver:GetWindowHandle' else {'value':'about:other'})
        with patch.object(p.Firefox,'close',self.closed([])):
            with self.assertRaises(RuntimeError):b.close()
        b.chrome.assert_not_called();self.assertFalse(b.disable_attempted)

    def test_new_manager_tab_is_retained_before_addon_effects_and_cannot_replay(self):
        b,_=self.browser();b.load_attempted=False;b.tab_attempted=False;b.manager_handle=None;b.parent_lease.acquired=True
        def command(name,args=None):
            if name=='WebDriver:NewWindow': return {'value':{'handle':'new-manager','type':'tab'}}
            self.assertEqual(args,{'handle':'new-manager'});self.assertEqual(b.manager_handle,'new-manager')
        b.command.side_effect=command
        def load(browser,xpi):
            self.assertTrue(browser.load_attempted);self.assertTrue(browser.tab_attempted);self.assertEqual(browser.manager_handle,'new-manager')
        with patch.object(p.Firefox,'load',load),patch.object(p,'control'):
            b.load(Path('metadata-only.xpi'))
            with self.assertRaises(RuntimeError): b.load(Path('metadata-only.xpi'))
        self.assertEqual(sum(c.args[0]=='WebDriver:NewWindow' for c in b.command.call_args_list),1)

    def test_tab_response_refusals_have_fixed_stages_without_retaining_values(self):
        import json
        sentinel='private-response-value'
        cases=[
            (None,'shape'),
            ({'handle':sentinel,'type':'tab','extra':sentinel},'shape'),
            ({'handle':sentinel,'type':'window'},'type'),
            ({'handle':'bad handle','type':'tab'},'handle'),
            ({'handle':'control','type':'tab'},'distinct'),
        ]
        for response,step in cases:
            b,_=self.browser();b.load_attempted=False;b.tab_attempted=False;b.manager_handle=None
            b.command.return_value=response;b.command.side_effect=None
            with self.subTest(step=step),patch.object(p.Firefox,'load') as load:
                with self.assertRaises(RuntimeError):b.load(Path('metadata-only.xpi'))
                self.assertEqual(b.first_failure['stage'],'manager-tab-response-'+step)
                self.assertNotIn(sentinel,json.dumps(b.diagnostic()))
                self.assertIsNone(b.manager_handle);self.assertFalse(b.load_attempted)
                with self.assertRaises(RuntimeError):b.load(Path('metadata-only.xpi'))
                b.command.assert_called_once_with('WebDriver:NewWindow',{'type':'tab'})
                load.assert_not_called()

    def test_tab_failure_snapshot_is_bounded_copied_and_never_retried(self):
        b,_=self.browser();b.load_attempted=False;b.tab_attempted=False;b.manager_handle=None
        b.command.side_effect=None;b.command.return_value={'handle':'control','type':'tab'}
        state={'version':1,**{key:False for key in p.TAB_FIELDS}}
        state['window_modal']=True;state['selected_control']=None;b.read_tab_state.return_value=state
        with self.assertRaises(RuntimeError):b.load(Path('unused'))
        self.assertEqual(b.first_failure['stage'],'manager-tab-response-distinct')
        self.assertEqual(b.tab_failure,{'state':'observed',**state})
        state['window_modal']=False
        observed=b.diagnostic();observed['tab_failure']['window_modal']=False
        self.assertTrue(b.tab_failure['window_modal']);self.assertTrue(b.failed)
        b.observe_tab_failure();b.read_tab_state.assert_called_once()
        self.assertFalse(b.load_attempted);self.assertIsNone(b.manager_handle)

    def test_bad_tab_snapshot_cannot_replace_original_refusal_or_leak_values(self):
        import json
        valid={'version':1,**{key:False for key in p.TAB_FIELDS}}
        for result in (None,{**valid,'version':True},{**valid,'window_modal':1},
                       {**valid,'selected_blank':'secret-value'},{**valid,'extra':'secret-value'}):
            b,_=self.browser();b.load_attempted=False;b.tab_attempted=False
            primary=RuntimeError('original tab failure');b.command.side_effect=primary;b.read_tab_state.return_value=result
            with self.subTest(result=result),self.assertRaises(RuntimeError) as caught:b.load(Path('unused'))
            self.assertIs(caught.exception,primary)
            self.assertEqual(b.first_failure['stage'],'manager-tab-creation')
            self.assertEqual(b.tab_failure,{'state':'unavailable'})
            self.assertNotIn('secret-value',json.dumps(b.diagnostic()))
            b.observe_tab_failure();b.read_tab_state.assert_called_once()

    def test_tab_snapshot_preserves_interruptions_without_new_commands_after_original_interrupt(self):
        for primary in (KeyboardInterrupt(),RuntimeError('first')):
            b,_=self.browser();b.load_attempted=False;b.tab_attempted=False
            b.command.side_effect=primary;secondary=KeyboardInterrupt();b.read_tab_state.side_effect=secondary
            with self.assertRaises(KeyboardInterrupt) as caught:b.load(Path('unused'))
            self.assertIs(caught.exception,primary if isinstance(primary,KeyboardInterrupt) else secondary)
            self.assertEqual(b.first_failure['stage'],'manager-tab-creation');self.assertTrue(b.failed)
            if isinstance(primary,KeyboardInterrupt):
                b.observe_tab_failure();b.read_tab_state.assert_not_called()
                self.assertEqual(b.tab_failure,{'state':'skipped-interruption'})
            else:b.read_tab_state.assert_called_once();self.assertEqual(b.tab_failure,{'state':'unavailable'})

    def test_original_tab_snapshot_commands_restore_once_and_preserve_script_interruption(self):
        for interrupted in (False,True):
            b,_=self.browser();del b.read_tab_state;b.verified=True
            failure=KeyboardInterrupt();b.script=Mock(side_effect=failure if interrupted else None,return_value={'observed':True})
            b.command.side_effect=[None,RuntimeError('restore')] if interrupted else [None,None]
            if interrupted:
                with self.assertRaises(BaseException) as caught:b.read_tab_state()
                self.assertIs(caught.exception,failure)
            else:self.assertEqual(b.read_tab_state(),{'observed':True})
            b.script.assert_called_once_with(p.TAB_STATE,['control'])
            self.assertEqual([c.args for c in b.command.call_args_list],[('Marionette:SetContext',{'value':'chrome'}),('Marionette:SetContext',{'value':'content'})])

    def test_plain_tab_reply_through_original_command_correlation_before_load(self):
        import json
        b,_=self.browser();b.load_attempted=False;b.tab_attempted=False;b.manager_handle=None
        del b.command
        b.verified=True;b.serial=0;b.connection=Mock()
        b.receive=Mock(side_effect=[[1,1,None,{'handle':'new-manager','type':'tab'}],[1,2,None,{'value':None}]])
        def load(browser,_):
            self.assertEqual(browser.manager_handle,'new-manager');self.assertTrue(browser.load_attempted)
        with patch.object(p.Firefox,'load',load),patch.object(p,'control'):
            b.load(Path('metadata-only.xpi'))
        packets=[json.loads(call.args[0].split(b':',1)[1]) for call in b.connection.sendall.call_args_list]
        self.assertEqual(packets,[[0,1,'WebDriver:NewWindow',{'type':'tab'}],[0,2,'WebDriver:SwitchToWindow',{'handle':'new-manager'}]])
        self.assertFalse(b.failed)

    def test_flat_marionette_tab_reply_and_selection_failure_retain_one_original_handle(self):
        b,_=self.browser();b.load_attempted=False;b.tab_attempted=False;b.manager_handle=None
        b.command.side_effect=[{'handle':'new-manager','type':'tab'},RuntimeError('modeled selection refusal')]
        with patch.object(p.Firefox,'load') as load:
            with self.assertRaises(RuntimeError):b.load(Path('metadata-only.xpi'))
            self.assertEqual(b.first_failure['stage'],'manager-tab-selection')
            self.assertEqual(b.manager_handle,'new-manager');self.assertFalse(b.load_attempted)
            with self.assertRaises(RuntimeError):b.load(Path('metadata-only.xpi'))
            self.assertEqual(b.command.call_count,2);load.assert_not_called()

    def test_failed_tab_creation_does_not_claim_or_dispatch_addon_load(self):
        b,_=self.browser();b.load_attempted=False;b.tab_attempted=False;b.parent_lease.acquired=True
        b.command.side_effect=RuntimeError('unknown tab creation')
        with patch.object(p.Firefox,'load') as load:
            with self.assertRaises(RuntimeError):b.load(Path('metadata-only.xpi'))
            with self.assertRaises(RuntimeError):b.load(Path('metadata-only.xpi'))
        load.assert_not_called();self.assertFalse(b.load_attempted);self.assertTrue(b.tab_attempted)

    def test_first_command_failure_is_not_replaced_by_context_restore_failure(self):
        b,_=self.browser();b.stage='disable-dispatch';del b.command
        primary=AutomationError('WebDriver:ExecuteAsyncScript',{'error':'script timeout','message':'not-retained'})
        restore=AutomationError('Marionette:SetContext',{'error':'no such window'})
        with patch.object(p.Firefox,'command',side_effect=[primary,restore]):
            with self.assertRaises(AutomationError):b.command('WebDriver:ExecuteAsyncScript')
            with self.assertRaises(AutomationError):b.command('Marionette:SetContext',{'value':'content'})
        self.assertEqual(b.command_failure,{'stage':'disable-dispatch','command':'WebDriver:ExecuteAsyncScript','kind':'script timeout'})
        self.assertNotIn('not-retained',str(b.diagnostic()))

    def test_bounded_failure_record_survives_ordinary_sink_error(self):
        with tempfile.TemporaryDirectory() as directory:
            r=p.ParentInstalledRun(Path('unused'),Path('unused'),Path('unused'),'a'*64,Path('unused'),'b'*64,parent_transport_experiment=True)
            r.plan=SimpleNamespace(path=Path(directory),created=True);b,_=self.browser();r.browsers=[b]
            error=AutomationError('WebDriver:ExecuteAsyncScript',{'error':'script timeout','message':'must-not-persist-opaque-model-detail'})
            b.chrome.side_effect=error
            with self.assertRaises(AutomationError):b.close()
            with patch.object(p.InstalledRun,'failure_record',side_effect=RuntimeError('ordinary sink')):
                with self.assertRaises(RuntimeError):r.failure_record(error)
            self.assertTrue((r.plan.path/'parent-failure.private.json').exists())
            text=(r.plan.path/'parent-failure.private.json').read_text(encoding='utf-8')
            self.assertIn('disable-dispatch',text);self.assertIn('script timeout',text);self.assertNotIn('must-not-persist',text)

    def test_independent_cleanup_records_even_on_cancellation_and_does_not_replay(self):
        r=p.ParentInstalledRun(Path('unused'),Path('unused'),Path('unused'),'a'*64,Path('unused'),'b'*64,parent_transport_experiment=True)
        r.record_diagnostic=Mock(side_effect=OSError('model sink'));cancelled=KeyboardInterrupt()
        with patch.object(r,'_failure_step',side_effect=cancelled) as cleanup:
            with self.assertRaises(KeyboardInterrupt) as caught:r.failure_cleanup()
            self.assertIs(caught.exception,cancelled);r.failure_cleanup()
        cleanup.assert_called_once();r.record_diagnostic.assert_called_once_with('parent-cleanup.private.json')

    def test_invalid_report_refuses_before_domain_preparation_or_application_effects(self):
        r=p.ParentInstalledRun(Path('unused'),Path('unused'),Path('unused'),'a'*64,Path('unused'),'b'*64,parent_transport_experiment=True)
        r.inputs=Mock(side_effect=AssertionError('no input after report refusal'))
        with patch.object(p.InstalledRun,'prepare',side_effect=AssertionError('no domain effects')) as prepare:
            with self.assertRaises(RuntimeError): r.prepare()
        prepare.assert_not_called();r.inputs.assert_not_called();self.assertIsNone(r.plan)


if __name__=='__main__': unittest.main()
