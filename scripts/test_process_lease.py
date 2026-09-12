"""Pure handle models. No process, image, profile or registry is opened here."""
from pathlib import Path
import subprocess
import unittest
from types import SimpleNamespace
from qualification.process_lease import ProcessLease, WindowsProcessAPI

IMAGE=Path(__file__).resolve().parent/'non-executable-process-model'

class API:
    def __init__(self):
        self.parents=[11,11];self.opened=[];self.signaled=set();self.closed=[]
        self.codes={11:0,12:0};self.close_failure=None;self.dirty=False
        self.records={11:(11,100,IMAGE),12:(12,200,IMAGE)}
    def parent(self,pid):
        assert pid==12
        return self.parents.pop(0)
    def open(self,pid): self.opened.append(pid);return pid
    def metadata(self,h): return self.records[h]
    def wait(self,h,ms): return h in self.signaled
    def exit_code(self,h):return self.codes[h]
    def close(self,h):
        self.closed.append(h)
        if h==self.close_failure:raise RuntimeError('modeled close failure')
    def clean(self):return not self.dirty


def model(candidate=12):
    # Exact Popen type, but deliberately NO OS creation or handle ownership.
    root=object.__new__(subprocess.Popen);root._child_created=False
    root.pid=11;root.poll=lambda:None
    api=API();owner=ProcessLease(root,candidate,IMAGE,api=api)
    return owner,api,root


