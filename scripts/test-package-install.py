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
import winreg

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


def delete_owned_registration(expected):
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, KEY, 0, winreg.KEY_READ | VIEW) as key:
        subkeys, values, _ = winreg.QueryInfoKey(key)
        value, kind = winreg.QueryValueEx(key, "")
        if subkeys or values != 1 or kind != winreg.REG_SZ or value != expected:
            raise RuntimeError("registration changed; preserving unknown entries")
    winreg.DeleteKeyEx(winreg.HKEY_CURRENT_USER, KEY, VIEW)


def refused_shape(setup, environment, root, values):
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
    result = subprocess.run([str(setup), action, "--root", str(root)], env=environment,
                            capture_output=True, timeout=90)
    if (result.returncode == 0) != success:
        # Deliberately omit raw captured output/paths from public CI logs.
        raise RuntimeError(f"setup {action} returned unexpected status {result.returncode}")
    return result


def test(package, report):
    closed_apps()
    if not key_absent():
        raise RuntimeError("existing registration; no test changes authorized")
    package = package.resolve()
    setup = package / "download-manager-setup.exe"
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
    evidence = {"descriptor_sha256": sha(package / "package.json"), "os": platform.platform(),
                "process_machine": platform.machine(), "native_architecture": os.environ.get("PROCESSOR_ARCHITEW6432", os.environ.get("PROCESSOR_ARCHITECTURE")),
                "kind": "actual package/native registry lifecycle, not Firefox UI qualification", "checks": []}
    try:
        for values in [[], [("", b"synthetic invalid type", winreg.REG_BINARY)], [("", "", winreg.REG_SZ)], [("Unexpected", "synthetic value", winreg.REG_SZ)]]:
            refused_shape(setup, environment, root, values)
        evidence["checks"].append("empty-malformed-and-named-registration-entries-refused")
        # An entirely test-owned foreign entry must be refused and preserved.
        with winreg.CreateKeyEx(winreg.HKEY_CURRENT_USER, KEY, 0, winreg.KEY_WRITE | VIEW) as key:
            winreg.SetValueEx(key, "", 0, winreg.REG_SZ, foreign)
        owned_values.add(foreign)
        run(setup, "install", environment, root, False)
        assert registration() == foreign
        delete_owned_registration(foreign)
        evidence["checks"].append("foreign-registration-refused")
        run(setup, "install", environment, root)
        first = registration()
        assert first and Path(first).resolve().is_relative_to(root.resolve())
        owned_values.add(first)
        receipt = json.loads((root / "installation.json").read_text())
        first_generation = receipt["current"]
        manifest = json.loads(Path(first).read_text())
        assert manifest["allowed_extensions"] == ["download-manager@halcyonxp.local"]
        assert sha(Path(manifest["path"])) == sha(package / "download-manager-native-host.exe")
        evidence["checks"].append("actual-install-and-isolated-helper-hello")
        broken = parent / "Broken Package With Spaces"
        broken.mkdir()
        leaves = ["download-manager-native-host.exe", "download-manager-setup.exe", "firefox-download-manager.xpi", "INSTALL.md", "SECURITY.md", "THIRD-PARTY-NOTICES.txt", "BUILD-INFO.json", "package.json"]
        for name in leaves:
            shutil.copyfile(package / name, broken / name)
        (broken / "download-manager-native-host.exe").write_bytes(b"synthetic invalid executable")
        descriptor = json.loads((broken / "package.json").read_text())
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
        assert second and second != first
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
        report.parent.mkdir(parents=True, exist_ok=True)
        report.write_text(json.dumps(evidence, indent=2)+"\n", encoding="utf-8")
    finally:
        # Guard cleanup by provenance: never delete an unrelated registration.
        value = registration()
        if value is not None:
            if value not in owned_values:
                # A setup failure might have activated a journaled test generation.
                path = Path(value)
                if not path.resolve().is_relative_to(root.resolve()) or not path.is_file():
                    raise RuntimeError("unexpected registry ownership; preserving for inspection")
                manifest = json.loads(path.read_text())
                if manifest.get("allowed_extensions") != ["download-manager@halcyonxp.local"]:
                    raise RuntimeError("unexpected manifest; preserving for inspection")
            closed_apps()
            delete_owned_registration(value)
        if not key_absent():
            raise RuntimeError("registration cleanup did not complete")
        shutil.rmtree(parent)
    print("Actual isolated package install/upgrade/cleanup/uninstall passed; state/downloads preserved.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    test(args.package, args.report)
