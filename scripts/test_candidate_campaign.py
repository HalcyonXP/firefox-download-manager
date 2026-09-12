"""Fileless ownership/UI models and actual benign HTTP/ZIP checks; no Firefox launch."""
import copy
import hashlib
import http.client
import io
import json
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import Mock, patch
from contextlib import closing
import uuid
import zipfile
from qualification.candidate_fixture import CandidateHandler, FILES, filename
from qualification.candidate_input import inspect, PAYLOADS, candidate_input
from qualification.support import ARTIFACTS
from qualification.candidate_permissions import prompt_ready, request, revoke, WATCH, SNAPSHOT, RETIRE
from qualification.candidate_run import CandidateRun
from qualification.candidate_ui import state, archive, BODY, STATE, DOWNLOAD
from qualification.fixture import Fixture


class CandidateTests(unittest.TestCase):
    def test_real_fixed_cases_and_cross_origin_retained_servers(self):
        owners = []
        try:
            target = Fixture(handler=CandidateHandler, owners=owners)
            source = Fixture(handler=CandidateHandler, owners=owners)
            source.candidate_target = target.url("file/cross")
            def fetch(fixture, path, method="GET", headers=None):
                with closing(http.client.HTTPConnection("127.0.0.1", fixture.server.server_port, timeout=5)) as c:
                    c.request(method, path, body=b"" if method == "POST" else None, headers=headers or {})
                    with closing(c.getresponse()) as response:
                        return response.status, dict(response.getheaders()), response.read(8193)
            for case in FILES:
                status, headers, data = fetch(source, "/page/"+case)
                self.assertEqual(status, 200); self.assertIn(b"Owned candidate fixture", data)
                method = "POST" if case == "post" else "GET"
                sent = {"Cookie": "candidate_fixture=synthetic"} if case == "cookie" else {}
                status, headers, data = fetch(source, "/file/"+case, method, sent)
                self.assertEqual(status, 200); self.assertEqual(data, BODY)
                self.assertIn(filename(case), headers["Content-Disposition"])
            status, headers, data = fetch(source, "/redirect")
            self.assertEqual((status, data), (302, b"")); self.assertEqual(headers["Location"], target.url("file/cross"))
            self.assertEqual(fetch(target, "/file/cross")[2], BODY)
            self.assertEqual(fetch(source, "/file/cookie")[0], 403)
            self.assertEqual(fetch(source, "/file/set-cookie")[1]["Set-Cookie"], "candidate_response=synthetic; Path=/unused; HttpOnly")
            self.assertEqual(fetch(source, "/file/vary")[1]["Vary"], "Cookie")
            self.assertEqual(fetch(source, "/not-a-fixture")[0], 404)
        finally:
            for fixture in owners: fixture.close()
        self.assertTrue(all(f.closed and not f.thread.is_alive() for f in owners))

    def test_archive_inventory_and_crc_are_not_inferred_from_metadata(self):
        expected = json.loads(Path("extension/candidate/manifest.json").read_text(encoding="utf-8"))
        files = {name: b"owned fixture" for name in PAYLOADS}
        files["manifest.json"] = json.dumps(expected).encode()
        def make(entries):
            out = io.BytesIO()
            with zipfile.ZipFile(out, "w") as z:
                for name, data in entries.items(): z.writestr(name, data)
            return out.getvalue()
        self.assertEqual(inspect(make(files), expected), {n: hashlib.sha256(v).hexdigest() for n, v in files.items()})
        for entries in ({**files, "inspect.html": b"extra"}, {k:v for k,v in files.items() if k!="click.js"}, {**files, "click.js": b"x"*(1024*1024+1)}):
            with self.assertRaises(RuntimeError): inspect(make(entries), expected)
        changed = {**expected, "incognito": "spanning"}
        with self.assertRaises(RuntimeError): inspect(make({**files, "manifest.json": json.dumps(changed).encode()}), expected)
        blob = bytearray(make(files)); offset = blob.find(b"owned fixture"); blob[offset] ^= 1
        with self.assertRaises(zipfile.BadZipFile): inspect(bytes(blob), expected)

    def test_candidate_directory_requires_clean_matching_metadata_and_assets(self):
        ARTIFACTS.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=ARTIFACTS) as temporary:
            root = Path(temporary).resolve()
            expected = json.loads(Path("extension/candidate/manifest.json").read_text(encoding="utf-8"))
            files = {n: b"candidate metadata fixture" for n in PAYLOADS}
            files["manifest.json"] = json.dumps(expected).encode()
            hashes = {n: hashlib.sha256(v).hexdigest() for n,v in files.items()}
            xpi = root / "download-manager-capture-candidate.xpi"
            with zipfile.ZipFile(xpi, "x") as z:
                for name,data in files.items():
                    (root/name).write_bytes(data); z.writestr(name, data)
            identity = {"candidate": True, "qualification": False, "source_commit": "a"*40,
                        "source_dirty": False, "xpi_sha256": hashlib.sha256(xpi.read_bytes()).hexdigest()}
            meta = {k:v for k,v in identity.items() if k!="xpi_sha256"}
            meta.update(version=1, files=hashes)
            (root/"BUILD.json").write_text(json.dumps(meta), encoding="utf-8")
            (root/"candidate.json").write_text(json.dumps(identity), encoding="utf-8")
            self.assertEqual(candidate_input(root), (xpi, identity))
            for field, value in (("source_dirty", True), ("qualification", 0), ("source_commit", "b"*40), ("xpi_sha256", "b"*64)):
                (root/"candidate.json").write_text(json.dumps({**identity, field:value}), encoding="utf-8")
                with self.assertRaises(RuntimeError): candidate_input(root)
            (root/"candidate.json").write_text(json.dumps(identity), encoding="utf-8")
            (root/"click.js").write_bytes(b"changed")
            with self.assertRaises(RuntimeError): candidate_input(root)

    def test_state_requires_strict_authority_settled_journal_and_exact_task_ids(self):
        task_id = str(uuid.uuid4())
        good = {"connected": True, "access": True, "access_ui": True, "enabled": True, "settled": True, "candidate": True,
                "saved": None, "journal": {"version": 1, "pending": []},
                "tasks": [{"id": "task-"+task_id, "name": filename("direct"), "state": "completed"}]}
        browser = Mock(); browser.script.return_value = good
        self.assertEqual(state(browser, {filename("direct"): None}), {filename("direct"): task_id})
        for key, value in (("access", 1), ("access_ui", False), ("enabled", False), ("settled", False), ("candidate", False), ("tasks", []),
                           ("journal", {"version": True, "pending": []}), ("saved", {"version": 1,"enabled": True})):
            browser.script.return_value = {**good, key: value}
            with self.assertRaises(RuntimeError): state(browser, {filename("direct"): task_id})
        browser.script.return_value = good
        with self.assertRaises(RuntimeError): state(browser, {filename("direct"): str(uuid.uuid4())})
        for key, value in (("state", "queued"), ("id", "task-invalid")):
            changed = copy.deepcopy(good); changed["tasks"][0][key] = value; browser.script.return_value = changed
            with self.assertRaises((RuntimeError, ValueError)): state(browser, {filename("direct"): task_id})

    def test_firefox_archival_requires_real_terminal_and_preserves_wrong_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve(); downloads = root / "Downloads"; downloads.mkdir()
            path = downloads / filename("off"); path.write_bytes(BODY)
            browser = Mock(); browser.chrome.return_value = True
            archive(browser, downloads, root, "off", "http://127.0.0.1:42/file/off")
            self.assertFalse(path.exists()); self.assertEqual((root / ("firefox-"+filename("off"))).read_bytes(), BODY)
            self.assertEqual(browser.chrome.call_args_list[0].args[1][-1], False)
            self.assertEqual(browser.chrome.call_args_list[1].args[1][-1], True)
            path.write_bytes(b"wrong")
            with self.assertRaises(RuntimeError): archive(browser, downloads, root, "off", "http://127.0.0.1:42/file/off")
            self.assertEqual(path.read_bytes(), b"wrong")

    def test_actual_candidate_browser_owner_retained_before_startup_failure(self):
        with patch("qualification.candidate_run.candidate_input", return_value=(Path("owned.xpi"), {})):
            run = CandidateRun(Path("unused"), Path("unused"), Path("unused"), Path("unused"))
        run.environment = {}; browser = Mock(); browser.start.side_effect = RuntimeError("owned failure")
        with patch("qualification.candidate_run.Firefox", return_value=browser):
            with self.assertRaises(RuntimeError): run.open_candidate(Mock(), Path("unused"), Path("unused"))
        self.assertEqual(run.browsers, [browser]); run.close_resources()
        browser.close.assert_called_once(); browser.process.wait.assert_called_once_with(timeout=0)

    def test_context_case_switches_to_retained_context_not_manager_before_navigation(self):
        with patch("qualification.candidate_run.candidate_input", return_value=(Path("owned.xpi"), {})):
            run = CandidateRun(Path("unused"), Path("unused"), Path("unused"), Path("unused"))
        run.manager_tab = "manager"; run.plan = Mock(path=Path("unused")); run.check_state = Mock()
        browser = Mock(); active = ["manager"]; windows = {"manager", "context"}; pages = []
        def command(name, args=None):
            if name == "WebDriver:SwitchToWindow": active[0] = args["handle"]
            elif name == "WebDriver:GetWindowHandles": return list(windows)
            elif name == "WebDriver:GetWindowHandle": return active[0]
            elif name == "WebDriver:CloseWindow": windows.remove(active[0])
            else: self.fail(name)
        browser.command.side_effect = command
        browser.navigate.side_effect = lambda url: pages.append((active[0], url))
        fixture = Mock(lock=threading.Lock(), requests={("GET", "/file/container"): 1})
        fixture.url.side_effect = lambda route: "http://127.0.0.1:42/"+route
        with patch("qualification.candidate_run.archive"):
            run.file_case(browser, fixture, Path("unused"), "container", context="context")
        self.assertEqual(pages, [("context", "http://127.0.0.1:42/page/container")]); self.assertEqual(windows, {"manager"})
        self.assertEqual(run.cases, ["container"])

    def test_permission_prompt_needs_exact_owner_request_and_enabled_controls(self):
        good = {"count": 1, "valid": True, "owner": True, "open": True, "primary": True, "secondary": True}
        self.assertTrue(prompt_ready(good))
        for key in good:
            self.assertFalse(prompt_ready({**good, key: 2 if key=="count" else False}))
        self.assertFalse(prompt_ready({**good, "count": True}))
        browser = Mock(manager="moz-extension://owned/manager.html")
        browser.chrome.side_effect = [True, good, True]
        request(browser, False)
        self.assertEqual(browser.click.call_args_list[0].args, ("#capture-access",))
        self.assertIn("secondary-button", browser.click.call_args_list[1].args[0])
        self.assertEqual(len(browser.click.call_args_list), 2)
        self.assertEqual(browser.chrome.call_args_list[-1].args, (RETIRE,))
        browser.reset_mock(); browser.script.return_value = False
        with self.assertRaises(RuntimeError): revoke(browser)

    def test_javascript_helpers_parse_and_have_no_hidden_capture_or_grant_commands(self):
        import subprocess
        sources = [STATE, DOWNLOAD, WATCH, SNAPSHOT, RETIRE]
        subprocess.run(["node", "-e", "for(const s of JSON.parse(process.argv[1]))new Function(s)", json.dumps(sources)], check=True, capture_output=True)
        source = Path("scripts/qualification/candidate_run.py").read_text(encoding="utf-8")
        for forbidden in ('message(browser', 'build_probe(', '.add(', 'setBoolPref(\'xpinstall', 'accept_session_prompt'):
            self.assertNotIn(forbidden, source.replace('extras.add(context)', ''))
        self.assertIn('browser.load(self.xpi)', source)
        self.assertIn('host.command("get_handoff"', source)
        self.assertIn('run.hold_failed_owners()', source)
        self.assertNotIn('info.resolve(', WATCH)


if __name__ == "__main__": unittest.main()
