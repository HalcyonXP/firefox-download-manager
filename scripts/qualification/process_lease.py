"""Read-only Windows process leases for a retained launcher and its direct child.

Not browser authentication or permission to adopt arbitrary PIDs. The caller must
retain this object BEFORE acquire(), own the launcher, and obtain the candidate
from its reviewed creation/control channel. No process launch/kill, profile,
registration, memory, environment or command-line inspection is implemented.
"""
import ctypes
from ctypes import wintypes as w
import os
from pathlib import Path
import subprocess
import sys

ERROR = 'retained process lease refused'
RIGHTS = 0x00100000 | 0x1000  # SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION


class Entry(ctypes.Structure):
    _fields_ = [('size',w.DWORD),('usage',w.DWORD),('pid',w.DWORD),
                ('heap',ctypes.c_size_t),('module',w.DWORD),('threads',w.DWORD),
                ('parent',w.DWORD),('priority',w.LONG),('flags',w.DWORD),('image',w.WCHAR*260)]


class WindowsProcessAPI:
    """Snapshot traverses PIDs for the selected parent; opens only specified IDs.

    Failed snapshot inverses stay retained and invalidate clean completion. This
    backend is not a production dependency or a general process-discovery API.
    """
    def __init__(self):
        if (os.name != 'nt' or ctypes.sizeof(ctypes.c_void_p) != 8 or ctypes.sizeof(Entry) != 568
                or Entry.pid.offset != 8 or Entry.parent.offset != 32 or Entry.image.offset != 44
                or ctypes.sizeof(w.FILETIME) != 8): raise RuntimeError(ERROR)
        self.snapshots = []
        self.dll = ctypes.WinDLL(str(Path(os.environ['WINDIR'])/'System32/kernel32.dll'),use_last_error=True)
        signatures = {
            'OpenProcess':([w.DWORD,w.BOOL,w.DWORD],w.HANDLE),
            'CloseHandle':([w.HANDLE],w.BOOL),
            'GetProcessId':([w.HANDLE],w.DWORD),
            'GetProcessTimes':([w.HANDLE,*([ctypes.POINTER(w.FILETIME)]*4)],w.BOOL),
            'QueryFullProcessImageNameW':([w.HANDLE,w.DWORD,w.LPWSTR,ctypes.POINTER(w.DWORD)],w.BOOL),
            'WaitForSingleObject':([w.HANDLE,w.DWORD],w.DWORD),
            'GetExitCodeProcess':([w.HANDLE,ctypes.POINTER(w.DWORD)],w.BOOL),
            'CreateToolhelp32Snapshot':([w.DWORD,w.DWORD],w.HANDLE),
            'Process32FirstW':([w.HANDLE,ctypes.POINTER(Entry)],w.BOOL),
            'Process32NextW':([w.HANDLE,ctypes.POINTER(Entry)],w.BOOL),
        }
        for name,(args,result) in signatures.items():
            method=getattr(self.dll,name);method.argtypes=args;method.restype=result

    def parent(self, pid):
        snapshot=self.dll.CreateToolhelp32Snapshot(2,0)  # TH32CS_SNAPPROCESS
        if snapshot in (None,ctypes.c_void_p(-1).value): raise RuntimeError(ERROR)
        self.snapshots.append(snapshot)
        try:
            entry=Entry();entry.size=ctypes.sizeof(entry)
            found=self.dll.Process32FirstW(snapshot,ctypes.byref(entry))
            # Bound traversal. No other record fields, names or IDs are retained.
            for _ in range(65536):
                if not found: raise RuntimeError(ERROR)
                if entry.pid == pid: return int(entry.parent)
                found=self.dll.Process32NextW(snapshot,ctypes.byref(entry))
            raise RuntimeError(ERROR)
        finally:
            primary=sys.exception()
            if self.dll.CloseHandle(snapshot):
                self.snapshots.remove(snapshot)
            elif primary is None or isinstance(primary,Exception):
                raise RuntimeError(ERROR)
            # A failed inverse remains retained; never mask cancellation with it.

    def open(self,pid):
        handle=self.dll.OpenProcess(RIGHTS,False,pid)
        if not handle: raise RuntimeError(ERROR)
        return handle

    def metadata(self,handle):
        pid=int(self.dll.GetProcessId(handle))
        times=[w.FILETIME() for _ in range(4)]
        if not pid or not self.dll.GetProcessTimes(handle,*(ctypes.byref(t) for t in times)): raise RuntimeError(ERROR)
        size=w.DWORD(32768);image=ctypes.create_unicode_buffer(size.value)
        if not self.dll.QueryFullProcessImageNameW(handle,0,image,ctypes.byref(size)): raise RuntimeError(ERROR)
        if not 0 < size.value < 32768: raise RuntimeError(ERROR)
        return pid,(times[0].dwHighDateTime<<32)|times[0].dwLowDateTime,Path(image.value).resolve()

    def wait(self,handle,milliseconds):
        result=self.dll.WaitForSingleObject(handle,milliseconds)
        if result==258: return False  # WAIT_TIMEOUT, not completion.
        if result!=0: raise RuntimeError(ERROR)
        return True

    def exit_code(self,handle):
        code=w.DWORD()
        if not self.dll.GetExitCodeProcess(handle,ctypes.byref(code)): raise RuntimeError(ERROR)
        return int(code.value)

    def close(self,handle):
        if not self.dll.CloseHandle(handle): raise RuntimeError(ERROR)

    def clean(self): return not self.snapshots


