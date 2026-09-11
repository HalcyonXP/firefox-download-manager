"""Build a separately labelled unsigned capture XPI; no browser or installation actions."""
import argparse
import hashlib
import json
import re
from pathlib import Path
import subprocess
import time
import zipfile

ROOT = Path(__file__).resolve().parents[1]
PAYLOADS = {"background.js", "click.js", "manager.js", "manifest.json", "manager.html", "manager.css", "LICENSE.txt", "THIRD-PARTY-NOTICES.txt"}


def unique(pairs):
    result = {}
    for key, value in pairs:
        if key in result: raise RuntimeError("duplicate candidate field")
        result[key] = value
    return result


def build(output):
    output = Path(output).absolute()
    # Node independently enforces the new artifacts-only directory and ordinary parents.
    child = subprocess.Popen(["node", str(ROOT / "scripts/build-capture-candidate.mjs"), str(output)],
                             cwd=ROOT, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        code = child.wait(timeout=60)
    except BaseException:
        # Never kill the build parent and abandon its synchronous compiler child.
        while True:
            try: child.wait(); break
            except BaseException:
                try: time.sleep(.1)
                except BaseException: pass
        raise
    if code != 0: raise RuntimeError("capture candidate build refused; preserve output")
    with (output / "BUILD.json").open("rb") as source: raw = source.read(16385)
    if len(raw) > 16384: raise RuntimeError("candidate metadata bound exceeded")
    info = json.loads(raw, object_pairs_hook=unique)
    if (not isinstance(info, dict) or set(info) != {"version", "candidate", "qualification", "source_commit", "source_dirty", "files"}
            or type(info.get("version")) is not int or not isinstance(info.get("source_commit"), str)
            or not re.fullmatch(r"[a-f0-9]{40}", info["source_commit"])
            or info.get("candidate") is not True or info.get("qualification") is not False
            or type(info.get("source_dirty")) is not bool or info.get("version") != 1
            or set(info.get("files", {})) != PAYLOADS):
        raise RuntimeError("candidate metadata refused")
    payloads = {}
    for name in sorted(PAYLOADS):
        file = output / name
        if file.is_symlink() or file.is_junction() or not file.is_file(): raise RuntimeError("candidate file refused")
        with file.open("rb") as source: data = source.read(1024 * 1024 + 1)
        if not 0 < len(data) <= 1024 * 1024 or hashlib.sha256(data).hexdigest() != info["files"][name]:
            raise RuntimeError("candidate payload differs")
        payloads[name] = data
    manifest = json.loads(payloads["manifest.json"], object_pairs_hook=unique)
    if (manifest.get("version") != "0.2.0" or manifest.get("incognito") != "not_allowed"
            or manifest.get("permissions") != ["nativeMessaging", "menus", "storage", "webRequest", "webRequestBlocking"]
            or manifest.get("host_permissions") != ["http://*/*", "https://*/*"]
            or manifest.get("browser_specific_settings", {}).get("gecko", {}).get("id") != "download-manager@halcyonxp.local"):
        raise RuntimeError("candidate authority differs")
    xpi = output / "download-manager-capture-candidate.xpi"
    with zipfile.ZipFile(xpi, "x", compression=zipfile.ZIP_STORED) as archive:
        for name, data in payloads.items(): archive.writestr(name, data)
    # Independent archive readback before recording its identity.
    with zipfile.ZipFile(xpi) as archive:
        if set(archive.namelist()) != PAYLOADS or len(archive.namelist()) != len(PAYLOADS): raise RuntimeError("candidate archive inventory differs")
        for name, data in payloads.items():
            if archive.read(name) != data: raise RuntimeError("candidate archive bytes differ")
    result = {"candidate": True, "qualification": False, "source_commit": info["source_commit"],
              "source_dirty": info["source_dirty"], "xpi_sha256": hashlib.sha256(xpi.read_bytes()).hexdigest()}
    with (output / "candidate.json").open("x", encoding="utf-8") as target: json.dump(result, target, indent=2)
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(build(args.output)))
