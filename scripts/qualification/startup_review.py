"""Opt-in owned first-run review. Structural readiness is NEVER a consent receipt."""
import math
import os
from pathlib import Path
import sys
import threading
import time
from .parent_installed import independent

SOURCE=Path(__file__).with_suffix('.js').read_text(encoding='utf-8')
ERROR='Owned startup review refused'


class Review:
    def __init__(self,browser,not_after,verify_inputs,notify):
        if hasattr(self,'thread'): raise RuntimeError(ERROR)
        self.thread=threading.current_thread();self.browser=browser;self.not_after=not_after
        self.verify_inputs=verify_inputs;self.notify=notify
        self.attempted=False;self.root=None;self.create_attempted=False
        self.destroy_attempted=False;self.destroyed=False;self.failed=False
        self.decision=None;self.error=None;self.checks=0;self.visible=False
        if (type(not_after) is not float or not math.isfinite(not_after)
                or not_after>time.monotonic()+240
                or not callable(verify_inputs) or not callable(notify)): raise RuntimeError(ERROR)

    def guard(self):
        b=self.browser
        if (threading.current_thread() is not self.thread or self.failed or b.failed is not False
                or b.verified is not True or b.parent_transport_experiment is not True
                or b.original is None or b.process is not b.original or b.original.poll() is not None
                or b.parent_lease is None or b.parent_lease.acquired is not True
                or b.parent_lease.released is not False or b.parent_lease.failed is not False
                or b.load_attempted is not False or b.tab_attempted is not False): raise RuntimeError(ERROR)
        if time.monotonic()>=self.not_after: raise RuntimeError(ERROR)

    def read(self):
        self.guard()
        if self.checks>=10: raise RuntimeError(ERROR)
        self.checks+=1 # Reserve before a command; uncertainty stops, never retries.
        value=self.browser.read_original_chrome(SOURCE,True)
        self.guard()
        if (type(value) is not dict or set(value)!={'version','state'}
                or type(value['version']) is not int or value['version']!=1
                or type(value['state']) is not str or value['state'] not in {'clear','terms'}): raise RuntimeError(ERROR)
        return value['state']

    def stop(self):
        self.decision='stop' # Stop is not a response to Firefox's terms.

    def check(self):
        if self.decision is not None: return
        try:
            if self.read()=='clear': self.decision='clear'
            else: self.message.configure(text='Firefox still shows its first-run flow. Review and respond in Firefox, then check again. This button does not accept anything.')
        except BaseException as error:
            self.error=error;self.failed=True;self.decision='stop'

    def callback_failure(self,kind,error,trace):
        if self.error is None or (isinstance(self.error,Exception) and not isinstance(error,Exception)):
            self.error=error
        self.failed=True;self.decision='stop'

    def close(self):
        if threading.current_thread() is not self.thread: raise RuntimeError(ERROR)
        if not self.create_attempted: return
        if self.destroy_attempted:
            if not self.destroyed: raise RuntimeError(ERROR)
            return
        self.destroy_attempted=True
        self.root.destroy()
        self.destroyed=True

    def cleanup_complete(self):
        return (not self.create_attempted and self.root is None) or (
            self.create_attempted and self.root is not None and self.destroyed is True)

    def show(self):
        # No implicit Tk root, HOME/cwd readprofile or injected Tcl library paths.
        if sys.flags.isolated!=1 or sys.flags.ignore_environment!=1 or any(
                key in os.environ for key in ('TCL_LIBRARY','TK_LIBRARY','TCLLIBPATH')): raise RuntimeError(ERROR)
        self.verify_inputs()
        import tkinter as tk
        from tkinter import ttk
        tk.NoDefaultRoot()
        self.root=tk.Tk.__new__(tk.Tk) # Retain before initialization effects.
        self.create_attempted=True
        self.root.__init__(baseName='ownedStartupReview',className='OwnedStartupReview')
        self.root.report_callback_exception=self.callback_failure
        self.root.title('Firefox first-run review — isolated test')
        self.root.geometry('650x310')
        body=ttk.Frame(self.root,padding=18);body.pack(fill='both',expand=True)
        self.message=ttk.Label(body,wraplength=610,text=(
            'Action required in the temporary Firefox window. Review its Terms of Use flow and make your own choice. '
            'No download has started; this wait is not test progress.'))
        self.message.pack(anchor='w',pady=10)
        ttk.Label(body,wraplength=610,text=(
            'After responding in Firefox, choose Check and continue below. To decline or stop, use Stop test here. '
            'Do not close Firefox, this test terminal or the supervisor yourself. '
            'The existing test time limits remain; expiry stops the test without accepting anything.')).pack(anchor='w',pady=10)
        row=ttk.Frame(body);row.pack(anchor='w',pady=12)
        ttk.Button(row,text='Check and continue',command=self.check).pack(side='left',padx=8)
        ttk.Button(row,text='Stop test',command=self.stop).pack(side='left',padx=8)
        self.root.protocol('WM_DELETE_WINDOW',self.stop)
        self.visible=True
        self.root.lift() # One attention presentation; no focus or modal grab.
        # Original-thread event loop. No custom timers, extra reader, keyboard
        # grab, modal grab, focus forcing or second browser command thread.
        while self.decision is None:
            self.guard()
            self.root.update()
            if self.decision is None: time.sleep(.1)
        if self.error is not None: raise self.error
        if self.decision!='clear': raise RuntimeError(ERROR)

    def run(self):
        if self.attempted: raise RuntimeError(ERROR)
        self.attempted=True
        try:
            if self.read()=='clear':
                self.decision='already-clear'
                return # No UI needed; no consent claim.
            self.notify('startup-review')
            independent((self.show,self.close))
            if not self.cleanup_complete() or self.read()!='clear': raise RuntimeError(ERROR)
            self.notify('startup-ready')
        except BaseException:
            self.failed=True
            raise

    def record(self):
        if not self.cleanup_complete(): raise RuntimeError(ERROR)
        return {'qualification':False,'ui_requested':self.visible,'failed':self.failed,
                'checks':self.checks,'window_destroy_returned':self.destroyed,
                'structural_clear':self.decision in ('clear','already-clear'),'consent_observed':False}
