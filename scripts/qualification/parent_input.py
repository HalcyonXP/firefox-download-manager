"""Closed parent-fixture archive integrity, not build provenance or execution authority.

Expected hashes MUST come from a separately retained, reviewed build controller;
reading them from the input under inspection would be circular. No browser,
registry, native fixture, preference or compiler is launched by this module.
"""
from dataclasses import dataclass
import hashlib
import io
import json
from pathlib import Path
import re
import subprocess
import zipfile

from .installed import ROOT, ordinary
from .support import ARTIFACTS, unique_object, invalid_constant

ADDON = 'download-manager@halcyonxp.local'
HOST = 'com.halcyonxp.firefox_download_manager'
ARCHIVE = 'parent-stdio-fixture.xpi'
LIMIT = 64 * 1024
IMAGE_LIMIT = 16 * 1024 * 1024
ARCHIVE_LIMIT = 6 * LIMIT
PAYLOADS = {'api.js', 'background.js', 'schema.json', 'manifest.json', 'LICENSE.txt'}
SOURCES = ('extension/parent-probe/api.js', 'extension/parent-probe/session.js',
           'extension/parent-probe/schema.json', 'extension/parent-probe/background.js',
           'extension/protection-bridge/parent-launcher.js',
           'extension/protection-bridge/native-transport.js', 'LICENSE')
UUID = r'[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}'


@dataclass(frozen=True)
class BuildExpectation:
    """Trusted controller values, NOT evidence that a compiler/process was joined."""
    commit: str
    source_dirty: bool
    domain: Path
    nonce: str
    image_sha256: str
    record_sha256: str


def digest(data):
    return hashlib.sha256(data).hexdigest()


def _match(value, pattern):
    return isinstance(value, str) and re.fullmatch(pattern, value) is not None


def _bytes(path, limit=LIMIT):
    ordinary(path)
    if not path.is_file() or getattr(path.stat(follow_symlinks=False), 'st_file_attributes', 0) & 0x400:
        raise RuntimeError('parent fixture input type refused')
    with path.open('rb') as source: data = source.read(limit + 1)
    if not 0 < len(data) <= limit: raise RuntimeError('parent fixture input bound refused')
    return data


def _json(data):
    try:
        return json.loads(data.decode('utf-8'), object_pairs_hook=unique_object,
                          parse_constant=invalid_constant)
    except (ValueError, RecursionError):
        raise RuntimeError('parent fixture metadata refused') from None


def manifest():
    return {'manifest_version': 3, 'name': 'Owned fileless parent stdio fixture', 'version': '0.0.1',
            'browser_specific_settings': {'gecko': {'id': ADDON, 'strict_min_version': '156.0'}},
            'incognito': 'not_allowed', 'permissions': ['nativeMessaging'],
            'background': {'scripts': ['background.js'], 'persistent': False},
            'content_security_policy': {'extension_pages': "default-src 'none'; script-src 'self'; object-src 'none'"},
            'experiment_apis': {'managerParentProbe': {'schema': 'schema.json', 'parent': {
                'scopes': ['addon_parent'], 'paths': [['managerParentProbe']], 'script': 'api.js'}}}}


