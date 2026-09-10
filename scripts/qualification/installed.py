"""Owned real-registration setup/native slice, NOT Firefox or persistent-XPI qualification."""
import ctypes
from dataclasses import dataclass
import hashlib
import importlib.util
from itertools import islice
import json
import os
import platform
from pathlib import Path
import re
import subprocess
import time
import uuid

from .fixture import Fixture, SMALL_SIZE, expected_sha256
from .installed_ui import Controls
from .native import Host, file_sha256, owned_architecture
from .setup_owner import DomainPlan, SetupOwner
from .support import ARTIFACTS, bounded_json, new_report, write_report

ROOT = Path(__file__).resolve().parents[2]
HOST = "com.halcyonxp.firefox_download_manager"
HELPER = "download-manager-native-host.exe"
SETUP = "download-manager-setup.exe"
XPI = "firefox-download-manager.xpi"
LINK = "Download Manager.lnk"
ROLES = {"BUILD-INFO.json", "INSTALL.md", "LICENSE.txt", "SECURITY.md", "THIRD-PARTY-NOTICES.txt", HELPER, SETUP, XPI}
FAULTS = ("domain-created", "setup-started", "installed", "companion-ready", "bridge-started", "completed", "reconnected", "manager-joined", "uninstalled")
REMOVED = "Verified program files and matching registration removed. Downloads and task state were preserved."


def wait(predicate, seconds=30):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(.05)
    raise RuntimeError("owned installed observation deadline")


def ordinary(path):
    if path.resolve() != path or any(p.is_symlink() or p.is_junction() for p in (path, *path.parents)):
        raise RuntimeError("owned installed path alias refused")


def absent(path):
    ordinary(path)
    assert not path.exists()


def package_input(package):
    ordinary(package)
    if not package.is_relative_to(ARTIFACTS):
        raise RuntimeError("use a reviewed paired package beneath artifacts")
    descriptor = bounded_json(package / "package.json")
    assert set(descriptor) == {"format", "version", "package_version", "repository", "commit", "target", "files"}
    assert descriptor["format"] == "firefox-download-manager-package" and type(descriptor["version"]) is int and descriptor["version"] == 1
    assert descriptor["repository"] == "HalcyonXP/firefox-download-manager"
    assert descriptor["target"] == "x86_64-pc-windows-gnullvm"
    assert re.fullmatch(r"[0-9a-f]{40}", descriptor["commit"])
    assert set(descriptor["files"]) == ROLES
    for name, digest in descriptor["files"].items():
        path = package / name
        ordinary(path)
        assert path.is_file() and 0 < path.stat().st_size <= 128 * 1024 * 1024
        assert re.fullmatch(r"[0-9a-f]{64}", digest) and file_sha256(path) == digest
    build = bounded_json(package / "BUILD-INFO.json")
    assert build["application_mode"] == "companion" and build["development"] is True
    assert build["source_dirty"] is False and build["commit"] == descriptor["commit"]
    # Passive compatibility hint only, NOT code authenticity or readiness. Refuse
    # known older UI bytes before launching a window the owner cannot observe.
    assert b"Operation 0: idle" in (package / SETUP).read_bytes()
    return descriptor


