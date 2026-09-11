"""Closed, separately built fileless privileged probe; never a product XPI policy."""
import hashlib
import io
import json
from pathlib import Path
import re
import subprocess
import uuid
import zipfile

from .installed import ROOT, ordinary
from .support import ARTIFACTS
from .xpi_policy import unique_object

SOURCES = {name: ROOT / 'extension/protection-probe' / name for name in ('api.js', 'schema.json', 'probe.html', 'probe.js')}
SOURCES['LICENSE.txt'] = ROOT / 'LICENSE'
PAYLOADS = set(SOURCES) | {'manifest.json'}
ARCHIVE = 'download-protection-probe.xpi'
LIMIT = 64 * 1024


def manifest(addon):
    if not isinstance(addon, str) or not re.fullmatch(r'protection-[a-f0-9]{32}@download-manager.invalid', addon):
        raise RuntimeError('probe addon identity refused')
    return {'manifest_version': 3, 'name': 'Download protection service probe', 'version': '0.1.0',
            'browser_specific_settings': {'gecko': {'id': addon, 'strict_min_version': '156.0'}},
            'incognito': 'not_allowed',
            'content_security_policy': {'extension_pages': "default-src 'none'; script-src 'self'; object-src 'none'"},
            'experiment_apis': {'managerProtection': {'schema': 'schema.json', 'parent': {
                'scopes': ['addon_parent'], 'paths': [['managerProtection']], 'script': 'api.js'}}}}


def payload(path):
    ordinary(path)
    with path.open('rb') as source: data = source.read(LIMIT + 1)
    if not 0 < len(data) <= LIMIT: raise RuntimeError('probe source size refused')
    return data


def revision():
    return subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True, timeout=15).strip()


def build(directory):
    directory = Path(directory).absolute()
    ordinary(directory.parent)
    if not directory.is_relative_to(ARTIFACTS) or directory.exists(): raise RuntimeError('new artifacts probe directory required')
    commit = revision()
    files = {name: payload(path) for name, path in SOURCES.items()}
    addon = f'protection-{uuid.uuid4().hex}@download-manager.invalid'
    files['manifest.json'] = (json.dumps(manifest(addon), indent=2)+'\n').encode('utf-8')
    data = io.BytesIO()
    with zipfile.ZipFile(data, 'w', compression=zipfile.ZIP_STORED) as archive:
        for name, content in files.items(): archive.writestr(name, content)
    packed = data.getvalue()
    identity = {'version': 1, 'qualification': False, 'addon_id': addon, 'source_commit': commit,
                'source_dirty': bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT, timeout=15)),
                'xpi_sha256': hashlib.sha256(packed).hexdigest(),
                'files': {name: hashlib.sha256(content).hexdigest() for name, content in files.items()}}
    directory.mkdir()
    for name, content in {**files, ARCHIVE: packed, 'BUILD.json': (json.dumps(identity, indent=2)+'\n').encode('utf-8')}.items():
        with (directory/name).open('xb') as target: target.write(content)
    if revision() != commit or any(payload(path) != files[name] for name, path in SOURCES.items()):
        raise RuntimeError('probe source changed during build')
    inspect(directory, clean=False)
    return identity


def inspect(directory, *, clean=True):
    directory = Path(directory).absolute()
    ordinary(directory)
    if not directory.is_relative_to(ARTIFACTS): raise RuntimeError('probe must be beneath artifacts')
    if {p.name for p in directory.iterdir()} != PAYLOADS | {ARCHIVE, 'BUILD.json'}:
        raise RuntimeError('probe directory inventory refused')
    ordinary(directory/'BUILD.json')
    identity = json.loads(payload(directory/'BUILD.json'), object_pairs_hook=unique_object)
    if (not isinstance(identity, dict) or set(identity) != {'version','qualification','addon_id','source_commit','source_dirty','xpi_sha256','files'}
            or type(identity['version']) is not int or identity['version'] != 1 or identity['qualification'] is not False
            or type(identity['source_dirty']) is not bool or (clean and identity['source_dirty'])
            or not isinstance(identity['source_commit'],str) or not re.fullmatch('[a-f0-9]{40}',identity['source_commit'])
            or not isinstance(identity['xpi_sha256'],str) or not re.fullmatch('[a-f0-9]{64}',identity['xpi_sha256'])):
        raise RuntimeError('probe source identity refused')
    expected = manifest(identity['addon_id'])
    ordinary(directory/ARCHIVE)
    with (directory/ARCHIVE).open('rb') as source: data = source.read(6*LIMIT+1)
    if not 0 < len(data) <= 6*LIMIT or hashlib.sha256(data).hexdigest() != identity['xpi_sha256']:
        raise RuntimeError('probe archive identity refused')
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        entries = archive.infolist()
        if (len(entries) != 6 or {e.filename for e in entries} != PAYLOADS
                or any(e.flag_bits & 1 or (e.external_attr >> 16) & 0o170000 not in (0,0o100000)
                       or not 0 < e.file_size <= LIMIT for e in entries)):
            raise RuntimeError('probe archive inventory refused')
        files = {e.filename: archive.read(e) for e in entries}
    if json.loads(files['manifest.json'], object_pairs_hook=unique_object) != expected:
        raise RuntimeError('probe authority differs')
    if any(payload(path) != files[name] for name,path in SOURCES.items()): raise RuntimeError('probe differs from current reviewed source')
    hashes = {name: hashlib.sha256(content).hexdigest() for name,content in files.items()}
    if identity['files'] != hashes or any(payload(directory/name) != content for name,content in files.items()):
        raise RuntimeError('probe payload identity differs')
    return directory/ARCHIVE, identity
