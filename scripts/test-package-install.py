"""Actual Windows package lifecycle on a disposable current-user registration only.
Never run against a live Firefox/helper or pre-existing native registration.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile
import uuid
import winreg

from qualification.native import evidence_identity
from qualification.support import bounded_json, new_report, write_report

KEY = r"Software\Mozilla\NativeMessagingHosts\com.halcyonxp.firefox_download_manager"
VIEW = winreg.KEY_WOW64_64KEY


def closed_apps():
    exe = Path(os.environ["WINDIR"]) / "System32/tasklist.exe"
    for name in ("firefox.exe", "download-manager-native-host.exe"):
        result = subprocess.run([str(exe), "/FI", f"IMAGENAME eq {name}", "/FO", "CSV", "/NH"],
                                capture_output=True, timeout=20, check=True)
        if f'"{name}",'.encode() in result.stdout.lower():
            raise RuntimeError("unowned Firefox/helper exists; refusing registry test")


def registration():
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, KEY, 0, winreg.KEY_READ | VIEW) as key:
            value, kind = winreg.QueryValueEx(key, "")
            if kind != winreg.REG_SZ:
                raise RuntimeError("unexpected registration type")
            return value
    except FileNotFoundError:
        return None


def key_absent():
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, KEY, 0, winreg.KEY_READ | VIEW):
            return False
    except FileNotFoundError:
        return True


def all_views_absent():
    for hive in (winreg.HKEY_CURRENT_USER, winreg.HKEY_LOCAL_MACHINE):
        for view in (winreg.KEY_WOW64_32KEY, winreg.KEY_WOW64_64KEY):
            try:
                with winreg.OpenKey(hive, KEY, 0, winreg.KEY_READ | view):
                    raise RuntimeError("existing registration; no test changes authorized")
            except FileNotFoundError:
                pass


def delete_owned_registration(expected):
    closed_apps()
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, KEY, 0, winreg.KEY_READ | VIEW) as key:
        subkeys, values, _ = winreg.QueryInfoKey(key)
        value, kind = winreg.QueryValueEx(key, "")
        if subkeys or values != 1 or kind != winreg.REG_SZ or value != expected:
            raise RuntimeError("registration changed; preserving unknown entries")
    winreg.DeleteKeyEx(winreg.HKEY_CURRENT_USER, KEY, VIEW)


def refused_shape(setup, environment, root, values):
    closed_apps()
    all_views_absent()
    if not key_absent():
        raise RuntimeError("foreign test cannot adopt an existing key")
    with winreg.CreateKeyEx(winreg.HKEY_CURRENT_USER, KEY, 0, winreg.KEY_WRITE | VIEW) as key:
        for name, value, kind in values:
            winreg.SetValueEx(key, name, 0, kind, value)
    def snapshot():
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, KEY, 0, winreg.KEY_READ | VIEW) as key:
            subkeys, count, _ = winreg.QueryInfoKey(key)
            return subkeys, sorted(winreg.EnumValue(key, i) for i in range(count))
    expected = snapshot()
    try:
        run(setup, "install", environment, root, False)
        assert snapshot() == expected
    finally:
        closed_apps()
        if snapshot() != expected:
            raise RuntimeError("unknown registry mutation; test content preserved")
        winreg.DeleteKeyEx(winreg.HKEY_CURRENT_USER, KEY, VIEW)


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def run(setup, action, environment, root, success=True):
    closed_apps()
    result = subprocess.run([str(setup), action, "--root", str(root)], env=environment,
                            capture_output=True, timeout=90)
    if (result.returncode == 0) != success:
        # Deliberately omit raw captured output/paths from public CI logs.
        raise RuntimeError(f"setup {action} returned unexpected status {result.returncode}")
    return result


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


def cleanup_owned(parent, owned_values):
    closed_apps()
    value = registration()
    if value is not None:
        if value not in owned_values:
            # Location and addon ID alone never establish ownership of a new binding.
            raise RuntimeError("unrecorded registration; preserving domain for reviewed recovery")
        delete_owned_registration(value)
    all_views_absent()
    shutil.rmtree(parent)


def test(package, report):
    if not __debug__:
        raise RuntimeError("qualification requires enabled assertions")
    report = new_report(report)
    closed_apps()
    all_views_absent()
    package = package.resolve()
    setup = package / "download-manager-setup.exe"
    subprocess.run([str(setup), "verify"], capture_output=True, check=True, timeout=30)
    def identity():
        return {**evidence_identity(package), "installer_harness_sha256": sha(Path(__file__))}
    artifact = identity()
    parent = Path(tempfile.mkdtemp(prefix="dm27 ")).resolve()
    local = parent / "Local Data"
    local.mkdir()
    root = local / "Host With Spaces"
    environment = {**os.environ, "LOCALAPPDATA": str(local), "USERPROFILE": str(parent / "Profile"),
                   "APPDATA": str(parent / "Roaming"), "PATH": str(Path(os.environ["WINDIR"]) / "System32")}
    state = local / "HalcyonXP/FirefoxDownloadManager/state"
    state.mkdir(parents=True)
    # Deliberately opaque preservation canaries, not a claim to test legacy decoding.
    task = state / "preserve-task-state.bin"
    task.write_bytes(b"synthetic protected state\x00\xff")
    download = parent / "Completed Download With Spaces.bin"
    download.write_bytes(bytes(range(256)) * 4096)
    before = (sha(task), sha(download))
    owned_values = set()
    foreign = str(parent / "Synthetic Foreign Host.json")
    evidence = {**artifact, "os": platform.platform(),
                "process_machine": platform.machine(), "native_architecture": os.environ.get("PROCESSOR_ARCHITEW6432", os.environ.get("PROCESSOR_ARCHITECTURE")),
                "kind": "actual package/native registry lifecycle, not Firefox UI qualification", "checks": []}
    try:
        for values in [[], [("", b"synthetic invalid type", winreg.REG_BINARY)], [("", "", winreg.REG_SZ)], [("Unexpected", "synthetic value", winreg.REG_SZ)]]:
            refused_shape(setup, environment, root, values)
        evidence["checks"].append("empty-malformed-and-named-registration-entries-refused")
        # Adversarial test-only key shapes are not an installation mechanism/fallback.
        closed_apps()
        all_views_absent()
        with winreg.CreateKeyEx(winreg.HKEY_CURRENT_USER, KEY, 0, winreg.KEY_WRITE | VIEW) as key:
            winreg.SetValueEx(key, "", 0, winreg.REG_SZ, foreign)
        owned_values.add(foreign)
        run(setup, "install", environment, root, False)
        assert registration() == foreign
        delete_owned_registration(foreign)
        evidence["checks"].append("foreign-registration-refused")
        run(setup, "install", environment, root)
        first = registration()
        first_generation = verified_binding(root, package, first)
        owned_values.add(first)
        evidence["checks"].append("actual-install-and-isolated-helper-hello")
        broken = parent / "Broken Package With Spaces"
        broken.mkdir()
        leaves = ["download-manager-native-host.exe", "download-manager-setup.exe", "firefox-download-manager.xpi", "INSTALL.md", "SECURITY.md", "THIRD-PARTY-NOTICES.txt", "BUILD-INFO.json", "LICENSE.txt", "package.json"]
        for name in leaves:
            shutil.copyfile(package / name, broken / name)
        (broken / "download-manager-native-host.exe").write_bytes(b"synthetic invalid executable")
        descriptor = bounded_json(broken / "package.json")
        descriptor["files"]["download-manager-native-host.exe"] = sha(broken / "download-manager-native-host.exe")
        (broken / "package.json").write_text(json.dumps(descriptor))
        old_receipt = (root / "installation.json").read_bytes()
        run(broken / "download-manager-setup.exe", "install", environment, root, False)
        assert registration() == first
        assert (root / "installation.json").read_bytes() == old_receipt
        assert not (root / "transaction.json").exists()
        evidence["checks"].append("actual-invalid-executable-launch-rollback")
        run(setup, "install", environment, root)
        second = registration()
        verified_binding(root, package, second)
        assert second != first
        owned_values.add(second)
        assert (sha(task), sha(download)) == before
        evidence["checks"].append("same-version-upgrade-preserves-state-and-downloads")
        # Keep unknown user content; cleanup must not recursively remove it.
        note = root / first_generation / "user-note.txt"
        note.write_bytes(b"unknown user note")
        run(setup, "cleanup", environment, root)
        assert note.read_bytes() == b"unknown user note"
        assert registration() == second
        evidence["checks"].append("cleanup-preserves-current-and-unknown-files")
        run(setup, "uninstall", environment, root)
        assert key_absent()
        assert not (root / "installation.json").exists()
        assert (sha(task), sha(download)) == before
        assert note.read_bytes() == b"unknown user note"
        evidence["checks"].append("actual-uninstall-keeps-state-downloads-unknown-files")
        if identity() != artifact:
            raise RuntimeError("installer artifact or harness changed during qualification")
    finally:
        try:
            cleanup_owned(parent, owned_values)
        except BaseException:
            ticket = Path(__file__).resolve().parents[1] / ".git" / f"install28-recovery-{uuid.uuid4()}.private.json"
            with ticket.open("x", encoding="utf-8") as recovery:
                json.dump({"owned_domain": str(parent), "installation_root": str(root)}, recovery)
            raise RuntimeError("owned installation domain preserved; no success report authorized") from None
    write_report(report, evidence)  # Only after every cleanup/identity gate, never overwriting a report.
    print("Actual isolated package install/upgrade/cleanup/uninstall passed; state/downloads preserved.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    test(args.package, args.report)
