"""Selected single-owner diagnostic control; no launch, GUI or browser selection.

A retained local caller supplies the owner and byte sinks. Continuation is only
for that owner's failed cleanup. Sequence IDs correlate requests, not authority.
The ordinary/fileless supervisor's serve interface is deliberately unchanged.
"""
import json
import threading
import time
from .parent_supervisor import ParentSupervisor

ERROR='Retained diagnostic control refused; keep original owners'
PHASES=('preflight','setup','manual-download','addon-retirement','browser-retirement','uninstall','controller-retirement')


def _unique(pairs):
    value={}
    for key,item in pairs:
        if key in value: raise RuntimeError(ERROR)
        value[key]=item
    return value


def _refuse(*_): raise RuntimeError(ERROR)


def request(raw):
    if type(raw) is not bytes or not raw.endswith(b'\n') or len(raw)>256: raise RuntimeError(ERROR)
    value=json.loads(raw.decode('ascii'),object_pairs_hook=_unique,parse_constant=_refuse)
    if (type(value) is not dict or set(value)!={'version','sequence','command'}
            or type(value['version']) is not int or value['version']!=1
            or type(value['sequence']) is not int or not 1<=value['sequence']<=32
            or value['command'] not in ('start','status','continue','finish')): raise RuntimeError(ERROR)
    return value


class RetirementControl:
    def __init__(self,owner,write):
        if hasattr(self,'_owner'): raise RuntimeError(ERROR)
        if (not callable(write) or any(not callable(getattr(owner,name,None))
                for name in ('execute','cleanup','cleanup_complete','continue_retirement'))): raise RuntimeError(ERROR)
        self._owner=owner
        self._supervisor=ParentSupervisor()
        self._supervisor.retain('sdk',owner) # Before any execution or sink effects.
        self.write=write;self.thread=threading.current_thread()
        self.sequence=0;self.busy=False;self.operation=None;self.phases=0;self.total_phases=0
        self.quarantined=False;self.output_failed=False;self.channel_failed=False;self.finishing=False

    @property
    def owner(self): return self._owner

    @property
    def supervisor(self): return self._supervisor

    def bound(self):
        entries=self.supervisor.owners
        return (type(entries) is list and len(entries)==1 and type(entries[0]) is tuple
                and len(entries[0])==2 and entries[0][0]=='sdk' and entries[0][1] is self.owner)

    def guard(self):
        if self.quarantined or not self.bound() or threading.current_thread() is not self.thread: raise RuntimeError(ERROR)

    def emit(self,kind,value):
        self.guard()
        if self.output_failed: raise RuntimeError(ERROR)
        raw=(json.dumps({'version':1,'sequence':self.sequence,'kind':kind,kind:value},separators=(',',':'))+'\n').encode('ascii')
        if len(raw)>2048: raise RuntimeError(ERROR)
        try:
            if self.write(raw) is not True: raise RuntimeError(ERROR)
        except BaseException:
            self.output_failed=True # Never retry an uncertain output operation.
            raise

    def progress(self,phase):
        # Synchronous original-thread milestones only: no sampler/writer races
        # and no phase frames after their terminal status response.
        self.guard()
        if (not self.busy or self.operation not in ('start','continue') or type(phase) is not str
                or phase not in PHASES or self.phases>=32 or self.total_phases>=96):
            raise RuntimeError(ERROR)
        # Reserve up to32 terminal responses within the128-frame channel limit.
        self.phases+=1;self.total_phases+=1
        self.emit('phase',phase)

    def handle(self,raw):
        try: self._handle(raw)
        except BaseException as error:
            self.channel_failed=True
            self.supervisor._failure(error)
            # The owning serve loop cleans up after unwinding. Do not invoke
            # owner cleanup inside a reentrant sink/active execute callback.
            raise

    def _handle(self,raw):
        self.guard()
        if self.busy or self.finishing or self.output_failed or self.channel_failed: raise RuntimeError(ERROR)
        value=request(raw)
        if value['sequence']!=self.sequence+1: raise RuntimeError(ERROR)
        command=value['command']
        if self.sequence==0 and command!='start': raise RuntimeError(ERROR)
        self.sequence=value['sequence'];self.operation=command;self.phases=0;self.busy=True
        try:
            if command=='start':
                self.supervisor.execute(lambda _:self.owner.execute())
            elif command=='continue':
                status=self.supervisor.status()
                if not all(status[key] is True for key in ('started','failed','cleanup_attempted','held')): raise RuntimeError(ERROR)
                try:
                    if self.owner.continue_retirement() is not True: self.supervisor._failure(RuntimeError(ERROR))
                except BaseException as error:
                    self.supervisor._failure(error) # Cancel stays retained until finish.
            elif command=='finish':
                if self.supervisor.exit_code() is None: raise RuntimeError(ERROR)
                self.finishing=True # Intent before the terminal output operation.
            self.guard()
            self.emit('status',self.supervisor.status())
        finally:
            self.busy=False;self.operation=None

    def fail(self,error):
        self.channel_failed=True
        self.supervisor._failure(error)
        if not self.bound() or threading.current_thread() is not self.thread:
            self.quarantined=True # Never adopt an owner or execute on another thread.
            return
        self.supervisor.cleanup() # Existing one-shot cleanup, not continuation.

    def exit_code(self):
        if self.quarantined or not self.bound(): return None
        return self.supervisor.exit_code()

    def finish(self):
        self.guard()
        if not self.finishing or self.output_failed or self.exit_code() is None: raise RuntimeError(ERROR)
        return self.supervisor.finish() # Preserve original interruption after retirement.


def serve(control,read,notice):
    """Keep the original host alive on EOF/sink failure while resources remain.

    Valid failed-held responses leave the command channel OPEN for deliberate
    continuation. A broken channel never restarts the run or replays an inverse.
    The caller must also retain this control across entry/exit-record failures.
    """
    try:
        while True:
            control.handle(read())
            if control.finishing: return control.finish()
    except BaseException as error:
        control.fail(error)
        try: notice()
        except BaseException as failure: control.fail(failure)
        while control.exit_code() is None:
            try: time.sleep(1)
            except BaseException as interruption: control.fail(interruption)
        return control.supervisor.finish()
