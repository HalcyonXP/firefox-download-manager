"""Owned native preview checks, not browser/installer or signed-package qualification.

Uses native control messages and an owned tray callback, not physical mouse input.
Only the retained preview process and its window/icon are mutated. State is retained.
"""
import argparse
import ctypes
from ctypes import wintypes as wt
import hashlib
import json
import msvcrt
import os
import platform
from pathlib import Path
import subprocess
import time
import uuid

from qualification.support import ARTIFACTS, new_report, write_report

ROOT = Path(__file__).resolve().parents[1]


class GUID(ctypes.Structure):
    _fields_ = [("a", wt.DWORD), ("b", wt.WORD), ("c", wt.WORD), ("d", wt.BYTE * 8)]


class IconId(ctypes.Structure):
    _fields_ = [("size", wt.DWORD), ("hwnd", wt.HWND), ("id", wt.UINT), ("guid", GUID)]


class IconData(ctypes.Structure):
    _fields_ = [("size", wt.DWORD), ("hwnd", wt.HWND), ("id", wt.UINT),
                ("flags", wt.UINT), ("callback", wt.UINT), ("icon", wt.HANDLE),
                ("tip", wt.WCHAR * 128), ("state", wt.DWORD), ("mask", wt.DWORD),
                ("info", wt.WCHAR * 256), ("version", wt.UINT),
                ("title", wt.WCHAR * 64), ("info_flags", wt.DWORD),
                ("guid", GUID), ("balloon", wt.HANDLE)]


def wait(predicate, seconds=15):
    until = time.monotonic() + seconds
    while time.monotonic() < until:
        value = predicate()
        if value:
            return value
        time.sleep(0.05)
    raise RuntimeError("owned preview observation deadline")