class ProcessLeaseTests(unittest.TestCase):
    def test_child_wait_is_distinct_from_launcher_wait_and_release(self):
        owner,api,_=model();owner.acquire()
        api.signaled.add(12)
        self.assertFalse(owner.observe())
        self.assertEqual(owner.exits,{12:0})
        with self.assertRaises(RuntimeError):owner.release()
        with self.assertRaises(RuntimeError):owner.receipt()
        api.signaled.add(11)
        self.assertTrue(owner.observe());owner.release();owner.release()
        self.assertEqual(api.closed,[12,11])
        self.assertEqual(owner.receipt()['process_handles_waited'],2)
        self.assertEqual(owner.receipt()['topology'],'direct-child')
        self.assertNotIn(str(IMAGE),repr(owner))

    def test_same_process_uses_one_handle(self):
        owner,api,_=model(11);owner.acquire()
        self.assertEqual(api.opened,[11])
        api.signaled.add(11);self.assertTrue(owner.observe());owner.release()
        self.assertEqual(owner.receipt()['topology'],'same-process')

    def test_foreign_or_changed_parent_refuses_without_adoption(self):
        for parents,opened in (([99],[]),([11,99],[11,12])):
            owner,api,_=model();api.parents=parents
            with self.assertRaises(RuntimeError):owner.acquire()
            self.assertEqual(api.opened,opened)
            self.assertFalse(owner.acquired);self.assertTrue(owner.failed)
            with self.assertRaises(RuntimeError):owner.acquire()
            if opened:
                api.signaled.update(opened);owner.observe();owner.release()
                self.assertTrue(owner.released)
                with self.assertRaises(RuntimeError):owner.receipt()

    def test_wrong_identity_old_child_or_changed_image_retains_handles(self):
        cases=[(11,(99,100,IMAGE)),(12,(13,200,IMAGE)),(12,(12,99,IMAGE)),
               (11,(11,0,IMAGE)),(12,(12,True,IMAGE)),(11,(11,100,Path('other'))),
               (12,(12,200,Path('other')))]
        for handle,metadata in cases:
            with self.subTest(metadata=metadata):
                owner,api,_=model();api.records[handle]=metadata
                with self.assertRaises(RuntimeError):owner.acquire()
                self.assertEqual(owner.handles,[11,12]);self.assertFalse(api.closed)
                api.signaled.update((11,12));owner.observe();owner.release()
                with self.assertRaises(RuntimeError):owner.receipt()

    def test_launcher_loss_and_early_exit_are_not_readiness(self):
        owner,api,root=model();root.poll=lambda:0
        with self.assertRaises(RuntimeError):owner.acquire()
        self.assertFalse(api.opened)
        owner,api,_=model();api.signaled.add(12)
        with self.assertRaises(RuntimeError):owner.acquire()
        self.assertEqual(owner.handles,[11,12])

    def test_wait_errors_invalid_types_and_failed_exit_never_succeed(self):
        for value in (None,0,1):
            owner,api,_=model();owner.acquire();api.wait=lambda h,ms:value
            with self.assertRaises(RuntimeError):owner.observe()
            self.assertFalse(owner.exits)
        for code in (2,False,None):
            owner,api,_=model();owner.acquire();api.signaled.update((11,12));api.codes[12]=code
            if type(code) is not int:
                with self.assertRaises(RuntimeError):owner.observe()
            else:
                owner.observe();owner.release()
                with self.assertRaises(RuntimeError):owner.receipt()

    def test_failed_close_attempt_is_retained_without_replay(self):
        owner,api,_=model();owner.acquire();api.signaled.update((11,12));owner.observe()
        api.close_failure=12
        with self.assertRaises(RuntimeError):owner.release()
        self.assertEqual(api.closed,[12,11]);self.assertEqual(owner.closed,{11})
        with self.assertRaises(RuntimeError):owner.release()
        self.assertEqual(api.closed,[12,11]);self.assertFalse(owner.released)
        with self.assertRaises(RuntimeError):owner.receipt()

    def test_partial_open_or_interrupted_metadata_can_retire_but_not_qualify(self):
        for partial in (True,False):
            owner,api,_=model();original=api.open
            if partial:
                def open_handle(pid):
                    if pid==12:raise RuntimeError('modeled open refusal')
                    return original(pid)
                api.open=open_handle;error=RuntimeError
            else:
                def metadata(_):raise KeyboardInterrupt()
                api.metadata=metadata;error=KeyboardInterrupt
            with self.assertRaises(error):owner.acquire()
            self.assertEqual(owner.handles,[11] if partial else [11,12])
            api.signaled.update((11,12));owner.observe();owner.release()
            self.assertTrue(owner.released)
            with self.assertRaises(RuntimeError):owner.receipt()

    def test_cancellation_survives_multiple_failed_handle_inverses(self):
        owner,api,_=model();owner.acquire();api.signaled.update((11,12));owner.observe()
        def close(handle):
            api.closed.append(handle)
            if handle==12:raise RuntimeError('first inverse failed')
            raise KeyboardInterrupt()
        api.close=close
        with self.assertRaises(KeyboardInterrupt):owner.release()
        self.assertEqual(api.closed,[12,11]);self.assertFalse(owner.released)

    def test_snapshot_cancellation_and_failed_inverse_are_both_preserved(self):
        api=object.__new__(WindowsProcessAPI);api.snapshots=[];closes=[]
        def first(*_):raise KeyboardInterrupt()
        def close(handle):closes.append(handle);return False
        api.dll=SimpleNamespace(CreateToolhelp32Snapshot=lambda *_:44,Process32FirstW=first,CloseHandle=close)
        with self.assertRaises(KeyboardInterrupt):api.parent(12)
        self.assertEqual(api.snapshots,[44]);self.assertEqual(closes,[44]);self.assertFalse(api.clean())

    def test_snapshot_absence_is_refusal_not_an_absence_receipt(self):
        api=object.__new__(WindowsProcessAPI);api.snapshots=[];closes=[]
        def close(handle):closes.append(handle);return True
        api.dll=SimpleNamespace(CreateToolhelp32Snapshot=lambda *_:44,Process32FirstW=lambda *_:False,CloseHandle=close)
        with self.assertRaises(RuntimeError):api.parent(12)
        self.assertEqual(closes,[44]);self.assertTrue(api.clean())

    def test_dirty_backend_and_retired_owners_refuse_admission(self):
        owner,api,_=model();api.dirty=True
        with self.assertRaises(RuntimeError):owner.acquire()
        owner,api,_=model();owner.acquire();api.signaled.update((11,12));owner.observe();owner.release()
        with self.assertRaises(RuntimeError):owner.acquire()
        with self.assertRaises(RuntimeError):owner.observe()
        for candidate in (True,0,-1,2**32):
            _,api,root=model()
            with self.assertRaises(RuntimeError):ProcessLease(root,candidate,IMAGE,api=api)

if __name__=='__main__':unittest.main()
