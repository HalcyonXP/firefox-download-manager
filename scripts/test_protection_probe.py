"""Build/input/ownership policies only. No Firefox, installation or live service calls."""
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile
from types import SimpleNamespace

from qualification import protection_input as inputs
from qualification import protection_run as driver
from qualification.support import ARTIFACTS


class ProtectionProbeTests(unittest.TestCase):
    def setUp(self):
        ARTIFACTS.mkdir(exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(dir=ARTIFACTS, prefix='protection-model-')
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name).resolve()/'probe'

    def test_build_six_payloads_only_no_general_authority_or_overwrite(self):
        identity = inputs.build(self.directory)
        xpi, read = inputs.inspect(self.directory,clean=False)
        self.assertEqual(read, identity)
        self.assertFalse(read['qualification'])
        with zipfile.ZipFile(xpi) as archive:
            self.assertEqual(set(archive.namelist()), inputs.PAYLOADS)
            self.assertIsNone(archive.testzip())
            manifest = json.loads(archive.read('manifest.json'))
        self.assertNotIn('permissions',manifest)
        self.assertNotIn('host_permissions',manifest)
        self.assertNotIn('background',manifest)
        self.assertEqual(manifest['incognito'],'not_allowed')
        self.assertEqual(list(manifest['experiment_apis']),['managerProtection'])
        with self.assertRaises(RuntimeError): inputs.build(self.directory)

    def test_dirty_wrong_version_extra_files_and_source_changes_refuse(self):
        inputs.build(self.directory)
        record = self.directory/'BUILD.json'; original = record.read_bytes()
        for key,value in [('source_dirty',True),('version',True),('qualification',True),('addon_id','other@invalid')]:
            data=json.loads(original);data[key]=value;record.write_text(json.dumps(data),encoding='utf-8')
            with self.assertRaises(RuntimeError): inputs.inspect(self.directory)
        record.write_bytes(original)
        (self.directory/'extra').write_bytes(b'owned')
        with self.assertRaises(RuntimeError): inputs.inspect(self.directory,clean=False)
        (self.directory/'extra').unlink()
        (self.directory/'api.js').write_bytes(b'changed')
        with self.assertRaises(RuntimeError): inputs.inspect(self.directory,clean=False)

    def test_rehashed_archive_cannot_expand_authority_or_replace_code(self):
        inputs.build(self.directory)
        original=(self.directory/inputs.ARCHIVE).read_bytes()
        identity=json.loads((self.directory/'BUILD.json').read_text(encoding='utf-8'))
        for kind in ['permission','source','duplicate']:
            with zipfile.ZipFile(io.BytesIO(original)) as archive:
                files={name:archive.read(name) for name in archive.namelist()}
            if kind=='permission':
                manifest=json.loads(files['manifest.json']);manifest['permissions']=['nativeMessaging']
                files['manifest.json']=json.dumps(manifest).encode('utf-8')
            if kind=='source': files['api.js']=b'changed'
            for name,data in files.items(): (self.directory/name).write_bytes(data)
            packed=io.BytesIO()
            with zipfile.ZipFile(packed,'w') as archive:
                for name,data in files.items(): archive.writestr(name,data)
                if kind=='duplicate': archive.writestr('extra.js',b'owned')
            raw=packed.getvalue();(self.directory/inputs.ARCHIVE).write_bytes(raw)
            record={**identity,'xpi_sha256':hashlib.sha256(raw).hexdigest(),'files':{n:hashlib.sha256(b).hexdigest() for n,b in files.items()}}
            (self.directory/'BUILD.json').write_text(json.dumps(record),encoding='utf-8')
            with self.assertRaises(RuntimeError): inputs.inspect(self.directory,clean=False)

    def test_receipt_is_not_boolean_or_missing_callback_acceptance(self):
        valid={'version':1,'qualification':False,'scope':'fixed-empty-loopback-text','stage':'settled',
               'result':'not-blocked','attempted':True,'callbacks':1}
        self.assertEqual(driver.valid_receipt(valid),valid)
        for key,value in [('version',True),('qualification',True),('stage','pending'),('result','unavailable'),
                          ('result','blocked'),('callbacks',True),('callbacks',0),('callbacks',2),('attempted',False)]:
            with self.assertRaises(RuntimeError): driver.valid_receipt({**valid,key:value})
        with self.assertRaises(RuntimeError): driver.valid_receipt({**valid,'path':'unowned'})

    def test_failed_browser_start_retains_owner_and_failed_close_does_not_drop_it(self):
        class Browser:
            closed=False
            def start(self): raise RuntimeError('owned synthetic start failure')
            def close(self): raise RuntimeError('owned synthetic close failure')
        browser=Browser()
        run=driver.ProtectionRun(self.directory,Path('unused'),self.directory/'report.json')
        with patch.object(driver,'preflight'),patch.object(driver,'Firefox',return_value=browser):
            with self.assertRaises(RuntimeError):run.open(self.directory/'profile',{})
        self.assertIs(run.browser,browser);self.assertFalse(run.retire());self.assertIs(run.browser,browser)

    def test_joined_failure_or_absence_cannot_be_success(self):
        class Process:
            def __init__(self, code): self.code=code
            def wait(self, timeout):
                self.assertion = timeout == 0
                return self.code
        for browser in [None,SimpleNamespace(closed=False,process=Process(0)),
                        SimpleNamespace(closed=True,process=None),SimpleNamespace(closed=True,process=Process(1))]:
            with self.assertRaises(RuntimeError):driver.require_joined(browser)
        process=Process(0);driver.require_joined(SimpleNamespace(closed=True,process=process))
        self.assertTrue(process.assertion)

    def test_initial_page_is_not_a_false_refusal_and_no_driver_preference_setters(self):
        page=inputs.SOURCES['probe.html'].read_text(encoding='utf-8')
        self.assertIn('id="receipt">starting',page)
        self.assertNotIn('id="receipt">unavailable',page)
        self.assertNotIn('setBoolPref',driver.PROTECTIONS)
        self.assertNotIn('setCharPref',driver.PROTECTIONS)
        self.assertIn('getPrefType',driver.PROTECTIONS)
        for key in ['xpinstall.signatures.required','extensions.experiments.enabled','browser.safebrowsing.downloads.enabled']:
            self.assertIn(key,driver.PROTECTIONS)


if __name__=='__main__':unittest.main()
