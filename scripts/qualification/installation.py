"""Read-only exact-generation checks shared by owned installation/browser drivers."""
import hashlib
from pathlib import Path
import uuid

if __package__:
    from .support import bounded_json
else:
    from support import bounded_json


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def verified_binding(root, package, value):
    receipt = bounded_json(root / "installation.json")
    generation = receipt["current"]
    if str(uuid.UUID(generation, version=4)) != generation:
        raise RuntimeError("invalid installed generation")
    folder = root / generation
    manifest_path = folder / "com.halcyonxp.firefox_download_manager.json"
    def local_path(raw):
        extended = "\\\\?\\" + root.drive + "\\"
        return Path(raw[4:] if raw.startswith(extended) else raw)
    if not isinstance(value, str) or local_path(value) != manifest_path:
        raise RuntimeError("unverified installed registration")
    for path in (root, folder, manifest_path):
        if path.is_symlink() or path.is_junction() or path.resolve() != path:
            raise RuntimeError("installed generation has an unowned alias")
    manifest = bounded_json(manifest_path)
    if (manifest.get("allowed_extensions") != ["download-manager@halcyonxp.local"]
            or local_path(manifest["path"]) != folder / "download-manager-native-host.exe"):
        raise RuntimeError("unverified installed native authority")
    for name in ("download-manager-native-host.exe", "firefox-download-manager.xpi"):
        path = folder / name
        if path.is_symlink() or path.is_junction() or path.resolve() != path or sha(path) != sha(package / name):
            raise RuntimeError("installed payload differs from the artifact")
    return generation
