"""Metadata-only archive fixtures. No compiler, executable, Firefox or registry work."""
from dataclasses import replace
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import uuid
import warnings
import zipfile

from qualification import parent_input as inputs
from qualification.support import ARTIFACTS


class ParentInputTests(unittest.TestCase):
    def setUp(self):
        ARTIFACTS.mkdir(exist_ok=True)
        temporary = tempfile.TemporaryDirectory(dir=ARTIFACTS, prefix='parent-input-model-')
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name).resolve()
        self.enterContext(patch.object(inputs, 'ARTIFACTS', root))
        self.domain = root / ('dm-installed-' + str(uuid.uuid4()))
        self.native = self.domain / 'native'
        self.probe = self.domain / 'probe'
        self.native.mkdir(parents=True)
        self.probe.mkdir()
        self.image = self.native / 'download-manager-native-host.exe'
        self.image.write_bytes(b'non-executable metadata fixture\n')
        native = {'name': inputs.HOST, 'type': 'stdio', 'path': str(self.image), 'allowed_extensions': [inputs.ADDON]}
        self.native_manifest = self.native / (inputs.HOST + '.json')
        self.native_manifest.write_text(json.dumps(native, indent=2, ensure_ascii=False) + '\n', encoding='utf-8', newline='\n')
        files = {'api.js': b'// modeled compiled payload; not an executable SDK observation\n',
                 'manifest.json': json.dumps(inputs.manifest()).encode('utf-8')}
        for name, source in [('background.js', 'extension/parent-probe/background.js'),
                             ('schema.json', 'extension/parent-probe/schema.json'), ('LICENSE.txt', 'LICENSE')]:
            files[name] = (inputs.ROOT / source).read_bytes()
        for name, data in files.items(): (self.probe / name).write_bytes(data)
        self.record = {'version': 1, 'qualification': False, 'addon_id': inputs.ADDON, 'nonce': str(uuid.uuid4()),
                       'fixture_image_sha256': inputs.digest(self.image.read_bytes()),
                       'files': {name: inputs.digest(data) for name, data in files.items()},
                       'sources': {name: inputs.digest((inputs.ROOT / name).read_bytes()) for name in inputs.SOURCES}}
        self.expected = inputs.BuildExpectation('a' * 40, False, self.domain, self.record['nonce'], self.record['fixture_image_sha256'], '0' * 64)
        self.write_record(self.record)
        self.dirty = False
        def git(command, **_):
            if command == ['git', 'rev-parse', 'HEAD']: return 'a' * 40 + '\n'
            self.assertEqual(command, ['git', 'status', '--porcelain'])
            return b'?? modeled-source.py\n' if self.dirty else b''
        self.enterContext(patch.object(inputs.subprocess, 'check_output', side_effect=git))

    def write_record(self, record, *, pin=True):
        raw = json.dumps(record).encode('utf-8') if not isinstance(record, bytes) else record
        (self.probe / 'probe.json').write_bytes(raw)
        if pin: self.expected = replace(self.expected, record_sha256=inputs.digest(raw))

    def rehash_payload(self, name, data):
        (self.probe / name).write_bytes(data)
        record = json.loads((self.probe / 'probe.json').read_bytes())
        record['files'][name] = inputs.digest(data)
        self.write_record(record)

    def test_five_member_exclusive_archive_independent_readback(self):
        archive, report = inputs.pack(self.domain, self.expected)
        self.assertEqual(report, inputs.inspect(self.domain, self.expected, report['xpi_sha256']))
        self.assertIs(report['qualification'], False)
        self.assertIs(report['source_dirty'], False)
        with zipfile.ZipFile(archive) as packed:
            self.assertEqual(set(packed.namelist()), inputs.PAYLOADS)
            self.assertEqual(len(packed.infolist()), 5)
            self.assertIsNone(packed.testzip())
            for entry in packed.infolist():
                self.assertEqual(entry.date_time, (1980, 1, 1, 0, 0, 0))
                self.assertEqual(packed.read(entry), (self.probe / entry.filename).read_bytes())
        before = archive.read_bytes()
        with self.assertRaises(RuntimeError): inputs.pack(self.domain, self.expected)
        self.assertEqual(archive.read_bytes(), before)

    def test_closed_expectations_domains_and_exact_boolean_flags(self):
        for field, value in [('commit', 'b' * 40), ('commit', True), ('nonce', 'not-a-uuid'),
                             ('source_dirty', 0), ('image_sha256', 'A' * 64), ('record_sha256', '0' * 64)]:
            with self.subTest(field=field, value=value), self.assertRaises(RuntimeError):
                inputs.pack(self.domain, replace(self.expected, **{field: value}))
        for clean in [None, 1, 'false']:
            with self.assertRaises(RuntimeError): inputs.pack(self.domain, self.expected, clean=clean)
        for domain in [str(self.domain), self.domain / 'child', self.domain.parent / 'other', self.domain / '..' / self.domain.name]:
            with self.assertRaises(RuntimeError): inputs.pack(domain, self.expected)
        self.assertFalse((self.probe / inputs.ARCHIVE).exists())

    def test_domain_must_match_the_retained_compiler_expectation(self):
        other = self.domain.parent / ('dm-installed-' + str(uuid.uuid4()))
        with self.assertRaisesRegex(RuntimeError, 'domain'):
            inputs.pack(self.domain, replace(self.expected, domain=other))

    def test_dirty_build_cannot_be_silently_relabelled_clean(self):
        self.dirty = True
        with self.assertRaises(RuntimeError): inputs.pack(self.domain, self.expected, clean=False)
        self.expected = replace(self.expected, source_dirty=True)
        with self.assertRaises(RuntimeError): inputs.pack(self.domain, self.expected)
        _, report = inputs.pack(self.domain, self.expected, clean=False)
        self.assertIs(report['source_dirty'], True)
        with self.assertRaises(RuntimeError): inputs.inspect(self.domain, self.expected, report['xpi_sha256'])
        self.dirty = False
        with self.assertRaises(RuntimeError): inputs.inspect(self.domain, self.expected, report['xpi_sha256'], clean=False)

    def test_rehashed_closed_metadata_and_duplicate_members_still_refuse(self):
        for field, value in [('version', True), ('qualification', True), ('addon_id', 'other@invalid'),
                             ('nonce', str(uuid.uuid4())), ('fixture_image_sha256', '0' * 64),
                             ('sources', {}), ('extra', False), ('files', {})]:
            self.write_record({**self.record, field: value})
            with self.subTest(field=field), self.assertRaises(RuntimeError): inputs.pack(self.domain, self.expected)
        for raw in [b'{"version":1,"version":1}', b'{"version":NaN}', b'\xff', b'[]']:
            self.write_record(raw)
            with self.assertRaises(RuntimeError): inputs.pack(self.domain, self.expected)

    def test_self_rehashing_cannot_replace_controller_build_expectation(self):
        original = self.expected
        self.rehash_payload('api.js', b'changed modeled bundle')
        with self.assertRaisesRegex(RuntimeError, 'build identity'):
            inputs.pack(self.domain, original)
        self.assertFalse((self.probe / inputs.ARCHIVE).exists())

    def test_images_manifest_source_and_payload_bounds_refuse(self):
        for path in [self.image, self.native_manifest, self.probe / 'api.js']:
            original = path.read_bytes()
            for raw in [b'', b'changed metadata']:
                path.write_bytes(raw)
                with self.assertRaises(RuntimeError): inputs.pack(self.domain, self.expected)
            path.write_bytes(original)
        raw = (self.probe / 'api.js').read_bytes()
        self.rehash_payload('api.js', b'x' * (inputs.LIMIT + 1))
        with self.assertRaisesRegex(RuntimeError, 'bound'): inputs.pack(self.domain, self.expected)
        self.rehash_payload('api.js', raw)
        original_reader = inputs._bytes
        def changed_source(path, limit=inputs.LIMIT):
            return b'modeled source replacement' if path == inputs.ROOT / inputs.SOURCES[0] else original_reader(path, limit)
        with patch.object(inputs, '_bytes', side_effect=changed_source), self.assertRaisesRegex(RuntimeError, 'source bytes'):
            inputs.pack(self.domain, self.expected)
        (self.probe / 'extra').write_bytes(b'owned')
        with self.assertRaisesRegex(RuntimeError, 'inventory'): inputs.pack(self.domain, self.expected)

    def test_rehashed_authority_and_selected_source_expansion_refuse(self):
        for field, value in [('host_permissions', ['<all_urls>']), ('permissions', ['nativeMessaging', 'webRequest']),
                             ('background', {'scripts': ['background.js'], 'persistent': True}), ('incognito', 'spanning')]:
            self.rehash_payload('manifest.json', json.dumps({**inputs.manifest(), field: value}).encode('utf-8'))
            with self.subTest(field=field), self.assertRaisesRegex(RuntimeError, 'authority'):
                inputs.pack(self.domain, self.expected)
        self.rehash_payload('manifest.json', json.dumps(inputs.manifest()).encode('utf-8'))
        self.rehash_payload('background.js', b'// different modeled background')
        with self.assertRaisesRegex(RuntimeError, 'selected source'): inputs.pack(self.domain, self.expected)

    def test_archive_external_pin_shape_and_actual_members_are_independent_checks(self):
        path, report = inputs.pack(self.domain, self.expected)
        with zipfile.ZipFile(path) as archive: files = {entry.filename: archive.read(entry) for entry in archive.infolist()}
        for kind in ['extra', 'duplicate', 'symlink', 'compressed', 'empty', 'changed', 'comment']:
            output = io.BytesIO()
            with warnings.catch_warnings(), zipfile.ZipFile(output, 'w') as archive:
                warnings.simplefilter('ignore', UserWarning)
                for name, content in files.items():
                    entry = zipfile.ZipInfo(name)
                    entry.create_system = 3
                    entry.external_attr = 0o100644 << 16
                    if name == 'api.js':
                        if kind == 'symlink': entry.external_attr = 0o120777 << 16
                        if kind == 'compressed': entry.compress_type = zipfile.ZIP_DEFLATED
                        if kind == 'empty': content = b''
                        if kind == 'changed': content = b'changed modeled bundle'
                    archive.writestr(entry, content)
                if kind in ['extra', 'duplicate']: archive.writestr('extra' if kind == 'extra' else 'api.js', b'owned')
                if kind == 'comment': archive.comment = b'owned'
            raw = output.getvalue(); path.write_bytes(raw)
            with self.subTest(kind=kind):
                with self.assertRaisesRegex(RuntimeError, 'archive identity'):
                    inputs.inspect(self.domain, self.expected, report['xpi_sha256'])
                with self.assertRaises(RuntimeError): inputs.inspect(self.domain, self.expected, inputs.digest(raw))

    def test_manifest_boolean_cannot_be_replaced_with_numeric_zero(self):
        manifest = inputs.manifest()
        manifest['background']['persistent'] = 0
        self.rehash_payload('manifest.json', json.dumps(manifest).encode('utf-8'))
        with self.assertRaisesRegex(RuntimeError, 'authority'): inputs.pack(self.domain, self.expected)

    def test_archive_appearing_after_validation_is_not_overwritten(self):
        original = inputs._inputs
        path = self.probe / inputs.ARCHIVE
        foreign = b'owned model of a concurrently created archive'
        def checked(*args, **kwargs):
            result = original(*args, **kwargs)
            if not kwargs['archived']:
                with path.open('xb') as output: output.write(foreign)
            return result
        with patch.object(inputs, '_inputs', side_effect=checked):
            with self.assertRaises(FileExistsError): inputs.pack(self.domain, self.expected)
        self.assertEqual(path.read_bytes(), foreign)

    def test_input_change_during_archive_read_refuses_after_exclusive_write(self):
        original_read = zipfile.ZipFile.read
        changed = False
        def read(archive, *args, **kwargs):
            nonlocal changed
            value = original_read(archive, *args, **kwargs)
            if not changed:
                changed = True
                self.image.write_bytes(b'modeled image changed during readback')
            return value
        with patch.object(zipfile.ZipFile, 'read', read), self.assertRaisesRegex(RuntimeError, 'image identity'):
            inputs.pack(self.domain, self.expected)
        self.assertTrue(changed)
        self.assertTrue((self.probe / inputs.ARCHIVE).exists())


if __name__ == '__main__': unittest.main()
