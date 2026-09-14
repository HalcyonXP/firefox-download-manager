"""Separate, closed input policy for the candidate; does not broaden package/XPI policies."""
import hashlib
import io
import json
import re
import zipfile
from .installed import ROOT, ordinary
from .support import ARTIFACTS, bounded_json
from .xpi_policy import unique_object

PAYLOADS = {"background.js", "click.js", "manager.js", "manifest.json", "manager.html", "manager.css", "LICENSE.txt", "THIRD-PARTY-NOTICES.txt"}


def inspect(data, expected):
    if not isinstance(data, bytes) or not 0 < len(data) <= 8*1024*1024:
        raise RuntimeError("candidate archive size refused")
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        entries = archive.infolist()
        if (len(entries) != 8 or {e.filename for e in entries} != PAYLOADS
                or any(e.flag_bits & 1 or (e.external_attr >> 16) & 0o170000 not in (0, 0o100000)
                       or not 0 < e.file_size <= 1024*1024 for e in entries)
                or sum(e.file_size for e in entries) > 8*1024*1024):
            raise RuntimeError("candidate archive inventory refused")
        files = {e.filename: archive.read(e) for e in entries}
    if json.loads(files["manifest.json"], object_pairs_hook=unique_object) != expected:
        raise RuntimeError("candidate manifest differs from reviewed authority")
    return {name: hashlib.sha256(content).hexdigest() for name, content in files.items()}


def candidate_input(directory):
    ordinary(directory)
    if not directory.is_relative_to(ARTIFACTS): raise RuntimeError("candidate must be beneath artifacts")
    for name in (*PAYLOADS, "BUILD.json", "candidate.json", "download-manager-capture-candidate.xpi"):
        ordinary(directory / name)
    meta = bounded_json(directory / "BUILD.json")
    identity = bounded_json(directory / "candidate.json")
    keys = {"candidate", "qualification", "source_commit", "source_dirty", "xpi_sha256"}
    if (not isinstance(identity, dict) or set(identity) != keys or identity["candidate"] is not True
            or identity["qualification"] is not False or identity["source_dirty"] is not False
            or not isinstance(identity["source_commit"], str) or not re.fullmatch(r"[a-f0-9]{40}", identity["source_commit"])
            or not isinstance(identity["xpi_sha256"], str) or not re.fullmatch(r"[a-f0-9]{64}", identity["xpi_sha256"])):
        raise RuntimeError("clean candidate identity required")
    if (not isinstance(meta, dict) or set(meta) != {"version", "candidate", "qualification", "source_commit", "source_dirty", "files"}
            or type(meta["version"]) is not int or meta["version"] != 1
            or any(meta[k] != identity[k] or type(meta[k]) is not type(identity[k]) for k in keys - {"xpi_sha256"})):
        raise RuntimeError("candidate source records differ")
    xpi = directory / "download-manager-capture-candidate.xpi"
    with xpi.open("rb") as source: data = source.read(8*1024*1024 + 1)
    expected = json.loads((ROOT / "extension/candidate/manifest.json").read_text(encoding="utf-8"), object_pairs_hook=unique_object)
    hashes = inspect(data, expected)
    if hashes != meta["files"] or hashlib.sha256(data).hexdigest() != identity["xpi_sha256"]:
        raise RuntimeError("candidate archive identity differs")
    for name, digest in hashes.items():
        with (directory / name).open("rb") as source: payload = source.read(1024*1024+1)
        if not 0 < len(payload) <= 1024*1024 or hashlib.sha256(payload).hexdigest() != digest:
            raise RuntimeError("candidate asset identity differs")
    return xpi, identity
