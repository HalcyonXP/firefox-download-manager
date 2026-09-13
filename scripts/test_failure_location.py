"""Modeled refusal locations and early persistence; no browser/registration effects."""
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

from qualification.failure_location import failure_location, _ROOT
from qualification import parent_installed as p
import test_browser_peer as peer_models


class FailureLocationTests(unittest.TestCase):
    def peer_failure(self):
        peer, _, _, inventory = peer_models.PeerTests().fixture()
        inventory['firefox.exe'].add(777)
        try:
            peer.require_browser_closed()
        except RuntimeError as error:
            return error
        self.fail('modeled extra browser must refuse')

    def test_actual_peer_refusal_has_only_allowlisted_locations(self):
        observed = failure_location(self.peer_failure())
        self.assertEqual(set(observed), {'locations', 'trace_truncated'})
        self.assertTrue(any(frame['source'] == 'browser-peer' for frame in observed['locations']))
        self.assertFalse(observed['trace_truncated'])
        self.assertNotIn(str(_ROOT), json.dumps(observed))
        self.assertTrue(all(set(frame) == {'source', 'line'} and type(frame['line']) is int
                            for frame in observed['locations']))

    def test_no_exception_formatting_arguments_or_unknown_source_locations(self):
        class Poison(RuntimeError):
            def __getattribute__(self, name):
                if name in ('args', '__traceback__'):
                    raise AssertionError('no overridden property access')
                return super().__getattribute__(name)
            def __str__(self):
                raise AssertionError('no formatting')
        try:
            raise Poison('opaque-private-marker')
        except Poison as error:
            observed = failure_location(error)
        self.assertEqual(observed, {'locations': [], 'trace_truncated': False})

    def test_model_trace_and_location_budgets_are_bounded(self):
        # Synthetic recognized code filename tests only the diagnostic codec.
        code = compile('def recurse(n):\n if n: return recurse(n-1)\n raise RuntimeError()\n',
                       str(_ROOT/'scripts/qualification/browser_peer.py'), 'exec')
        space = {}; exec(code, space)
        for depth, truncated in ((12, False), (30, False), (31, True), (80, True)):
            try:
                space['recurse'](depth)
            except RuntimeError as error:
                observed = failure_location(error)
            self.assertEqual(len(observed['locations']), 8)
            self.assertIs(observed['trace_truncated'], truncated)

    def test_browser_failure_is_saved_before_cleanup_or_profile_creation(self):
        peer, _, _, inventory = peer_models.PeerTests().fixture()
        inventory['firefox.exe'].add(777)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            browser = p.ParentBrowser(Path('unused.exe'), root/'not-created', {}, peer)
            with self.assertRaises(RuntimeError) as caught:
                browser.start()
            self.assertFalse(browser.profile.exists()); self.assertIsNone(browser.original)
            run = p.ParentInstalledRun(Path('unused'), Path('unused'), Path('unused'), 'a'*64,
                                       Path('unused.exe'), 'b'*64, parent_transport_experiment=True)
            run.plan = SimpleNamespace(path=root, created=True); run.browsers = [browser]
            with patch.object(p.InstalledRun, 'failure_record'):
                run.failure_record(caught.exception)
            observed = json.loads((root/'parent-failure.private.json').read_text(encoding='utf-8'))
            self.assertEqual(observed['version'], 2)
            self.assertIsNone(observed['cleanup_failure'])
            record = observed['browsers'][0]
            self.assertEqual(record['version'], 4)
            self.assertIn('locations', record['first_failure'])
            self.assertTrue(any(v['source'] == 'browser-peer' for v in record['first_failure']['locations']))
            self.assertFalse(run.failure_cleanup_attempted)

    def test_cleanup_first_failure_saved_before_hold_without_replaying_uninstall(self):
        run = p.ParentInstalledRun(Path('unused'), Path('unused'), Path('unused'), 'a'*64,
                                   Path('unused.exe'), 'b'*64, parent_transport_experiment=True)
        run.process = object()
        run.owner = SimpleNamespace(process=run.process, joined=False, quiesce=Mock(), retire=Mock())
        run.install_requested = True
        def refuse():
            raise RuntimeError('opaque-private-marker')
        run.close_resources = Mock(); run.uninstall = Mock(side_effect=refuse)
        with tempfile.TemporaryDirectory() as directory:
            run.plan = SimpleNamespace(path=Path(directory), created=True)
            with self.assertRaises(RuntimeError):
                run.failure_cleanup()
            observed = json.loads((run.plan.path/'parent-cleanup.private.json').read_text(encoding='utf-8'))
            self.assertEqual(observed['cleanup_failure']['step'], 'uninstall-unconfirmed')
            self.assertNotIn('opaque-private-marker', json.dumps(observed))
            self.assertFalse(run.owner.joined); run.owner.retire.assert_not_called()
            run.failure_cleanup(); run.uninstall.assert_called_once()

    def test_cleanup_observation_error_does_not_skip_other_original_stages(self):
        run = p.ParentInstalledRun(Path('unused'), Path('unused'), Path('unused'), 'a'*64,
                                   Path('unused.exe'), 'b'*64, parent_transport_experiment=True)
        run.process = object()
        run.owner = SimpleNamespace(process=run.process, joined=False, quiesce=Mock(), retire=Mock())
        original = KeyboardInterrupt()
        run.close_resources = Mock(side_effect=original)
        caught = []
        with patch.object(p, 'failure_location', side_effect=OSError('sink')):
            try: run._failure_step()
            except BaseException as error: caught.append(error)
        self.assertEqual(len(caught), 1); self.assertIs(caught[0], original)
        run.owner.quiesce.assert_called_once(); run.owner.retire.assert_not_called()

    def test_browser_observation_error_preserves_original_interruption(self):
        peer, _, _, _ = peer_models.PeerTests().fixture()
        browser = p.ParentBrowser(Path('unused.exe'), Path('unused-profile'), {}, peer)
        original = KeyboardInterrupt()
        caught = []
        with patch.object(p.Firefox, 'start', side_effect=original), \
                patch.object(p, 'failure_location', side_effect=OSError('sink')):
            try: browser.start()
            except BaseException as error: caught.append(error)
        self.assertEqual(len(caught), 1); self.assertIs(caught[0], original); self.assertTrue(browser.failed)

    def test_new_observation_interruption_is_not_hidden_by_ordinary_refusal(self):
        peer, _, _, _ = peer_models.PeerTests().fixture()
        browser = p.ParentBrowser(Path('unused.exe'), Path('unused-profile'), {}, peer)
        interruption = KeyboardInterrupt(); caught = []
        with patch.object(p.Firefox, 'start', side_effect=RuntimeError('ordinary refusal')), \
                patch.object(p, 'failure_location', side_effect=interruption):
            try: browser.start()
            except BaseException as error: caught.append(error)
        self.assertEqual(len(caught), 1); self.assertIs(caught[0], interruption)
        self.assertTrue(browser.failed)

    def test_first_browser_failure_does_not_change_during_later_cleanup(self):
        peer, _, _, _ = peer_models.PeerTests().fixture()
        browser = p.ParentBrowser(Path('unused.exe'), Path('unused-profile'), {}, peer)
        browser.note_failure(self.peer_failure())
        before = json.dumps(browser.first_failure, sort_keys=True)
        browser.stage = 'later'; browser.note_failure(RuntimeError('later refusal'))
        self.assertEqual(json.dumps(browser.first_failure, sort_keys=True), before)

    def test_first_cleanup_location_remains_after_later_refusal(self):
        run = p.ParentInstalledRun(Path('unused'), Path('unused'), Path('unused'), 'a'*64,
                                   Path('unused.exe'), 'b'*64, parent_transport_experiment=True)
        run.process = object()
        run.owner = SimpleNamespace(process=run.process, joined=False, quiesce=Mock(), retire=Mock())
        run.close_resources = Mock(side_effect=RuntimeError('first refusal'))
        with self.assertRaises(RuntimeError): run._failure_step()
        before = json.dumps(run.cleanup_failure, sort_keys=True)
        run.close_resources.side_effect = None; run.owner.quiesce.side_effect = OSError('later refusal')
        with self.assertRaises(OSError): run._failure_step()
        self.assertEqual(json.dumps(run.cleanup_failure, sort_keys=True), before)


if __name__ == '__main__':
    unittest.main()
