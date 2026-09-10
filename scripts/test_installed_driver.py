"""Installed-driver policies and real isolated HTTP resources; never mutate registration."""
import hashlib
import json
from pathlib import Path
import tempfile
import threading
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch
import uuid

from qualification.fixture import Fixture, BoundedServer, BLOCK, SMALL_SIZE
from qualification.installed import Binding, InstalledRun, verify_binding, closed_apps, package_input, HOST, HELPER, XPI, LINK, FAULTS, REMOVED, ROLES
from qualification.native import Host
from qualification.setup_owner import SetupOwner


class InstalledDriverTests(unittest.TestCase):
    def test_failed_cli_retains_parent_until_exit_even_with_broken_status_sink(self):
        for broken in (False, True):
            run = InstalledRun(Path("not-executed"), Path("not-written"))
            alive = [True]
            run.process = Mock()
            run.process.poll.side_effect = lambda: None if alive[0] else 0
            run.owner = Mock(joined=False)
            run.owner.retire.side_effect = lambda **kwargs: alive.__setitem__(0, False)
            run.uninstall = Mock()
            with patch("builtins.print", side_effect=OSError("sink") if broken else None), patch("qualification.installed.time.sleep"):
                run.hold_failed_owners()
            run.owner.retire.assert_called_once_with(timeout=1)
            run.process.wait.assert_called_once_with(timeout=0)
            run.uninstall.assert_not_called()
            self.assertFalse(run.owner.joined)  # Never invent Manager retirement from parent exit.

    def test_finished_http_handler_is_joined_before_its_reference_is_pruned(self):
        server = BoundedServer.__new__(BoundedServer)  # Pure bookkeeping model; no socket.
        server.ownership = threading.Lock()
        ended, live = Mock(), Mock()
        ended.is_alive.return_value = False; live.is_alive.return_value = True
        server.handlers = [(ended, Mock()), *[(live, Mock()) for _ in range(32)]]
        server.shutdown_request = Mock()
        request = Mock()
        server.process_request(request, ("127.0.0.1", 0))
        ended.join.assert_called_once_with(timeout=0)
        live.join.assert_not_called()
        self.assertEqual(len(server.handlers), 32)
        server.shutdown_request.assert_called_once_with(request)

    def test_setup_preflight_excludes_only_the_live_retained_parent(self):
        parent = Mock(pid=71); parent.poll.return_value = None
        for rows, owner, accepted in ((b"INFO: none", None, True),
            (b'"download-manager-setup.exe","71","Console"\n', parent, True),
            (b'"download-manager-setup.exe","72","Console"\n', parent, False),
            (b'"download-manager-setup.exe","71","Console"\n', None, False),
            (b'"download-manager-setup.exe","invalid","Console"\n', parent, False),
            (b"INFO: none", parent, False)):
            def tasklist(arguments, **kwargs):
                return SimpleNamespace(stdout=rows if arguments[2].endswith("download-manager-setup.exe") else b"INFO: none")
            with patch("qualification.installed.subprocess.run", side_effect=tasklist):
                if accepted:
                    closed_apps(Mock(), owner)
                else:
                    with self.assertRaises((RuntimeError, AssertionError)):
                        closed_apps(Mock(), owner)

    def test_package_hint_and_content_checks_refuse_legacy_before_launch(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            build = {"application_mode": "companion", "development": True, "source_dirty": False, "commit": "a" * 40}
            for name in ROLES:
                (root / name).write_bytes(json.dumps(build).encode() if name == "BUILD-INFO.json" else b"synthetic bytes")
            descriptor = {"format": "firefox-download-manager-package", "version": 1, "package_version": "0.1.0",
                "repository": "HalcyonXP/firefox-download-manager", "commit": "a" * 40, "target": "x86_64-pc-windows-gnullvm"}
            def publish():
                descriptor["files"] = {name: hashlib.sha256((root / name).read_bytes()).hexdigest() for name in ROLES}
                (root / "package.json").write_text(json.dumps(descriptor))
            publish()
            with patch("qualification.installed.ARTIFACTS", root), patch("qualification.installed.subprocess.Popen") as popen:
                with self.assertRaises(AssertionError): package_input(root)
                (root / "download-manager-setup.exe").write_bytes(b"synthetic Operation 0: idle")
                with self.assertRaises(AssertionError): package_input(root)  # Changed digest.
                publish(); self.assertEqual(package_input(root), descriptor)
                descriptor["version"] = True; publish()
                with self.assertRaises(AssertionError): package_input(root)
                popen.assert_not_called()

    def test_quiesce_keeps_setup_available_for_uninstall_then_close(self):
        process = Mock(); process.poll.return_value = None
        state = ["Operation 0: idle", "No Manager process launched by this setup."]
        actions = []
        def quit_manager(identity):
            actions.append(("quit", identity))
            state[1] = "Manager exit observed; retained child joined."
        owner = SetupOwner(process, lambda: tuple(state), lambda: actions.append("close"), quit_manager)
        owner.request(lambda: state.__setitem__(slice(None), ["Operation 1: complete", "Owned Manager process: 71"]))
        owner.quiesce()
        self.assertEqual(actions, [("quit", 71)])
        process.wait.assert_not_called()
        def uninstall():
            actions.append("uninstall")
            state[0] = "Operation 2: complete"
        owner.request(uninstall)
        owner.retire()
        self.assertEqual(actions, [("quit", 71), "uninstall", "close"])
        process.wait.assert_called_once()

    def test_resource_failure_does_not_skip_manager_quit_or_authorize_uninstall(self):
        run = InstalledRun(Path("not-executed"), Path("not-written"))
        order = []
        def failed_host():
            order.append("host")
            raise RuntimeError("synthetic native cleanup refusal")
        run.hosts = [SimpleNamespace(close=failed_host)]
        run.fixtures = [SimpleNamespace(close=lambda: order.append("fixture"))]
        run.owner = SimpleNamespace(joined=False, quiesce=lambda: order.append("quiesce"), retire=lambda: order.append("retire"))
        run.install_requested = True
        run.uninstall = lambda: order.append("uninstall")
        run.failure_cleanup()
        self.assertEqual(order, ["host", "fixture", "quiesce", "retire"])
        self.assertIn("native-parent-or-readers", run.cleanup_errors)

    def test_uncertain_uninstall_is_not_replayed_after_retiring_manager(self):
        run = InstalledRun(Path("not-executed"), Path("not-written"))
        run.owner = Mock(joined=False)
        run.install_requested = run.uninstall_requested = True
        run.uninstall = Mock()
        run.failure_cleanup()
        run.uninstall.assert_not_called()
        run.owner.quiesce.assert_called_once()
        run.owner.retire.assert_called_once()

    def test_changed_binding_or_live_browser_refuses_before_uninstall_dispatch(self):
        for changed in (True, False):
            run = InstalledRun(Path("not-executed"), Path("not-written"))
            run.binding = Binding(Path("generation"), Path("group"), "a" * 64)
            run.current_binding = lambda: Binding(Path("generation"), Path("group"), ("b" if changed else "a") * 64)
            run.owner, run.preflight = Mock(), Mock()
            with patch("qualification.installed.closed_apps", side_effect=RuntimeError("live fixture browser")) as closed:
                with self.assertRaises((AssertionError, RuntimeError)):
                    run.uninstall()
                self.assertEqual(closed.call_count, int(not changed))
            self.assertFalse(run.uninstall_requested)
            run.owner.request.assert_not_called()

    def test_preflight_refusal_precedes_domain_and_process_creation(self):
        run = InstalledRun(Path("not-executed"), Path("not-written"))
        run.prepare = Mock()
        preflight = Mock()
        preflight.closed_apps.side_effect = RuntimeError("synthetic live browser")
        with patch("qualification.installed.preflight_module", return_value=preflight), patch("qualification.installed.subprocess.Popen") as popen:
            with self.assertRaises(RuntimeError):
                run.execute()
            popen.assert_not_called()
        run.prepare.assert_not_called()
        self.assertIsNone(run.plan)

    def test_failure_record_sink_cannot_skip_cleanup(self):
        run = InstalledRun(Path("not-executed"), Path("not-written"))
        preflight = Mock(); preflight.closed_apps.side_effect = RuntimeError("synthetic preflight")
        run.failure_record = Mock(side_effect=OSError("synthetic sink"))
        run.failure_cleanup = Mock()
        with patch("qualification.installed.preflight_module", return_value=preflight):
            with self.assertRaises(OSError):
                run.execute()
        run.failure_cleanup.assert_called_once()

    def test_fixture_constructor_failures_retain_owner_and_close_real_socket(self):
        for location in ("thread-object", "thread-start"):
            owners = []
            target = "qualification.fixture.threading.Thread" + (".start" if location == "thread-start" else "")
            with patch(target, side_effect=RuntimeError("synthetic thread failure")):
                with self.assertRaises(RuntimeError):
                    Fixture(large_size=0, owners=owners)
            self.assertEqual(len(owners), 1)
            self.assertTrue(owners[0].closed)
            self.assertEqual(owners[0].server.fileno(), -1)
            owners[0].close()  # Idempotent; no shutdown wait for an unstarted loop.

    def test_native_constructor_retained_before_partial_reader_start_failure(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            owners = []
            process = Mock(); process.poll.return_value = None
            def popen(*args, **kwargs):
                self.assertEqual(len(owners), 1)
                self.assertEqual(kwargs["env"], {"OWNED_FIXTURE": "synthetic"})
                return process
            thread = Mock(ident=None); thread.start.side_effect = RuntimeError("synthetic reader start")
            thread.is_alive.return_value = False
            with patch("qualification.native.subprocess.Popen", side_effect=popen), patch("qualification.native.threading.Thread", return_value=thread):
                with self.assertRaises(RuntimeError):
                    Host(root, root, owners, environment={"OWNED_FIXTURE": "synthetic"})
            self.assertIs(owners[0].process, process)
            self.assertTrue(owners[0].closed)
            process.kill.assert_called_once()
            process.wait.assert_called_once()

    def test_orchestration_success_and_all_nine_fault_stages_without_registration(self):
        # Model the UI/native peers, not installed behavior. The actual output
        # assertions and cleanup/report ordering in execute() still run.
        for fault in (None, *FAULTS):
            with self.subTest(fault=fault), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary).resolve()
                package = root / "package"; package.mkdir(); (package / "package.json").write_bytes(b"{}")
                run = InstalledRun(package, root / "not-written.json", fault)
                state = {"sequence": 0, "child": None, "status": "initial"}
                process = Mock(returncode=0); process.poll.return_value = None
                binding = Binding(root / "install/generation", root / "Programs/group", "a" * 64)
                order = []
                def prepare():
                    run.plan = SimpleNamespace(path=root, created=True)
                    run.checkpoint("domain-created")
                    run.environment = {}
                    run.destination = root / "Downloads"; run.destination.mkdir()
                    run.install = root / "install"; run.install.mkdir()
                    run.programs = root / "Programs"; run.programs.mkdir()
                def observe():
                    lifetime = (f'Owned Manager process: {state["child"]}' if state["child"] else
                                "Manager exit observed; retained child joined." if state["sequence"] else
                                "No Manager process launched by this setup.")
                    return f'Operation {state["sequence"]}: ' + ("complete" if state["sequence"] else "idle"), lifetime
                def button(number):
                    order.append(number)
                    if number == 300:
                        state["sequence"] += 1; state["child"] = 71
                        binding.generation.mkdir(); binding.group.mkdir()
                        (run.install / "installation.json").write_bytes(b"owned fixture")
                    elif number == 304:
                        state["sequence"] += 1; state["status"] = REMOVED
                        binding.generation.rmdir(); binding.group.rmdir()
                        (run.install / "installation.json").unlink()
                    elif number == 305:
                        process.returncode = 0
                def quit_manager(identity):
                    self.assertEqual(identity, 71)
                    order.append("quit"); state["child"] = None
                def start(log):
                    run.process = process
                    run.ui = SimpleNamespace(find=lambda *args: 19, visible=lambda *args: True,
                        tray=lambda *args: state["child"] is not None,
                        text=lambda *args: "Manager is running. Downloads can continue across Firefox restarts.")
                    run.owner = SetupOwner(process, observe, lambda: button(305), quit_manager)
                    run.checkpoint("setup-started")
                run.prepare, run.start_setup, run.button = prepare, start, button
                run.current_binding = lambda: binding
                run.text = lambda number: state["status"] if number == 310 else observe()[int(number == 311)]
                class Peer:
                    def __init__(self, package, domain, owners, **kwargs):
                        owners.append(self); self.closed = False; self.process = Mock()
                        self.process.poll.return_value = None
                        self.reader = self.error_reader = SimpleNamespace(ident=1, is_alive=lambda: not self.closed)
                        self.tasks = {"task": {"state": "completed"}}; self.completed = {"task"}
                    def add(self, *_):
                        (run.destination / "owned-installed.bin").write_bytes(BLOCK * (SMALL_SIZE // len(BLOCK)))
                        return "task"
                    def terminal(self, *_): return {"state": "completed"}
                    def wait(self, predicate): assert predicate()
                    def close(self):
                        self.closed = True
                        self.process.poll.return_value = 0
                def fixture(*args, owners, **kwargs):
                    obj = SimpleNamespace(url=lambda *_: "http://127.0.0.1/range", closed=False, thread=None)
                    obj.close = lambda: setattr(obj, "closed", True)
                    owners.append(obj); return obj
                descriptor = {"commit": "a" * 40}
                with patch("qualification.installed.preflight_module", return_value=Mock()), \
                     patch("qualification.installed.closed_apps"), \
                     patch("qualification.installed.package_input", return_value=descriptor), \
                     patch("qualification.installed.Host", Peer), patch("qualification.installed.Fixture", fixture), \
                     patch("qualification.installed.owned_architecture", return_value={}), \
                     patch("qualification.installed.write_report") as report:
                    if fault is None:
                        run.execute()
                        self.assertTrue(run.owner.joined and run.uninstalled)
                        report.assert_called_once()
                    else:
                        with self.assertRaises(RuntimeError): run.execute()
                        report.assert_not_called()
                        if run.owner is not None:
                            self.assertTrue(run.owner.joined)
                        self.assertTrue((root / "failure.private.json").is_file())
                        self.assertTrue((root / "cleanup.private.json").is_file())
                    self.assertEqual(run.cleanup_errors, [])
                    self.assertTrue(all(host.closed for host in run.hosts))
                    if 304 in order:
                        self.assertLess(order.index("quit"), order.index(304))
                        self.assertLess(order.index(304), order.index(305))

    def test_binding_checks_actual_journal_and_preserves_unknown_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve(); install = root / "host"; install.mkdir()
            programs = root / "Programs"; programs.mkdir()
            identity, generation_id = str(uuid.uuid4()), str(uuid.uuid4())
            generation = install / generation_id; generation.mkdir()
            group = programs / f"Download Manager {identity}"; group.mkdir()
            data = {HELPER: b"fixture-helper", XPI: b"fixture-extension", LINK: b"fixture-link"}
            data[f"{HOST}.json"] = json.dumps({"name": HOST, "description": "Firefox Download Manager native host",
                "path": str(generation / HELPER), "type": "stdio", "allowed_extensions": ["download-manager@halcyonxp.local"]}).encode()
            for name, body in data.items(): (generation / name).write_bytes(body)
            (group / LINK).write_bytes(data[LINK])
            digests = {name: hashlib.sha256(body).hexdigest() for name, body in data.items()}
            record = {"id": generation_id, "package_version": "0.1.0", "helper_sha256": digests[HELPER], "extension_sha256": digests[XPI],
                "manifest_sha256": digests[f"{HOST}.json"], "shortcut_sha256": digests[LINK]}
            receipt = {"format": "firefox-download-manager-installation", "version": 2, "installation_id": identity, "current": generation_id,
                "generations": [record], "shortcut_scope": hashlib.sha256(str(programs).encode()).hexdigest()}
            (install / "installation.json").write_text(json.dumps(receipt))
            descriptor = {"files": digests, "package_version": "0.1.0"}
            registration = lambda: str(generation / f"{HOST}.json")
            self.assertEqual(verify_binding(install, programs, descriptor, registration).generation, generation)
            (install / "transaction.json").write_bytes(b"unresolved fixture journal")
            with self.assertRaises(AssertionError): verify_binding(install, programs, descriptor, registration)
            self.assertEqual((install / "transaction.json").read_bytes(), b"unresolved fixture journal")
            (install / "transaction.json").unlink()  # Exact fixture bytes owned by this test.
            (group / "unknown").write_bytes(b"preserve")
            with self.assertRaises(AssertionError): verify_binding(install, programs, descriptor, registration)
            self.assertEqual((group / "unknown").read_bytes(), b"preserve")


if __name__ == "__main__":
    unittest.main()