def _pid(value): return type(value) is int and 0 < value <= 0xffffffff


class ProcessLease:
    """Two independently opened, noninheritable query/wait handles; no kill rights.

    Launcher Popen ownership is not replaced. Acquisition failure keeps acquired
    handles for explicit retirement/release by the retained caller. No destructor,
    no implicit replay and no admission based on process names or absence.
    """
    def __init__(self,launcher,candidate,image,*,api=None):
        if type(launcher) is not subprocess.Popen or not _pid(launcher.pid) or not _pid(candidate) or not isinstance(image,Path):
            raise RuntimeError(ERROR)
        self.launcher,self.launcher_pid,self.candidate=launcher,launcher.pid,candidate
        self.image=image.resolve()
        self.api=WindowsProcessAPI() if api is None else api  # Models/owned fixtures only.
        self.handles=[];self.exits={};self.close_attempted=set();self.closed=set()
        self.attempted=self.acquired=self.failed=self.released=False

    def __repr__(self): return '<ProcessLease redacted>'

    def _launcher_live(self):
        if self.launcher.pid!=self.launcher_pid or self.launcher.poll() is not None: raise RuntimeError(ERROR)

    def acquire(self):
        if self.attempted or self.released: raise RuntimeError(ERROR)
        self.attempted=True
        try:
            self._launcher_live()
            if self.candidate!=self.launcher_pid and self.api.parent(self.candidate)!=self.launcher_pid:
                raise RuntimeError(ERROR)  # Refuse unrelated IDs BEFORE OpenProcess.
            root=self.api.open(self.launcher_pid);self.handles.append(root)
            child=root
            if self.candidate!=self.launcher_pid:
                child=self.api.open(self.candidate);self.handles.append(child)
            rpid,born,root_image=self.api.metadata(root)
            cpid,created,image=self.api.metadata(child)
            if (not _pid(rpid) or not _pid(cpid) or rpid!=self.launcher_pid or cpid!=self.candidate
                    or root_image!=self.image or image!=self.image
                    or type(born) is not int or type(created) is not int or not 0 < born <= created): raise RuntimeError(ERROR)
            if self.candidate!=self.launcher_pid and self.api.parent(self.candidate)!=self.launcher_pid: raise RuntimeError(ERROR)
            self._launcher_live()
            if any(self.api.wait(h,0) is not False for h in self.handles) or not self.api.clean(): raise RuntimeError(ERROR)
            self.acquired=True
        except BaseException:
            self.failed=True
            raise

    def observe(self):
        """Nonblocking observation of every handle; the caller owns any deadline."""
        if self.released: raise RuntimeError(ERROR)
        try:
            for handle in self.handles:
                if handle in self.exits: continue
                signaled=self.api.wait(handle,0)
                if type(signaled) is not bool: raise RuntimeError(ERROR)
                if signaled:
                    code=self.api.exit_code(handle)
                    if type(code) is not int or not 0 <= code <= 0xffffffff: raise RuntimeError(ERROR)
                    self.exits[handle]=code
            return bool(self.handles) and len(self.exits)==len(self.handles)
        except BaseException:
            self.failed=True
            raise

    def release(self):
        """Release only signaled/queried handles; uncertain CloseHandle never replays."""
        if self.released: return
        if not self.handles or len(self.exits)!=len(self.handles): raise RuntimeError(ERROR)
        errors=[]
        for handle in reversed(self.handles):
            if handle in self.close_attempted: continue
            self.close_attempted.add(handle)
            try:
                self.api.close(handle)
                self.closed.add(handle)
            except BaseException as error:
                self.failed=True;errors.append(error)
        self.released=len(self.closed)==len(self.handles) and self.api.clean()
        if errors: raise next((error for error in errors if not isinstance(error,Exception)),errors[0])
        if not self.released: raise RuntimeError(ERROR)

    def cleanup_complete(self):
        return self.api.clean() and (not self.handles or self.released)

    def receipt(self):
        if not self.acquired or not self.released or self.failed or any(self.exits.values()): raise RuntimeError(ERROR)
        return {'version':1,'qualification':False,'topology':'same-process' if self.candidate==self.launcher_pid else 'direct-child',
                'launcher_pid':self.launcher_pid,'parent_pid':self.candidate,
                'process_handles_waited':len(self.exits),'handles_released':True,'exit_code':0}
