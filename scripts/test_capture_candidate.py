"""Candidate build checks; no browser, setup, registration or installation."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch
import zipfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("candidate_builder", ROOT / "scripts/build-capture-candidate.py")
builder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(builder)


class CandidateBuildTests(unittest.TestCase):
    def test_real_candidate_is_separate_bounded_and_non_overwriting(self):
        artifacts = ROOT / "artifacts"; artifacts.mkdir(exist_ok=True)
        before = (ROOT / "extension/src/manifest.json").read_bytes()
        with tempfile.TemporaryDirectory(prefix="candidate-model-", dir=artifacts) as directory:
            output = Path(directory).resolve() / "candidate"
            result = builder.build(output)
            self.assertIs(result["candidate"], True); self.assertIs(result["qualification"], False)
            with zipfile.ZipFile(output / "download-manager-capture-candidate.xpi") as archive:
                self.assertEqual(set(archive.namelist()), builder.PAYLOADS)
                manifest = json.loads(archive.read("manifest.json"))
                self.assertEqual(manifest["version"], "0.2.0")
                self.assertEqual(manifest["incognito"], "not_allowed")
                self.assertNotIn("inspect.html", archive.namelist())
                self.assertNotIn(b"arm-missing-terminal", archive.read("background.js"))
                self.assertIn(b"task_handoff_phase", archive.read("background.js"))
            with self.assertRaises(RuntimeError): builder.build(output)
            self.assertEqual(json.loads((output / "candidate.json").read_text(encoding="utf-8")), result)
        self.assertEqual((ROOT / "extension/src/manifest.json").read_bytes(), before)

    def test_failed_wait_keeps_exact_build_parent_until_joined(self):
        child = Mock(); child.wait.side_effect = [TimeoutError("fixture"), InterruptedError("fixture"), 0]
        with patch.object(builder.subprocess, "Popen", return_value=child), patch.object(builder.time, "sleep"):
            with self.assertRaises(TimeoutError): builder.build(ROOT / "artifacts/unused-fixture")
        self.assertEqual(child.wait.call_count, 3)
        child.kill.assert_not_called(); child.terminate.assert_not_called()

    def test_duplicate_metadata_members_are_not_normalized(self):
        with self.assertRaises(RuntimeError): json.loads('{"candidate":true,"candidate":false}', object_pairs_hook=builder.unique)


if __name__ == "__main__": unittest.main()
