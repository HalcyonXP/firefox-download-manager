"""Fileless owned review models; no Firefox, installation, consent or native work."""
import sys
import threading
import time
from pathlib import Path
from types import SimpleNamespace as NS
import unittest
from unittest.mock import Mock,patch
from qualification import startup_review as m
from qualification import parent_installed as p


def reply(state):return {'version':1,'state':state}


def browser(*states):
    process=Mock();process.poll.return_value=None
    return NS(failed=False,verified=True,parent_transport_experiment=True,original=process,process=process,
              parent_lease=NS(acquired=True,released=False,failed=False),load_attempted=False,tab_attempted=False,
              read_original_chrome=Mock(side_effect=[reply(s) for s in states]))


class ReviewTests(unittest.TestCase):
    def review(self,*states):return m.Review(browser(*states),time.monotonic()+200,Mock(),Mock())

    def shown(self,r,action):
        def show():
            r.root=Mock();r.create_attempted=True;r.visible=True;r.message=Mock()
            action()
            if r.error is not None:raise r.error
            if r.decision!='clear':raise RuntimeError(m.ERROR)
        r.show=show

    def test_no_modal_needs_no_window_and_makes_no_consent_claim(self):
        r=self.review('clear');r.run();self.assertTrue(r.cleanup_complete())
        self.assertIsNone(r.root);r.notify.assert_not_called();r.verify_inputs.assert_not_called()
        self.assertTrue(r.record()['structural_clear']);self.assertFalse(r.record()['consent_observed'])
        with self.assertRaises(RuntimeError):r.run()
        self.assertEqual(r.checks,1)

    def test_check_and_final_read_follow_original_window_retirement(self):
        r=self.review('terms','clear','clear');self.shown(r,r.check)
        command=r.browser.read_original_chrome
        def read(*args):
            if command.call_count==3:self.assertTrue(r.destroyed)
            return reply('terms' if command.call_count==1 else 'clear')
        command.side_effect=read;failure=None
        try:r.run()
        except Exception as error:failure=error
        self.assertIsNone(failure,'the positive ordered review must complete')
        r.root.destroy.assert_called_once();self.assertEqual(r.checks,3)
        self.assertEqual([v.args[0] for v in r.notify.call_args_list],['startup-review','startup-ready'])
        self.assertFalse(r.record()['consent_observed'])

    def test_check_while_terms_remain_does_not_close_or_accept_them(self):
        r=self.review('terms','terms','clear','clear')
        def action():
            r.check();self.assertIsNone(r.decision);r.root.destroy.assert_not_called();r.check()
        self.shown(r,action);r.run();self.assertEqual(r.checks,4)

    def test_stop_destroys_only_owned_review_window_then_refuses(self):
        r=self.review('terms');self.shown(r,r.stop)
        with self.assertRaises(RuntimeError):r.run()
        self.assertTrue(r.failed);self.assertTrue(r.cleanup_complete());self.assertEqual(r.checks,1)
        self.assertEqual([v.args[0] for v in r.notify.call_args_list],['startup-review'])

    def test_uncertain_read_never_retries_and_interruption_survives_destroy_error(self):
        r=self.review('terms');interrupt=KeyboardInterrupt();r.browser.read_original_chrome.side_effect=[reply('terms'),interrupt]
        def action():r.root.destroy.side_effect=OSError('sink');r.check()
        self.shown(r,action)
        with self.assertRaises(KeyboardInterrupt) as caught:r.run()
        self.assertIs(caught.exception,interrupt);self.assertFalse(r.cleanup_complete());self.assertEqual(r.checks,2)
        secondary=None
        try:r.close()
        except BaseException as error:secondary=error
        r.root.destroy.assert_called_once()
        self.assertIsInstance(secondary,RuntimeError)

    def test_changed_process_or_modal_after_review_refuses_admission(self):
        r=self.review('terms','clear','terms');self.shown(r,r.check)
        with self.assertRaises(RuntimeError):r.run()
        self.assertTrue(r.destroyed);self.assertTrue(r.failed)
        r=self.review('clear');r.browser.process=Mock(pid=r.browser.original.pid)
        with self.assertRaises(RuntimeError):r.run()
        r.browser.read_original_chrome.assert_not_called()

    def test_exact_closed_response_and_deadline_refuse_before_further_commands(self):
        for value in (None,reply('unsupported'),reply('unavailable'),{'version':True,'state':'clear'},{**reply('clear'),'extra':'opaque'}):
            r=self.review();r.browser.read_original_chrome.side_effect=None;r.browser.read_original_chrome.return_value=value
            with self.assertRaises(RuntimeError):r.run()
            self.assertEqual(r.checks,1);self.assertFalse(r.create_attempted)
        r=self.review('clear');r.not_after=time.monotonic()-1
        with self.assertRaises(RuntimeError):r.run()
        self.assertEqual(r.checks,0)

    def test_original_thread_and_bounded_check_count_are_required(self):
        r=self.review('clear');r.thread=object()
        with self.assertRaises(RuntimeError):r.run()
        r=self.review('clear');r.checks=10
        with self.assertRaises(RuntimeError):r.run()
        r.browser.read_original_chrome.assert_not_called()

    def fake_tk(self,r,update):
        events=[]
        class Root:
            def __init__(self,**_):
                self.assert_retained();events.append('init')
            def assert_retained(self):
                if r.root is not self or not r.create_attempted:raise AssertionError('root not retained')
            def title(self,*_):pass
            def geometry(self,*_):pass
            def lift(self):events.append('lift')
            def protocol(self,name,callback):self.stop_callback=callback
            def update(self):update(self)
            def destroy(self):self.assert_retained();events.append('destroy')
        class Widget:
            def __init__(self,parent,**_):self.parent=parent
            def pack(self,**_):pass
            def configure(self,**_):pass
        tk=NS(Tk=Root,NoDefaultRoot=lambda:events.append('no-default-root'),ttk=NS(Frame=Widget,Label=Widget,Button=Widget))
        return tk,events

    def test_actual_show_structure_retains_root_and_close_is_stop_not_consent(self):
        r=self.review('terms');tk,events=self.fake_tk(r,lambda root:root.stop_callback())
        with patch.dict(sys.modules,{'tkinter':tk}),patch.object(sys,'flags',NS(isolated=1,ignore_environment=1)):
            with self.assertRaises(RuntimeError):r.run()
        self.assertEqual(events,['no-default-root','init','lift','destroy']);r.verify_inputs.assert_called_once()
        self.assertTrue(r.cleanup_complete());self.assertFalse(r.record()['consent_observed'])

    def test_human_wait_expiry_closes_review_without_any_firefox_response(self):
        r=self.review('terms');tk,events=self.fake_tk(r,lambda root:setattr(r,'not_after',time.monotonic()-1))
        with patch.dict(sys.modules,{'tkinter':tk}),patch.object(sys,'flags',NS(isolated=1,ignore_environment=1)),patch.object(m.time,'sleep',return_value=None):
            with self.assertRaises(RuntimeError):r.run()
        self.assertEqual(events[-1],'destroy');self.assertTrue(r.failed);self.assertEqual(r.checks,1)

    def test_selected_hook_is_before_tab_effects_with_browser_already_retained(self):
        run=p.ParentInstalledRun.__new__(p.ParentInstalledRun)
        run.inputs=Mock(return_value=(Path('unused.xpi'),{}));run.owner=object();run.binding=object();run.current_binding=Mock()
        run.fixtures=[];run.browsers=[];run.executable=Path('unused.exe');run.plan=NS(path=Path('unused'));run.environment={}
        b=Mock()
        def review(current):
            self.assertIs(current,b);self.assertEqual(run.browsers,[b]);b.start.assert_called_once();b.load.assert_not_called()
            raise RuntimeError('review stopped')
        run.before_browser_load=review
        with patch.object(p,'BrowserPeer'),patch.object(p,'Fixture'),patch.object(p,'ParentBrowser',return_value=b):
            with self.assertRaises(RuntimeError):run.transfer(None)
        b.load.assert_not_called();self.assertEqual(run.stage,'parent-startup-review')

    def test_metadata_refusal_is_consumed_without_uncreated_root_and_has_bounded_location(self):
        r=m.Review.__new__(m.Review)
        with self.assertRaises(RuntimeError):r.__init__(browser(),float('nan'),Mock(),Mock())
        self.assertTrue(r.cleanup_complete())
        with self.assertRaises(RuntimeError):r.__init__(browser(),time.monotonic()+20,Mock(),Mock())
        r=self.review('clear');r.not_after=time.monotonic()-1
        try:r.run()
        except RuntimeError as error:
            from qualification.failure_location import failure_location
            self.assertTrue(any(v['source']=='startup-review' for v in failure_location(error)['locations']))
        else:self.fail('deadline refusal expected')
