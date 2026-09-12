"""Compiler-owner models: all node.exe/image inputs are non-executable bytes."""
from contextlib import contextmanager, ExitStack
from dataclasses import replace
import os
from pathlib import Path
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from qualification import parent_build as build


@contextmanager
def model(*, code=0, fault=None, interrupt=False, unknown=False):
    assert fault is None or (type(fault) is tuple and len(fault)==2 and type(fault[0]) is str
                             and type(fault[1]) is int and fault[1]>0)
    with tempfile.TemporaryDirectory(prefix='parent-build-model-') as temporary, ExitStack() as stack:
        root=Path(temporary).resolve()
        stack.enter_context(patch.object(build,'ARTIFACTS',root))
        stack.enter_context(patch.object(build,'ROOT',root))
        plan=build.DomainPlan.record(root,root);plan.create()
        image=root/'non-executable-image';image.write_bytes(b'non-executable model image\n')
        node=root/'node.exe';node.write_bytes(b'non-executable model compiler\n')
        for name in build.BUILD_SOURCES:
            path=root/name;path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(b'metadata-only compiler input\n')
        pins=build.BuildPins('a'*40,True,tuple((name,build.inputs.digest((root/name).read_bytes())) for name in build.BUILD_SOURCES),
                             node,build.file_sha256(node),image,build.file_sha256(image),'11111111-1111-4111-8111-111111111111')
        owner=build.ParentBuild(plan,pins)
        events=[];waits=[];calls=[];reads=[]
        def git(command,**_):
            return pins.commit+'\n' if command==['git','rev-parse','HEAD'] else b'?? model.py\n'
        stack.enter_context(patch.object(build.subprocess,'check_output',side_effect=git))
        original=owner._inputs
        def inputs():
            reads.append(True)
            if fault==('inputs',len(reads)):raise RuntimeError('modeled changed input')
            return original()
        owner._inputs=inputs
        class Process:
            pid=101
            def wait(self,timeout):
                assert owner.process is self
                waits.append(timeout);events.append('wait')
                if interrupt:raise KeyboardInterrupt()
                if fault==('wait',len(waits)):raise subprocess.TimeoutExpired('model',timeout)
                return code
        def start(args,**kwargs):
            assert owner.start_attempted
            calls.append((args,kwargs));events.append('start')
            if unknown:raise RuntimeError('modeled uncertain start')
            probe=plan.path/'probe';probe.mkdir()
            (probe/'probe.json').write_bytes(b'{"modeled":true}\n')
            return Process()
        def pack(domain,expected,**kwargs):
            assert owner.joined and type(owner.exit_code) is int and owner.exit_code==0
            events.append('pack')
            if fault==('pack',1):raise RuntimeError('modeled archive refusal')
            return domain/'probe'/build.inputs.ARCHIVE,{'xpi_sha256':'e'*64,'qualification':False}
        def inspect(*args,**kwargs):
            events.append('inspect')
            if fault==('inspect',1):return {'xpi_sha256':'f'*64,'qualification':False}
            return {'xpi_sha256':'e'*64,'qualification':False}
        stack.enter_context(patch.object(build.subprocess,'Popen',side_effect=start))
        stack.enter_context(patch.object(build.inputs,'pack',side_effect=pack))
        stack.enter_context(patch.object(build.inputs,'inspect',side_effect=inspect))
        yield SimpleNamespace(owner=owner,pins=pins,plan=plan,events=events,waits=waits,calls=calls,root=root)