def _inputs(domain, expected, *, archived, clean):
    if (not isinstance(domain, Path) or not isinstance(expected, BuildExpectation)
            or type(clean) is not bool or type(expected.source_dirty) is not bool
            or not _match(expected.commit, '[a-f0-9]{40}')
            or not _match(expected.nonce, UUID)
            or not all(_match(value, '[a-f0-9]{64}') for value in (expected.image_sha256, expected.record_sha256))):
        raise RuntimeError('parent fixture controller expectation refused')
    # The bundle contains absolute native paths. Its compiler domain cannot move.
    if domain != expected.domain or domain.parent != ARTIFACTS or not _match(domain.name, 'dm-installed-' + UUID):
        raise RuntimeError('parent fixture domain refused')
    ordinary(domain)
    probe = domain / 'probe'
    ordinary(probe)
    allowed = PAYLOADS | {'probe.json'} | ({ARCHIVE} if archived else set())
    if not probe.is_dir() or {p.name for p in probe.iterdir()} != allowed:
        raise RuntimeError('parent fixture inventory refused')
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True, encoding='utf-8', timeout=15).strip()
    dirty = bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT, timeout=15))
    if commit != expected.commit or dirty != expected.source_dirty or (clean and dirty):
        raise RuntimeError('parent fixture source state refused')
    raw = _bytes(probe / 'probe.json')
    if digest(raw) != expected.record_sha256: raise RuntimeError('parent fixture build identity refused')
    record = _json(raw)
    if (not isinstance(record, dict) or set(record) != {'version', 'qualification', 'addon_id', 'nonce', 'fixture_image_sha256', 'files', 'sources'}
            or type(record['version']) is not int or record['version'] != 1
            or record['qualification'] is not False or record['addon_id'] != ADDON
            or record['nonce'] != expected.nonce or record['fixture_image_sha256'] != expected.image_sha256):
        raise RuntimeError('parent fixture build record refused')
    if record['sources'] != {name: digest(_bytes(ROOT / name)) for name in SOURCES}:
        raise RuntimeError('parent fixture source bytes refused')
    command = domain / 'native' / 'download-manager-native-host.exe'
    if digest(_bytes(command, IMAGE_LIMIT)) != expected.image_sha256:
        raise RuntimeError('parent fixture image identity refused')
    native = {'name': HOST, 'type': 'stdio', 'path': str(command), 'allowed_extensions': [ADDON]}
    exact_native = (json.dumps(native, indent=2, ensure_ascii=False) + '\n').encode('utf-8')
    if _bytes(command.parent / (HOST + '.json')) != exact_native:
        raise RuntimeError('parent fixture native manifest refused')
    files = {name: _bytes(probe / name) for name in PAYLOADS}
    if record['files'] != {name: digest(data) for name, data in files.items()}:
        raise RuntimeError('parent fixture payload identity refused')
    # Python container equality aliases False and 0; compare typed JSON values.
    actual_manifest = json.dumps(_json(files['manifest.json']), sort_keys=True, ensure_ascii=False)
    if actual_manifest != json.dumps(manifest(), sort_keys=True, ensure_ascii=False):
        raise RuntimeError('parent fixture authority refused')
    for name, source in [('background.js', 'extension/parent-probe/background.js'),
                         ('schema.json', 'extension/parent-probe/schema.json'), ('LICENSE.txt', 'LICENSE')]:
        if files[name] != _bytes(ROOT / source): raise RuntimeError('parent fixture selected source refused')
    return files, dirty


def inspect(domain, expected, archive_sha256, *, clean=True):
    """Read back an externally pinned archive; self-reported hashes confer no trust."""
    if not _match(archive_sha256, '[a-f0-9]{64}'):
        raise RuntimeError('parent fixture archive expectation refused')
    files, dirty = _inputs(domain, expected, archived=True, clean=clean)
    raw = _bytes(domain / 'probe' / ARCHIVE, ARCHIVE_LIMIT)
    if digest(raw) != archive_sha256: raise RuntimeError('parent fixture archive identity refused')
    try:
        with zipfile.ZipFile(io.BytesIO(raw)) as archive:
            entries = archive.infolist()
            if (archive.comment or len(entries) != 5 or {entry.filename for entry in entries} != PAYLOADS
                    or any(entry.flag_bits & 1 or entry.compress_type != zipfile.ZIP_STORED
                           or entry.extra or entry.comment or entry.create_system != 3
                           or entry.external_attr >> 16 != 0o100644
                           or not 0 < entry.file_size <= LIMIT or entry.compress_size != entry.file_size
                           for entry in entries)):
                raise RuntimeError('parent fixture archive shape refused')
            if any(archive.read(entry) != files[entry.filename] for entry in entries):
                raise RuntimeError('parent fixture archive payload refused')
    except (zipfile.BadZipFile, ValueError, NotImplementedError):
        raise RuntimeError('parent fixture archive refused') from None
    # Recheck source/inputs after reading, not just before opening the archive.
    again, after_dirty = _inputs(domain, expected, archived=True, clean=clean)
    if again != files or after_dirty != dirty: raise RuntimeError('parent fixture inputs changed')
    return {'version': 1, 'qualification': False, 'source_commit': expected.commit,
            'source_dirty': dirty, 'nonce': expected.nonce, 'addon_id': ADDON,
            'fixture_image_sha256': expected.image_sha256, 'build_record_sha256': expected.record_sha256,
            'xpi_sha256': archive_sha256, 'files': {name: digest(data) for name, data in files.items()}}


def pack(domain, expected, *, clean=True):
    """Exclusive five-member archive only; no compiler, native process or browser."""
    files, _ = _inputs(domain, expected, archived=False, clean=clean)
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, 'w', compression=zipfile.ZIP_STORED) as archive:
        for name, data in sorted(files.items()):
            entry = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            entry.create_system = 3
            entry.external_attr = 0o100644 << 16
            archive.writestr(entry, data)
    raw = buffer.getvalue()
    if not 0 < len(raw) <= ARCHIVE_LIMIT: raise RuntimeError('parent fixture archive size refused')
    path = domain / 'probe' / ARCHIVE
    with path.open('xb') as output: output.write(raw)
    return path, inspect(domain, expected, digest(raw), clean=clean)
