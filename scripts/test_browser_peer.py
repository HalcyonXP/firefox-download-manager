"""Modeled preflight authority only; no browser/process/registration execution."""
import unittest
import os
from pathlib import Path
from unittest import mock
from qualification.browser_peer import BrowserPeer, IMAGES, parse_inventory
from qualification.setup_owner import SetupOwner


class Process:
    pid = 17
    exit = None

    def poll(self):
        return self.exit


class PeerTests(unittest.TestCase):
    def fixture(self):
        process = Process()
        state = {"operation": "Operation 0: idle", "lifetime": "No Manager process launched by this setup.",
                 "binding": "owned-fixture-binding"}
        owner = SetupOwner(process, lambda: (state["operation"], state["lifetime"]), lambda: None, lambda _: None)
        owner.request(lambda: state.update(operation="Operation 1: complete", lifetime="Owned Manager process: 42"))
        inventory = {name: set() for name in IMAGES}
        inventory["download-manager-setup.exe"] = {17}
        inventory["download-manager-native-host.exe"] = {42}
        peer = BrowserPeer(owner, state["binding"], lambda: state["binding"], lambda: inventory)
        return peer, owner, state, inventory

    def test_only_retained_parent_and_child_are_excluded(self):
        peer, _, _, _ = self.fixture()
        peer.require_browser_closed()

    def test_browser_or_extra_helper_is_never_excluded(self):
        for image in IMAGES:
            with self.subTest(image=image):
                peer, _, _, inventory = self.fixture()
                inventory[image].add(99)
                with self.assertRaises(RuntimeError):
                    peer.require_browser_closed()

    def test_missing_child_is_not_inferred_joined(self):
        peer, _, _, inventory = self.fixture()
        inventory["download-manager-native-host.exe"].clear()
        with self.assertRaises(RuntimeError):
            peer.require_browser_closed()

    def test_explicit_parent_join_can_explain_absence_not_readiness(self):
        for receipt in ("Manager exit observed; retained child joined.", "Manager failed; retained child joined."):
            peer, _, state, inventory = self.fixture()
            state["lifetime"] = receipt
            inventory["download-manager-native-host.exe"].clear()
            peer.require_browser_closed()

    def test_changed_or_lost_witness_refuses(self):
        changes = ({"lifetime": "Owned Manager process: 43"},
                   {"lifetime": "No Manager process launched by this setup."},
                   {"binding": "changed-binding"}, {"operation": "Operation 1: running"})
        for change in changes:
            peer, _, state, _ = self.fixture()
            state.update(change)
            with self.assertRaises(RuntimeError):
                peer.require_browser_closed()
        for field, value in (("close_requested", True), ("joined", True), ("process", Process())):
            peer, owner, _, _ = self.fixture()
            setattr(owner, field, value)
            with self.assertRaises(RuntimeError):
                peer.require_browser_closed()
        peer, owner, _, _ = self.fixture()
        owner.process.exit = 0
        with self.assertRaises(RuntimeError):
            peer.require_browser_closed()

    def test_transition_during_inventory_is_not_a_stable_witness(self):
        peer, _, state, inventory = self.fixture()
        def racing_inventory():
            state["lifetime"] = "Manager exit observed; retained child joined."
            return inventory
        peer.inventory = racing_inventory
        with self.assertRaises(RuntimeError):
            peer.require_browser_closed()

    def test_constructor_cannot_adopt_no_child_or_unfinished_setup(self):
        peer, owner, state, _ = self.fixture()
        with self.assertRaises(RuntimeError):
            BrowserPeer(owner, None, lambda: None)
        for lifetime in ("No Manager process launched by this setup.", "Manager exit observed; retained child joined."):
            state["lifetime"] = lifetime
            with self.assertRaises(RuntimeError):
                BrowserPeer(owner, peer.binding, peer.verify_binding)


@unittest.skipUnless(os.name == "nt", "Windows browser driver policy; no browser launch")
class BrowserHookTests(unittest.TestCase):
    fixture = PeerTests.fixture
    def test_default_guard_and_mutation_guard_remain_strict(self):
        from qualification import firefox
        peer, _, _, _ = self.fixture()
        ordinary = firefox.Firefox(Path("unused"), Path("unused"), {})
        owned = firefox.Firefox(Path("unused"), Path("unused"), {}, owned_peer=peer)
        with mock.patch.object(firefox, "closed_apps", side_effect=RuntimeError("helper remains")):
            with self.assertRaises(RuntimeError):
                ordinary._require_apps_closed()
            owned._require_apps_closed()
            with mock.patch.object(firefox.subprocess, "run") as mutation:
                with self.assertRaises(RuntimeError):
                    firefox.setup(Path("unused"), "uninstall", Path("unused"), {})
                mutation.assert_not_called()
        with self.assertRaises(RuntimeError):
            firefox.Firefox(Path("unused"), Path("unused"), {}, owned_peer=lambda: None)
        peer, _, _, inventory = self.fixture()
        inventory["firefox.exe"] = {99}
        owned = firefox.Firefox(Path("unused"), Path("unused"), {}, owned_peer=peer)
        with mock.patch.object(Path, "mkdir") as create_profile:
            with self.assertRaises(RuntimeError):
                owned.start()
            create_profile.assert_not_called()



class InventoryTests(unittest.TestCase):
    def test_exact_rows_and_known_absence(self):
        row = b'"firefox.exe","42","Console","1","123,456 K"\r\n'
        self.assertEqual(parse_inventory("firefox.exe", row), {42})
        self.assertEqual(parse_inventory("firefox.exe", b"INFO: No tasks are running which match the specified criteria.\r\n"), set())

    def test_malformed_output_cannot_become_absence(self):
        for data in (b"", b"unknown message", b'"other.exe","42","Console","1","0 K"',
                     b'"firefox.exe","042","Console","1","0 K"',
                     b'"firefox.exe","4294967296","Console","1","0 K"',
                     b'"firefox.exe","42","Console","1","0 K"\n'*2,
                     b'"firefox.exe","42"', b"x"*65537):
            with self.subTest(size=len(data)):
                with self.assertRaises(RuntimeError):
                    parse_inventory("firefox.exe", data)


if __name__ == "__main__":
    unittest.main()