class ParentBuildTests(unittest.TestCase):
    def test_retained_compiler_precedes_archive_and_receipt(self):
        with model() as f:
            receipt=f.owner.execute()
            self.assertEqual(f.events,['start','wait','pack','inspect'])
            self.assertEqual(f.waits,[120]);self.assertTrue(f.owner.cleanup_complete())
            self.assertFalse(receipt['native_fixture_executed']);self.assertFalse(receipt['browser_executed'])
            self.assertFalse(receipt['qualification']);self.assertEqual(receipt['compiler_pid'],101)
            self.assertEqual((f.plan.path/'native/download-manager-native-host.exe').read_bytes(),f.pins.image.read_bytes())
            self.assertEqual(f.owner.expected.domain,f.plan.path)
            f.owner.cleanup();self.assertEqual(f.waits,[120])
            with self.assertRaises(RuntimeError):f.owner.execute()

    def test_minimal_environment_fixed_command_and_no_output_capture(self):
        with patch.dict(os.environ,{'NODE_OPTIONS':'--require unwanted','NODE_PATH':'unwanted',
                                   'NODE_V8_COVERAGE':'unwanted','ESBUILD_BINARY_PATH':'unwanted'}),model() as f:
            f.owner.execute();args,kw=f.calls[0]
            self.assertEqual(args,[str(f.pins.node),str(f.root/'scripts/build-parent-probe.mjs'),str(f.plan.path),f.pins.nonce])
            env=kw['env']
            self.assertEqual(set(env),{'SYSTEMROOT','WINDIR','PATH','HOME','USERPROFILE','LOCALAPPDATA','APPDATA',
                                       'TEMP','TMP','ESBUILD_WORKER_THREADS','ESBUILD_MAX_BUFFER'})
            self.assertEqual(env['ESBUILD_WORKER_THREADS'],'0')
            self.assertEqual(env['ESBUILD_MAX_BUFFER'],'16777216')
            for name in ('TEMP','TMP','HOME','USERPROFILE','LOCALAPPDATA','APPDATA'):
                self.assertTrue(Path(env[name]).is_relative_to(f.plan.path))
            self.assertEqual(os.environ['NODE_OPTIONS'],'--require unwanted')
            for name in ('stdin','stdout','stderr'):self.assertEqual(kw[name],subprocess.DEVNULL)
            self.assertEqual(kw['creationflags'],subprocess.CREATE_NO_WINDOW)

    def test_cleanup_before_execution_consumes_controller(self):
        with model() as f:
            self.assertTrue(f.owner.cleanup())
            with self.assertRaises(RuntimeError):f.owner.execute()
            self.assertFalse(f.calls)
            self.assertEqual({p.name for p in f.plan.path.iterdir()},{'creation.private.json'})

    def test_changed_domain_and_existing_entries_refuse_before_launch(self):
        with model() as f:
            f.plan.path=f.root/'changed'
            with self.assertRaises(RuntimeError):f.owner.execute()
            self.assertFalse(f.calls)
        with model() as f:
            (f.plan.path/'native').mkdir()
            with self.assertRaises(RuntimeError):f.owner.execute()
            self.assertFalse(f.calls)
            self.assertFalse((f.plan.path/'parent-build.private.json').exists())

    def test_external_pins_and_input_changes_are_not_self_reported_authority(self):
        for field in ('node_sha256','image_sha256','commit'):
            with model() as f:
                pins=replace(f.pins,**{field:'c'*(40 if field=='commit' else 64)})
                f.owner.pins=pins
                with self.assertRaises(RuntimeError):f.owner.execute()
                self.assertFalse(f.calls)
        for name in build.BUILD_SOURCES[-2:]:
            with model() as f:
                (f.root/name).write_bytes(b'changed compiler input\n')
                with self.assertRaises(RuntimeError):f.owner.execute()
                self.assertFalse(f.calls)

    def test_unknown_start_has_no_join_or_retry(self):
        with model(unknown=True) as f:
            with self.assertRaises(RuntimeError):f.owner.execute()
            self.assertTrue(f.owner.start_attempted);self.assertIsNone(f.owner.process)
            self.assertFalse(f.owner.cleanup_complete())
            with self.assertRaises(RuntimeError):f.owner.cleanup()
            with self.assertRaises(RuntimeError):f.owner.execute()
            self.assertEqual(len(f.calls),1)
            with self.assertRaises(RuntimeError):f.owner.receipt()

    def test_timeout_then_join_cannot_restore_success(self):
        with model(fault=('wait',1)) as f:
            with self.assertRaises(subprocess.TimeoutExpired):f.owner.execute()
            self.assertFalse(f.owner.cleanup_complete());self.assertNotIn('pack',f.events)
            self.assertTrue(f.owner.cleanup());self.assertEqual(f.waits,[120,120])
            self.assertTrue(f.owner.failed)
            with self.assertRaises(RuntimeError):f.owner.receipt()
            with self.assertRaises(RuntimeError):f.owner.execute()

    def test_wait_boolean_nonzero_and_interruption_refuse(self):
        for code in (False,None,2):
            with model(code=code) as f:
                with self.assertRaises(RuntimeError):f.owner.execute()
                self.assertNotIn('pack',f.events)
                self.assertEqual(f.owner.joined,type(code) is int)
                with self.assertRaises(RuntimeError):f.owner.receipt()
        with model(interrupt=True) as f:
            with self.assertRaises(KeyboardInterrupt):f.owner.execute()
            self.assertTrue(f.owner.failed);self.assertIsNotNone(f.owner.process)
            self.assertFalse(f.owner.cleanup_complete())

    def test_post_build_changes_and_archive_disagreement_refuse_promotion(self):
        for fault in (('inputs',2),('inputs',3),('inputs',4),('pack',1),('inspect',1)):
            with self.subTest(fault=fault),model(fault=fault) as f:
                with self.assertRaises(RuntimeError):f.owner.execute()
                self.assertIsNone(f.owner.expected);self.assertIsNone(f.owner.archive_sha256)
                self.assertFalse(f.owner.stage=='complete')
                f.owner.cleanup()
                with self.assertRaises(RuntimeError):f.owner.receipt()

    def test_late_receipt_failure_revokes_outputs_but_retains_compiler(self):
        with model() as f:
            with patch.object(f.owner,'receipt',side_effect=RuntimeError('modeled receipt failure')):
                with self.assertRaises(RuntimeError):f.owner.execute()
            self.assertTrue(f.owner.joined);self.assertIsNotNone(f.owner.process)
            self.assertIsNone(f.owner.expected);self.assertIsNone(f.owner.archive_sha256)
            self.assertTrue(f.owner.failed)
            with self.assertRaises(RuntimeError):f.owner.receipt()

    def test_closed_pin_schema(self):
        with model() as f:
            for pins in (replace(f.pins,source_dirty=0),replace(f.pins,sources=dict(f.pins.sources)),
                         replace(f.pins,sources=f.pins.sources[::-1]),replace(f.pins,nonce='invalid')):
                with self.assertRaises(RuntimeError):build.ParentBuild(f.plan,pins)
            self.assertFalse(f.calls)

if __name__=='__main__':unittest.main()