def qualify(report, setup_window=False, setup_binary=None):
    if os.name != "nt" or ctypes.sizeof(ctypes.c_void_p) != 8:
        raise RuntimeError("native preview driver requires Windows x64")
    report = new_report(report)
    binary = ROOT / ("target/debug/download-manager-setup.exe" if setup_window else "target/debug/download-manager-companion.exe")
    if setup_binary is not None:
        candidate = Path(os.path.abspath(setup_binary))
        if not setup_window or candidate.name != "download-manager-setup.exe" or not candidate.is_relative_to(ARTIFACTS):
            raise RuntimeError("setup binary override requires an owned artifact setup-window case")
        binary = candidate
    if (not binary.is_file() or not 0 < binary.stat().st_size <= 128 * 1024 * 1024
            or any(p.is_symlink() or p.is_junction() for p in (binary, *binary.parents))):
        raise RuntimeError("build an ordinary owned companion preview first")
    identity = uuid.uuid4().hex
    domain = ARTIFACTS / (f"setup-{identity}" if setup_window else f"companion50-preview-{identity}")
    ARTIFACTS.mkdir(exist_ok=True)
    domain.mkdir()  # create-new; never adopt an existing domain
    ticket = ROOT / f".git/companion50-preview-{identity}.private.json"
    with ticket.open("x", encoding="utf-8") as f:
        json.dump({"domain": str(domain), "scope": "owned setup window only" if setup_window else "owned companion preview only"}, f)
    system = Path(os.environ["WINDIR"]) / "System32"
    kernel = ctypes.WinDLL(str(system / "kernel32.dll"), use_last_error=True)
    kernel.IsWow64Process2.argtypes = [wt.HANDLE, ctypes.POINTER(wt.USHORT), ctypes.POINTER(wt.USHORT)]
    kernel.IsWow64Process2.restype = wt.BOOL
    user = ctypes.WinDLL(str(system / "user32.dll"), use_last_error=True)
    shell = ctypes.WinDLL(str(system / "shell32.dll"), use_last_error=True)
    callback_type = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    user.EnumWindows.argtypes = [callback_type, wt.LPARAM]
    user.EnumWindows.restype = wt.BOOL
    user.GetWindowThreadProcessId.argtypes = [wt.HWND, ctypes.POINTER(wt.DWORD)]
    user.GetWindowThreadProcessId.restype = wt.DWORD
    user.GetClassNameW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
    user.GetClassNameW.restype = ctypes.c_int
    user.GetDlgItem.argtypes = [wt.HWND, ctypes.c_int]
    user.GetDlgItem.restype = wt.HWND
    user.GetWindowTextW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
    user.GetWindowTextW.restype = ctypes.c_int
    user.IsWindowVisible.argtypes = [wt.HWND]
    user.IsWindowVisible.restype = wt.BOOL
    user.SendMessageTimeoutW.argtypes = [wt.HWND, wt.UINT, wt.WPARAM, wt.LPARAM, wt.UINT, wt.UINT, ctypes.POINTER(ctypes.c_size_t)]
    user.SendMessageTimeoutW.restype = wt.LPARAM
    shell.Shell_NotifyIconW.argtypes = [wt.DWORD, ctypes.POINTER(IconData)]
    shell.Shell_NotifyIconW.restype = wt.BOOL
    shell.Shell_NotifyIconGetRect.argtypes = [ctypes.POINTER(IconId), ctypes.POINTER(wt.RECT)]
    shell.Shell_NotifyIconGetRect.restype = ctypes.c_long
    proc = None
    hwnd = None
    checks = []
    stage = "launch"
    try:
        with binary.open("rb") as image, (domain / "process.private.txt").open("x", encoding="utf-8") as log:
            digest = hashlib.file_digest(image, "sha256").hexdigest()
            env = os.environ.copy()
            env.update(TMP=str(domain), TEMP=str(domain))
            if setup_window:
                local = domain / "Local"
                local.mkdir()
                env.update(LOCALAPPDATA=str(local), APPDATA=str(domain / "Roaming"), USERPROFILE=str(domain / "Profile"), HOME=str(domain / "Profile"))
            proc = subprocess.Popen([str(binary)] + ([] if setup_window else ["--preview"]), env=env, stdout=log, stderr=log)
            process_machine, native_machine = wt.USHORT(), wt.USHORT()
            assert kernel.IsWow64Process2(int(proc._handle), ctypes.byref(process_machine), ctypes.byref(native_machine))
            assert process_machine.value == 0 and native_machine.value == 0x8664

            def owned(window):
                pid = wt.DWORD()
                user.GetWindowThreadProcessId(window, ctypes.byref(pid))
                if proc.poll() is not None or pid.value != proc.pid:
                    raise RuntimeError("preview window ownership refused")

            def find_window():
                found = []

                @callback_type
                def visit(window, _):
                    pid = wt.DWORD()
                    user.GetWindowThreadProcessId(window, ctypes.byref(pid))
                    if pid.value == proc.pid:
                        name = ctypes.create_unicode_buffer(128)
                        user.GetClassNameW(window, name, len(name))
                        if name.value == ("DownloadManagerPairedSetup" if setup_window else "DownloadManagerCompanionPreview"):
                            found.append(window)
                    return True

                user.EnumWindows(visit, 0)
                return found[0] if len(found) == 1 else None

            def send(window, message, wparam=0, lparam=0):
                owned(window)
                result = ctypes.c_size_t()
                if not user.SendMessageTimeoutW(window, message, wparam, lparam, 2, 2000, ctypes.byref(result)):
                    raise RuntimeError("owned preview control delivery refused")

            def text(control):
                owned(control)
                value = ctypes.create_unicode_buffer(256)
                user.GetWindowTextW(control, value, len(value))
                return value.value

            if setup_window:
                stage = "owned-setup-window"
                hwnd = wait(find_window)
                assert user.IsWindowVisible(hwnd)
                expected = ["Install / upgrade", "Open Manager", "Repair registration", "Recover journal", "Uninstall", "Close setup", "Clean retired versions"]
                assert [text(user.GetDlgItem(hwnd, 300 + i)) for i in range(7)] == expected
                assert text(user.GetDlgItem(hwnd, 311)) == "No Manager process launched by this setup."
                checks.append("visible_setup_window_and_expected_controls")
                stage = "read-only-missing-installation"
                send(user.GetDlgItem(hwnd, 301), 0x00F5)
                status = user.GetDlgItem(hwnd, 310)
                wait(lambda: text(status) == "installation path is unsafe, unavailable or outside local application data")
                assert list(local.iterdir()) == []
                checks.append("open_missing_installation_refuses_without_creation")
                stage = "owned-setup-close"
                send(user.GetDlgItem(hwnd, 305), 0x00F5)
                assert proc.wait(timeout=15) == 0
                assert list(local.iterdir()) == []
                checks.append("setup_worker_retired_and_window_process_joined")
                image.seek(0)
                assert hashlib.file_digest(image, "sha256").hexdigest() == digest
                write_report(report, {"scope": "paired setup window and read-only refusal only", "qualification": False,
                    "checks": checks, "binary_sha256": digest, "physical_mouse_or_keyboard_input": False,
                    "browser_or_native_host_registration_test": False, "installed_workflow_qualified": False,
                    "harness_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                    "harness_worktree_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT))})
                print("Passed three owned setup-window checks; no install, registry, browser or tray-readiness claim.")
                return

            stage = "owned-window-and-engine-readiness"
            hwnd = wait(find_window)
            status = user.GetDlgItem(hwnd, 200)
            wait(lambda: text(status) == "Engine running. Firefox bridge is not connected in this preview.")
            assert user.IsWindowVisible(hwnd)
            icon_id = IconId()
            icon_id.size = ctypes.sizeof(icon_id)
            icon_id.hwnd, icon_id.id = hwnd, 1

            def icon_present():
                rect = wt.RECT()
                return shell.Shell_NotifyIconGetRect(ctypes.byref(icon_id), ctypes.byref(rect)) == 0

            stage = "tray-registration"
            assert icon_present()
            checks.append("real_window_engine_ready_and_tray_registration")
            stage = "hide"
            send(user.GetDlgItem(hwnd, 201), 0x00F5)  # native BM_CLICK, not physical input
            wait(lambda: not user.IsWindowVisible(hwnd))
            assert icon_present()
            checks.append("hide_preserves_registered_tray")
            stage = "synthetic-tray-open"
            send(hwnd, 0x8000, 1, 0x0202)  # owned WM_APP / left-button-up callback
            wait(lambda: user.IsWindowVisible(hwnd))
            checks.append("synthetic_owned_tray_callback_opens_status")
            stage = "owned-icon-recovery"
            owned(hwnd)
            data = IconData()
            data.size, data.hwnd, data.id = ctypes.sizeof(data), hwnd, 1
            registrations = user.GetDlgItem(hwnd, 203)
            before = text(registrations)
            assert before.startswith("Tray registrations: ")
            count = int(before.removeprefix("Tray registrations: "))
            assert count >= 1
            assert shell.Shell_NotifyIconW(2, ctypes.byref(data))  # remove ONLY this owned icon
            # Do not require sampling the fleeting absence between DELETE and
            # re-ADD. Require a new acknowledged registration instead of a
            # possibly cached rectangle or a timing-only sleep.
            wait(lambda: text(registrations) == f"Tray registrations: {count + 1}")
            assert icon_present()
            assert text(status) == "Engine running. Firefox bridge is not connected in this preview."
            checks.append("owned_icon_loss_re_registered_without_explorer_restart")
            stage = "quit-and-join"
            send(user.GetDlgItem(hwnd, 202), 0x00F5)
            assert proc.wait(timeout=15) == 0
            assert not icon_present()  # read-only absence observation, never mutate a retired HWND
            checks.append("quit_process_join_and_icon_absence")
            stage = "state-lock-release"
            roots = list(domain.glob("DownloadManagerCompanionPreview-*"))
            assert len(roots) == 1 and roots[0].is_dir()
            for path in (roots[0], roots[0] / "state", roots[0] / "downloads", roots[0] / "state/.task-store.lock"):
                assert not path.is_junction() and not path.is_symlink()
            with (roots[0] / "state/.task-store.lock").open("r+b") as lock:
                msvcrt.locking(lock.fileno(), msvcrt.LK_NBLCK, 1)
                lock.seek(0)
                msvcrt.locking(lock.fileno(), msvcrt.LK_UNLCK, 1)
            assert list((roots[0] / "downloads").iterdir()) == []
            checks.append("state_lock_released_and_no_downloads")
            image.seek(0)
            assert hashlib.file_digest(image, "sha256").hexdigest() == digest
        evidence = {"scope": "native development preview only", "checks": checks,
                    "windows_version": platform.version(), "native_machine": "AMD64", "execution": "native_x64",
                    "binary_sha256": digest, "harness_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                    "harness_worktree_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)),
                    "physical_mouse_or_keyboard_input": False, "actual_explorer_restart": False,
                    "browser_or_native_host_registration_test": False, "ipc_or_installer_qualified": False,
                    "temporary_state_retained": True}
        stage = "report-publication"
        write_report(report, evidence)
        print("Passed six owned native preview checks; no browser, installer or physical tray-input claim.")
    except BaseException as error:
        kind = "assertion" if isinstance(error, AssertionError) else "timeout" if isinstance(error, subprocess.TimeoutExpired) else "driver"
        with (domain / "failure.private.json").open("x", encoding="utf-8") as f:
            json.dump({"stage": stage, "kind": kind, "completed_checks": checks}, f)
        print(f"Preview observation failed at {stage}; bounded kind: {kind}.")
        terminated = False
        try:
            if proc is not None and proc.poll() is None:
                # Try the owned Quit control, then terminate only the exact retained
                # Popen handle if containment fails. Never a PID/name/tree fallback.
                try:
                    if hwnd:
                        send(user.GetDlgItem(hwnd, 305 if setup_window else 202), 0x00F5)
                    proc.wait(timeout=10)
                except Exception:
                    proc.terminate()
                    terminated = True
                    proc.wait(timeout=10)
        finally:
            with (domain / "containment.private.json").open("x", encoding="utf-8") as f:
                json.dump({"retained_handle_termination_used": terminated,
                           "owned_process_joined": proc is None or proc.poll() is not None,
                           "success_report_authorized": False}, f)
        raise RuntimeError("owned preview failed; private domain retained, no success report") from None


if __name__ == "__main__":
    if not __debug__:
        raise SystemExit("qualification requires enabled assertions")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--setup-window", action="store_true", help="owned no-install setup UI and missing-installation refusal only")
    parser.add_argument("--setup-binary", type=Path, help="explicit owned artifacts setup executable; setup-window only")
    args = parser.parse_args()
    try:
        qualify(args.report, args.setup_window, args.setup_binary)
    except Exception:
        raise SystemExit("Owned preview check failed; private domain retained; no success report.") from None
