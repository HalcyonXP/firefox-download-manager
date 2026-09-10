"""Setup-driver ownership tests; no browser, registry, or installed process use."""
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from qualification.setup_owner import DomainPlan, SetupOwner


class Process:
    def __init__(self):
        self.exit = None
        self.waited = False

    def poll(self):
        return self.exit

    def wait(self, timeout):
        if self.exit is None:
            raise TimeoutError("synthetic wait failure")
        self.waited = True
        return self.exit


class SetupOwnershipTests(unittest.TestCase):
    def fixture(self):
        process = Process()
        observed = ["Operation 0: idle", "No Manager process launched by this setup."]
        calls = []
        def close():
            calls.append("close")
            observed[:] = ["", ""]
            process.exit = 0
        def quit_manager(identifier):
            calls.append(("quit", identifier))
            observed[1] = "Manager exit observed; retained child joined."
        owner = SetupOwner(process, lambda: tuple(observed), close, quit_manager)
        return owner, process, observed, calls

    def test_before_dispatch_failure_closes_and_joins_retained_setup(self):
        owner, process, _, calls = self.fixture()
        owner.retire()
        self.assertEqual(calls, ["close"])
        self.assertTrue(process.waited and owner.joined)
        self.assertEqual(owner.before_close, ("idle", "no retained Manager"))

    def test_delivery_failure_cannot_turn_initial_no_child_label_into_completion(self):
        owner, process, observed, calls = self.fixture()
        def uncertain():
            raise OSError("synthetic delivery failure")
        with self.assertRaises(OSError):
            owner.request(uncertain)
        with self.assertRaisesRegex(RuntimeError, "unconfirmed"):
            owner.retire(timeout=.001)
        self.assertFalse(process.waited)
        self.assertEqual(calls, [])
        self.assertIs(owner.process, process)
        observed[0] = "Operation 1: complete"
        owner.retire()
        self.assertTrue(process.waited)

    def test_running_operation_is_not_complete_even_without_a_child(self):
        owner, process, observed, calls = self.fixture()
        owner.request(lambda: observed.__setitem__(0, "Operation 1: running"))
        with self.assertRaisesRegex(RuntimeError, "unconfirmed"):
            owner.retire(timeout=.001)
        self.assertEqual(calls, [])
        observed[0] = "Operation 1: complete"
        owner.retire()
        self.assertTrue(process.waited)

    def test_child_quit_precedes_parent_close_and_observations_survive_close(self):
        owner, process, observed, calls = self.fixture()
        owner.request(lambda: observed.__setitem__(slice(None), ["Operation 1: complete", "Owned Manager process: 17"]))
        owner.retire()
        self.assertEqual(calls, [("quit", 17), "close"])
        self.assertEqual(observed, ["", ""])
        self.assertTrue(process.waited)
        self.assertEqual(owner.before_close, ("complete", "no retained Manager"))

    def test_changed_identity_or_unknown_receipts_never_authorize_close(self):
        for changed in ["Owned Manager process: 18", "No Manager process launched by this setup.", "Manager failed; retained child joined.extra"]:
            owner, _, observed, calls = self.fixture()
            owner.request(lambda: observed.__setitem__(slice(None), ["Operation 1: complete", "Owned Manager process: 17"]))
            owner._observe()
            observed[1] = changed
            with self.assertRaisesRegex(RuntimeError, "unconfirmed"):
                owner.retire(timeout=.001)
            self.assertEqual(calls, [])
        owner, _, observed, calls = self.fixture()
        observed[0] = "Operation 18446744073709551616: complete"
        with self.assertRaises(RuntimeError):
            owner.request(lambda: calls.append("dispatch"))
        self.assertEqual(calls, [])

    def test_failed_wait_keeps_exact_owner_and_does_not_claim_join(self):
        owner, process, _, _ = self.fixture()
        owner.close_setup = lambda: None
        with self.assertRaises(TimeoutError):
            owner.retire(timeout=.1)
        self.assertIs(owner.process, process)
        self.assertFalse(owner.joined)

    def test_late_exit_after_failed_wait_is_joined_without_another_close(self):
        owner, process, _, calls = self.fixture()
        owner.close_setup = lambda: calls.append("close")
        with self.assertRaises(TimeoutError):
            owner.retire(timeout=.1)
        process.exit = 0
        owner.retire()
        self.assertEqual(calls, ["close"])
        self.assertTrue(owner.joined and process.waited)

    def test_uncertain_quit_is_not_replayed_and_parent_stays_retained(self):
        owner, process, observed, calls = self.fixture()
        owner.request(lambda: observed.__setitem__(slice(None), ["Operation 1: complete", "Owned Manager process: 17"]))
        def uncertain(identifier):
            calls.append(("quit", identifier))
            raise OSError("synthetic uncertain Quit")
        owner.quit_manager = uncertain
        with self.assertRaises(OSError):
            owner.retire()
        with self.assertRaises(RuntimeError):
            owner.retire(timeout=.001)
        self.assertEqual(calls, [("quit", 17)])
        self.assertIs(owner.process, process)
        observed[1] = "Manager exit observed; retained child joined."
        owner.retire()
        self.assertEqual(calls, [("quit", 17), "close"])

    def test_action_after_uncertain_close_is_refused(self):
        owner, _, _, calls = self.fixture()
        owner.close_setup = lambda: None
        with self.assertRaises(TimeoutError):
            owner.retire(timeout=.1)
        with self.assertRaises(RuntimeError):
            owner.request(lambda: calls.append("dispatch"))
        self.assertEqual(calls, [])

    def test_ui_completion_source_order_and_failure_sink_cleanup_guard(self):
        source = Path("crates/setup/src/application_ui.rs").read_text()
        start = source[source.index("    fn start("):source.index("    fn operation_complete(")]
        self.assertLess(start.index("self.sequence.set(sequence)"), start.index("Configuration::current()"))
        accept = source[source.index("    fn accept("):source.index("    fn tick(")]
        self.assertLess(accept.index("self.launched.borrow_mut().push(child)"), accept.index("self.lifetime"))
        self.assertLess(accept.index("self.lifetime"), accept.index("self.operation_complete()"))
        harness = Path("scripts/test-companion-preview.py").read_text()
        self.assertIn("setup_owner.request(lambda: send", harness)
        self.assertIn("finally:\n            try:\n                if setup_owner is not None:\n                    setup_owner.retire()", harness)

    def test_private_plan_precedes_creation_and_does_not_adopt_collisions(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            private = root / "private"; private.mkdir()
            plan = DomainPlan.record(root, private)
            self.assertEqual(len(list(private.iterdir())), 1)
            self.assertFalse(plan.path.exists() or plan.created)
            plan.path.mkdir()  # Deliberate collision, NOT a creator witness.
            with self.assertRaises(FileExistsError):
                plan.create()
            self.assertFalse(plan.created)
            self.assertEqual(list(plan.path.iterdir()), [])
            second = DomainPlan.record(root, private)
            second.create()
            self.assertTrue(second.created)
            self.assertTrue((second.path / "creation.private.json").is_file())

    def test_creation_witness_survives_a_later_record_failure(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            plan = DomainPlan.record(root, root)
            with patch.object(Path, "open", side_effect=OSError("record failure")):
                with self.assertRaises(OSError):
                    plan.create()
            self.assertTrue(plan.created and plan.path.is_dir())

    def test_failed_ticket_write_never_creates_a_domain(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            with patch.object(Path, "open", side_effect=OSError("ticket failure")):
                with self.assertRaises(OSError):
                    DomainPlan.record(root, root)
            self.assertEqual(list(root.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