def preflight_module():
    spec = importlib.util.spec_from_file_location("installed_preflight", ROOT / "scripts/test-package-install.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def closed_apps(preflight, setup=None):
    preflight.closed_apps()
    exe = Path(os.environ["WINDIR"]) / "System32/tasklist.exe"
    for name in ("download-manager-app.exe", "download-manager-companion.exe"):
        result = subprocess.run([str(exe), "/FI", f"IMAGENAME eq {name}", "/FO", "CSV", "/NH"],
                                capture_output=True, timeout=20, check=True)
        if f'"{name}",'.encode() in result.stdout.lower():
            raise RuntimeError("application remains; installed test refused")
    result = subprocess.run([str(exe), "/FI", f"IMAGENAME eq {SETUP}", "/FO", "CSV", "/NH"],
                            capture_output=True, timeout=20, check=True)
    output = result.stdout.lower()
    assert len(output) <= 64 * 1024
    identifiers = re.findall(rb'^"download-manager-setup.exe","([0-9]+)",', output, re.MULTILINE)
    assert len(identifiers) == output.count(b'"download-manager-setup.exe",')
    expected = set() if setup is None else {setup.pid}
    if setup is not None and setup.poll() is not None:
        raise RuntimeError("retained setup exited before preflight")
    if {int(identifier) for identifier in identifiers} != expected:
        raise RuntimeError("another setup or missing retained setup; installed test refused")


@dataclass(frozen=True)
class Binding:
    generation: Path
    group: Path
    receipt_sha256: str


def verify_binding(install, programs, descriptor, registration):
    ordinary(install)
    ordinary(programs)
    receipt_path = install / "installation.json"
    ordinary(receipt_path)
    receipt = bounded_json(receipt_path)
    assert set(receipt) == {"format", "version", "installation_id", "current", "generations", "shortcut_scope"}
    assert receipt["format"] == "firefox-download-manager-installation" and receipt["version"] == 2
    for identity in (receipt["installation_id"], receipt["current"]):
        assert str(uuid.UUID(identity, version=4)) == identity
    assert receipt["shortcut_scope"] == hashlib.sha256(str(programs).encode()).hexdigest()
    assert len(receipt["generations"]) == 1
    generation = receipt["generations"][0]
    assert set(generation) == {"id", "package_version", "helper_sha256", "extension_sha256", "manifest_sha256", "shortcut_sha256"}
    assert generation["id"] == receipt["current"] and generation["package_version"] == descriptor["package_version"]
    directory = install / generation["id"]
    entries = list(islice(install.iterdir(), 65))
    assert len(entries) <= 64
    for entry in entries:
        if entry.name not in ("installation.json", "transaction.json", generation["id"]):
            assert entry.name.startswith("companion-runtime.")
            endpoint = entry.name.removeprefix("companion-runtime.")
            assert str(uuid.UUID(endpoint, version=4)) == endpoint
    ordinary(directory)
    manifest = directory / f"{HOST}.json"
    roles = {HELPER: "helper_sha256", XPI: "extension_sha256", manifest.name: "manifest_sha256", LINK: "shortcut_sha256"}
    assert {p.name for p in islice(directory.iterdir(), 5)} == set(roles)
    for name, field in roles.items():
        path = directory / name
        ordinary(path)
        assert path.is_file() and 0 < path.stat().st_size <= 128 * 1024 * 1024
        assert file_sha256(path) == generation[field]
    for name in (HELPER, XPI):
        assert file_sha256(directory / name) == descriptor["files"][name]
    assert bounded_json(manifest) == {"name": HOST, "description": "Firefox Download Manager native host",
        "path": str(directory / HELPER), "type": "stdio", "allowed_extensions": ["download-manager@halcyonxp.local"]}
    assert registration() == str(manifest)
    group = programs / f'Download Manager {receipt["installation_id"]}'
    link = group / LINK
    ordinary(link)
    assert list(islice(group.iterdir(), 2)) == [link] and link.stat().st_size <= 8192
    assert link.read_bytes() == (directory / LINK).read_bytes()
    absent(install / "transaction.json")
    return Binding(directory, group, file_sha256(receipt_path))


class InstalledRun:
    def __init__(self, package, report, fault=None):
        self.package, self.report, self.fault = package, report, fault
        self.stage = "preflight"
        self.plan = self.process = self.owner = self.ui = self.binding = None
        self.hosts, self.fixtures, self.checks = [], [], []
        self.install_requested = self.uninstall_requested = self.uninstalled = False
        self.manager_window = None
        self.cleanup_errors = []

    def checkpoint(self, stage):
        self.stage = stage
        if self.fault == stage:
            raise RuntimeError("injected owned installed failure")

    def setup_window(self):
        if self.process is None or self.process.poll() is not None:
            raise RuntimeError("retained setup is unavailable")
        return self.ui.find(self.process.pid, "DownloadManagerPairedSetup")

    def text(self, number):
        return self.ui.text(self.setup_window(), number, self.process.pid)

    def observation(self):
        return self.text(312), self.text(311)

    def button(self, number):
        self.ui.click(self.setup_window(), number, self.process.pid)

    def quit_manager(self, identity):
        # The identifier alone never grants authority: recheck retained parent.
        assert self.owner._observe()[1] == identity
        window = self.ui.find(identity, "DownloadManagerCompanion")
        self.ui.click(window, 202, identity)

    def complete(self):
        try:
            return self.owner._observe()[0] == "complete"
        except RuntimeError:
            return False

    def current_binding(self):
        return verify_binding(self.install, self.programs, self.descriptor, self.preflight.registration)

    def prepare(self):
        self.plan = DomainPlan.record(Path(os.environ["LOCALAPPDATA"]).resolve(), ROOT / ".git")
        self.plan.create()
        self.checkpoint("domain-created")
        self.stage = "prepare-profile"
        root = self.plan.path
        self.local, self.profile = root / "Local", root / "Profile"
        self.destination = self.profile / "Downloads"
        self.local.mkdir(); self.profile.mkdir(); self.destination.mkdir()
        self.install = self.local / "HalcyonXP/FirefoxDownloadManager/host"
        self.environment = {**os.environ, "LOCALAPPDATA": str(self.local), "APPDATA": str(root / "Roaming"),
            "USERPROFILE": str(self.profile), "HOME": str(self.profile), "TMP": str(root), "TEMP": str(root),
            "PATH": str(Path(os.environ["WINDIR"]) / "System32")}
        # Resolve in the child environment; do not assume Programs follows APPDATA.
        self.stage = "resolve-programs"
        powershell = Path(os.environ["WINDIR"]) / "System32/WindowsPowerShell/v1.0/powershell.exe"
        result = subprocess.run([str(powershell), "-NoProfile", "-NonInteractive", "-Command",
            "[Environment]::GetFolderPath([Environment+SpecialFolder]::Programs, [Environment+SpecialFolderOption]::DoNotVerify)"],
            env=self.environment, stdin=subprocess.DEVNULL, capture_output=True, timeout=10, check=True,
            creationflags=subprocess.CREATE_NO_WINDOW)
        assert len(result.stdout) <= 2048 and not result.stderr
        self.programs = Path(result.stdout.decode().strip())
        ordinary(self.programs)
        assert self.programs.is_relative_to(self.profile) and self.programs != self.profile and not self.programs.exists()
        self.stage = "prepare-programs"
        self.programs.mkdir(parents=True)
        self.programs = self.programs.resolve()
        with (root / "fixture.private.json").open("x", encoding="utf-8") as stream:
            json.dump({"local": str(self.local), "profile": str(self.profile), "programs": str(self.programs),
                "install": str(self.install), "package": str(self.package),
                "package_source_commit": self.descriptor["commit"], "qualification": False}, stream)

    def start_setup(self, log):
        self.stage = "setup-launch"
        self.ui = Controls()  # Configure SDK calls before any process starts.
        self.process = subprocess.Popen([str(self.package / SETUP)], env=self.environment,
            stdin=subprocess.DEVNULL, stdout=log, stderr=log)
        self.owner = SetupOwner(self.process, self.observation, lambda: self.button(305), self.quit_manager)
        self.checkpoint("setup-started")
        window = wait(self.setup_window)
        assert self.ui.visible(window)
        assert self.observation() == ("Operation 0: idle", "No Manager process launched by this setup.")

    def close_resources(self):
        errors = []
        for host in self.hosts:
            try:
                host.close()
                if host.process is not None:
                    host.process.wait(timeout=0)  # Explicit retained wait even after observed exit.
            except BaseException:
                errors.append("native-parent-or-readers")
        for fixture in self.fixtures:
            try:
                fixture.close()
            except BaseException:
                errors.append("fixture")
        if errors:
            for error in errors:
                if error not in self.cleanup_errors:
                    self.cleanup_errors.append(error)
            raise RuntimeError("owned native/fixture retirement unconfirmed")

    def uninstall(self):
        if self.uninstall_requested:
            raise RuntimeError("uninstall already dispatched; no uncertain replay")
        observed = self.current_binding()
        if self.binding is not None:
            assert observed == self.binding
        self.binding = observed
        closed_apps(self.preflight, self.process)
        self.uninstall_requested = True
        self.owner.request(lambda: self.button(304))
        wait(self.complete)
        assert self.text(310) == REMOVED
        self.preflight.all_views_absent()
        for path in (self.binding.group, self.binding.generation, self.install / "installation.json", self.install / "transaction.json"):
            absent(path)
        self.uninstalled = True

    def failure_cleanup(self):
        # Every stage is attempted independently; a resource failure cannot skip
        # owned Manager Quit. Failed resources DO prevent registration removal.
        resources_clean = True
        try:
            self.close_resources()
        except BaseException:
            resources_clean = False
        if self.owner is None and self.process is not None:
            # Reconstruct only from our still-retained Popen before any dispatch,
            # never from an old ticket, process identifier or discovered tree.
            if self.install_requested:
                self.cleanup_errors.append("setup-owner-unresolved")
                return
            self.owner = SetupOwner(self.process, self.observation, lambda: self.button(305), self.quit_manager)
        if self.owner is not None and not self.owner.joined:
            settled = False
            try:
                self.owner.quiesce()
                settled = True
            except BaseException:
                self.cleanup_errors.append("setup-or-manager")
            if settled and resources_clean and self.install_requested and not self.uninstall_requested:
                try:
                    self.uninstall()  # Exact binding + fresh closed-app checks; never raw key deletion.
                except BaseException:
                    self.cleanup_errors.append("uninstall-unconfirmed")
            try:
                self.owner.retire()
            except BaseException:
                self.cleanup_errors.append("setup-join-unconfirmed")

    def retained_threads(self):
        threads = [thread for host in self.hosts for thread in (host.reader, host.error_reader)]
        for fixture in self.fixtures:
            if fixture.thread is not None:
                threads.append(fixture.thread)
            if fixture.server is not None:
                with fixture.server.ownership:
                    threads.extend(thread for thread, _ in fixture.server.handlers)
        return threads

    def hold_failed_owners(self):
        """Failure-only retention, not a deadline extension or successful retry.

        Keep the CLI alive while known actors remain live. Never rediscover PIDs,
        replay installation actions, or infer Manager retirement from parent exit.
        External termination can still destroy this ownership; it is not containment.
        """
        announced = False
        while True:
            processes = ([self.process] if self.process is not None else []) + [host.process for host in self.hosts if host.process is not None]
            live = any(process.poll() is None for process in processes)
            live = any(thread.is_alive() for thread in self.retained_threads()) or live
            if not live:
                for process in processes:
                    process.wait(timeout=0)
                for thread in self.retained_threads():
                    if thread.ident is not None:
                        thread.join(timeout=0)
                break
            if not announced:
                try:
                    print("Installed test failed; retaining live fixture owners. No success report is authorized.", flush=True)
                except OSError:
                    pass  # An unavailable status sink must not release live owners.
                announced = True
            try:
                self.close_resources()
            except Exception:
                pass  # The original failure and cleanup errors remain failures.
            if self.owner is not None and self.process.poll() is None:
                try:
                    self.owner.retire(timeout=1)
                except Exception:
                    pass
            time.sleep(.1)
        if announced and self.plan is not None and self.plan.created:
            with (self.plan.path / "retirement.private.json").open("x", encoding="utf-8") as stream:
                json.dump({"success": False, "retained_processes_waited": True,
                    "started_retained_threads_joined": True,
                    "setup_manager_retirement_observed": self.owner is not None and self.owner.joined,
                    "process_tree_containment": False, "domain_preserved": True}, stream)

    def failure_record(self, error):
        if self.plan is None or not self.plan.created:
            return  # No created-domain authority; any existing plan remains non-authoritative.
        chain = []
        current = error
        for _ in range(4):
            if current is None:
                break
            chain.append(type(current).__name__[:128])
            current = current.__cause__ if current.__cause__ is not None else current.__context__
        snapshot = {"stage": self.stage, "failure_type": type(error).__name__, "failure_types": chain, "success": False,
            "native": [{"process_started": host.process is not None,
                        "exit_observed": host.process.poll() if host.process is not None else None,
                        "closed": host.closed,
                        "reader_started": host.reader.ident is not None,
                        "reader_live": host.reader.is_alive(),
                        "error_reader_started": host.error_reader.ident is not None,
                        "error_reader_live": host.error_reader.is_alive()} for host in self.hosts],
            "fixtures": [{"closed": fixture.closed,
                          "thread_started": fixture.thread is not None and fixture.thread.ident is not None,
                          "thread_live": fixture.thread is not None and fixture.thread.is_alive()} for fixture in self.fixtures]}
        try:
            snapshot.update(operation=self.text(312), lifetime=self.text(311), status=self.text(310))
        except Exception:
            snapshot["observation_unavailable"] = True
        with (self.plan.path / "failure.private.json").open("x", encoding="utf-8") as stream:
            json.dump(snapshot, stream)

    def execute(self):
        try:
            self.preflight = preflight_module()
            closed_apps(self.preflight)
            self.preflight.all_views_absent()
            self.descriptor = package_input(self.package)
            self.prepare()
            with (self.plan.path / "setup.private.txt").open("x", encoding="utf-8") as log:
                self.start_setup(log)
                closed_apps(self.preflight, self.process); self.preflight.all_views_absent()
                self.install_requested = True
                self.owner.request(lambda: self.button(300))
                wait(self.complete, 60)
                self.binding = self.current_binding()
                self.checkpoint("installed")
                identity = self.owner._observe()[1]
                assert identity is not None
                def find_manager():
                    assert self.owner._observe()[1] == identity
                    return self.ui.find(identity, "DownloadManagerCompanion")
                self.manager_window = wait(find_manager)
                def running():
                    assert self.owner._observe()[1] == identity
                    return self.ui.text(self.manager_window, 200, identity) == "Manager is running. Downloads can continue across Firefox restarts."
                wait(running)
                assert self.owner._observe()[1] == identity
                assert self.ui.visible(self.manager_window) and self.ui.tray(self.manager_window)
                self.checks.append("real_setup_receipt2_registration_shortcut_visible_companion_tray")
                self.checkpoint("companion-ready")
                self.stage = "fixture-startup"
                fixture = Fixture(large_size=0, owners=self.fixtures)
                self.stage = "native-bridge-startup"
                host = Host(self.binding.generation, self.plan.path, self.hosts, environment=self.environment)
                architecture = owned_architecture(host)
                self.checkpoint("bridge-started")
                self.stage = "native-add"
                task = host.add(fixture.url("range"), "owned-installed.bin")
                assert host.terminal(task, 60)["state"] == "completed" and task in host.completed
                output = self.destination / "owned-installed.bin"
                ordinary(output)
                assert output.stat().st_size == SMALL_SIZE and file_sha256(output) == expected_sha256(SMALL_SIZE)
                self.checks.append("installed_native_bridge_completed_independent_8mib_output")
                self.checkpoint("completed")
                host.close(); host.process.wait(timeout=0)
                self.stage = "native-reconnect"
                host = Host(self.binding.generation, self.plan.path, self.hosts, environment=self.environment)
                host.wait(lambda: task in host.tasks)
                assert len(host.tasks) == 1 and host.tasks[task]["state"] == "completed"
                assert self.owner._observe()[1] == identity and self.ui.tray(self.manager_window)
                self.checks.append("native_eof_reconnect_same_companion_one_task")
                self.checkpoint("reconnected")
                self.close_resources()
                self.owner.quiesce()
                entries = list(islice(self.install.iterdir(), 65))
                assert len(entries) <= 64 and not any(p.name.startswith("companion-runtime.") for p in entries)
                assert not self.ui.tray(self.manager_window)
                self.checks.append("native_fixture_joins_manager_quit_parent_join_runtime_tray_removed")
                self.checkpoint("manager-joined")
                self.uninstall()
                assert output.stat().st_size == SMALL_SIZE and file_sha256(output) == expected_sha256(SMALL_SIZE)
                self.checkpoint("uninstalled")
                self.owner.retire()
                assert self.process.returncode == 0
                closed_apps(self.preflight); self.preflight.all_views_absent()
                assert package_input(self.package) == self.descriptor and not self.cleanup_errors
                self.checks.append("gui_uninstall_registration_shortcut_removal_output_preserved_setup_join")
            write_report(self.report, {"scope": "owned installed setup companion native slice", "m5_install_ready": False,
                "firefox_or_persistent_xpi_qualified": False, "physical_input": False, "normal_start_menu": False,
                "checks": self.checks, "architecture": architecture, "windows_version": platform.version(), "bytes": SMALL_SIZE, "sha256": expected_sha256(SMALL_SIZE),
                "package_source_commit": self.descriptor["commit"], "package_descriptor_sha256": file_sha256(self.package / "package.json"),
                "harness_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True, timeout=15).strip(),
                "harness_worktree_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, timeout=15)),
                "owned_domain_preserved": True})
        except BaseException as error:
            try:
                self.failure_record(error)  # Before any UI destruction; never raw exceptions/URLs.
            finally:
                self.failure_cleanup()
                if self.plan is not None and self.plan.created:
                    with (self.plan.path / "cleanup.private.json").open("x", encoding="utf-8") as stream:
                        json.dump({"success": False, "errors": self.cleanup_errors,
                            "setup_joined": self.owner is not None and self.owner.joined,
                            "uninstall_observed": self.uninstalled, "domain_preserved": True}, stream)
            raise RuntimeError("installed slice failed; preserve owned domain; no success authorized") from None


def qualify(package, report, fault=None):
    if not __debug__ or os.name != "nt" or ctypes.sizeof(ctypes.c_void_p) != 8:
        raise RuntimeError("installed driver requires assertions and 64-bit Windows Python")
    if fault is not None and fault not in FAULTS:
        raise RuntimeError("invalid installed fault stage")
    run = InstalledRun(Path(os.path.abspath(package)), new_report(report), fault)
    try:
        run.execute()
    except BaseException:
        run.hold_failed_owners()
        raise
