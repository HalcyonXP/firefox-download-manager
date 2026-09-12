"""Closed manual parent-candidate archive policy; no compiler/browser/registration effects."""
import hashlib
import io
import json
import re
import zipfile
from .installed import ROOT, ordinary
from .support import ARTIFACTS
from .xpi_policy import unique_object

PAYLOADS = {'background.js', 'manager.js', 'manager.html', 'manager.css', 'manifest.json',
            'parent-api.js', 'parent-schema.json', 'LICENSE.txt', 'THIRD-PARTY-NOTICES.txt'}
ARCHIVE = 'download-manager-parent-candidate.xpi'
LIMIT = 1024*1024
ERROR = 'owned parent candidate input refused'


def read(path, limit):
    ordinary(path)
    with path.open('rb') as stream: data = stream.read(limit+1)
    if not 0 < len(data) <= limit: raise RuntimeError(ERROR)
    return data


def decode(data):
    return json.loads(data, object_pairs_hook=unique_object,
                      parse_constant=lambda _: (_ for _ in ()).throw(RuntimeError(ERROR)))


def authority(files):
    # Separate Python readback, not a trust decision based on Node's BUILD record.
    manifest = decode(files['manifest.json'])
    expected = decode((ROOT/'extension/src/manifest.json').read_bytes())
    expected.update(name='Download Manager parent transport candidate', version='0.3.0',
                    description='Development parent transport with manual downloads; automatic capture is unavailable.',
                    experiment_apis={'managerParentTransport': {'schema':'parent-schema.json', 'parent': {
                        'scopes':['addon_parent'], 'paths':[['managerParentTransport']], 'script':'parent-api.js'}}})
    if json.dumps(manifest, sort_keys=True) != json.dumps(expected, sort_keys=True): raise RuntimeError(ERROR)
    schema = decode(files['parent-schema.json'])
    expected_schema = decode((ROOT/'extension/parent-bridge/schema.json').read_bytes())
    if json.dumps(schema, sort_keys=True) != json.dumps(expected_schema, sort_keys=True): raise RuntimeError(ERROR)


def inspect(data):
    if type(data) is not bytes or not 0 < len(data) <= 10*LIMIT: raise RuntimeError(ERROR)
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        entries = archive.infolist()
        if (archive.comment or len(entries) != len(PAYLOADS) or {e.filename for e in entries} != PAYLOADS
                or any(e.compress_type != zipfile.ZIP_STORED or e.flag_bits or e.extra or e.comment
                       or e.create_system != 3 or e.external_attr >> 16 != 0o100644
                       or e.date_time != (1980,1,1,0,0,0) or not 0 < e.file_size <= LIMIT for e in entries)):
            raise RuntimeError(ERROR)
        files = {e.filename: archive.read(e) for e in entries}
    authority(files)
    # Reject prefixes/suffixes, reordered members and ignored ZIP attributes;
    # parsing a valid central directory alone does not establish canonical bytes.
    canonical=io.BytesIO()
    with zipfile.ZipFile(canonical,'w',compression=zipfile.ZIP_STORED) as archive:
        for name in sorted(files):
            entry=zipfile.ZipInfo(name,(1980,1,1,0,0,0));entry.create_system=3;entry.external_attr=0o100644<<16
            archive.writestr(entry,files[name])
    if data!=canonical.getvalue(): raise RuntimeError(ERROR)
    return {name:hashlib.sha256(body).hexdigest() for name,body in files.items()}


def inputs(directory, *, clean):
    ordinary(directory)
    if not directory.is_relative_to(ARTIFACTS): raise RuntimeError(ERROR)
    meta = decode(read(directory/'BUILD.json', 16384))
    if (type(meta) is not dict or set(meta) != {'version','candidate','qualification','mode','capture_ready','source_commit','source_dirty','files'}
            or type(meta['version']) is not int or meta['version'] != 1 or meta['candidate'] is not True
            or meta['qualification'] is not False or meta['mode'] != 'parent-transport' or meta['capture_ready'] is not False
            or type(meta['source_commit']) is not str or not re.fullmatch('[a-f0-9]{40}',meta['source_commit'])
            or type(meta['source_dirty']) is not bool or (clean and meta['source_dirty'])
            or type(meta['files']) is not dict or set(meta['files']) != PAYLOADS): raise RuntimeError(ERROR)
    files = {name:read(directory/name,LIMIT) for name in PAYLOADS}
    if {n:hashlib.sha256(b).hexdigest() for n,b in files.items()} != meta['files']: raise RuntimeError(ERROR)
    authority(files)
    return meta,files


def package(directory):
    """Exclusive deterministic packaging of an already built candidate, not a build receipt."""
    meta,files = inputs(directory, clean=False)
    xpi = directory/ARCHIVE
    ordinary(xpi)
    with zipfile.ZipFile(xpi,'x',compression=zipfile.ZIP_STORED) as archive:
        for name in sorted(files):
            entry = zipfile.ZipInfo(name, (1980,1,1,0,0,0));entry.create_system=3;entry.external_attr=0o100644 << 16
            archive.writestr(entry,files[name])
    data = read(xpi,10*LIMIT)
    if inspect(data) != meta['files']: raise RuntimeError(ERROR)
    identity = {k:meta[k] for k in ('candidate','qualification','mode','capture_ready','source_commit','source_dirty')}
    identity['xpi_sha256'] = hashlib.sha256(data).hexdigest()
    with (directory/'candidate.json').open('x',encoding='utf-8',newline='\n') as stream: json.dump(identity,stream,indent=2)
    return identity


def candidate_input(directory, expected_sha256):
    """Require an external archive pin and clean source; self-report alone cannot admit bytes."""
    if type(expected_sha256) is not str or not re.fullmatch('[a-f0-9]{64}',expected_sha256): raise RuntimeError(ERROR)
    meta,_ = inputs(directory, clean=True)
    data = read(directory/ARCHIVE,10*LIMIT)
    if hashlib.sha256(data).hexdigest() != expected_sha256 or inspect(data) != meta['files']: raise RuntimeError(ERROR)
    return directory/ARCHIVE, meta
