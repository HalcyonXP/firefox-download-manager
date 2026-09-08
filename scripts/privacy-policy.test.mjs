import assert from "node:assert/strict";
import test from "node:test";

import { allowedEmail, inspectText, sensitivePath } from "./privacy-policy.mjs";

test("fixtures and public technical principals are distinguished from contact data", () => {
  for (const value of [
    "fixture@example.test",
    "fake@example.com",
    "noreply@github.com",
    "1+account@users.noreply.github.com",
    "download-manager@halcyonxp.local",
  ])
    assert.equal(allowedEmail(value), true);
  // Assemble a fake non-reserved contact domain; no actual address is in this test.
  const fakeContact = ["synthetic", "mail", "local"].join("@").replace("@local", ".local");
  assert.equal(allowedEmail(fakeContact), false);
  assert.equal(allowedEmail("fixture@example.test", true), false);
  assert.equal(inspectText(fakeContact)[0].category, "non-public-email");
  assert.ok(!JSON.stringify(inspectText(fakeContact)).includes(fakeContact));
});
test("private paths and credentials are detected without echoed values", () => {
  const path = ["C:", "Users", "SyntheticPerson", "download"].join(String.fromCharCode(92));
  const token = "ghp_" + "x".repeat(36);
  for (const text of [path, token]) {
    const result = inspectText(text);
    assert.ok(result.length > 0);
    assert.ok(!JSON.stringify(result).includes(text));
  }
  assert.deepEqual(inspectText("C:\\Users\\Example\\Downloads"), []);
});
test("runtime artifacts cannot become publication input", () => {
  for (const path of [
    ".env",
    "state/task.json",
    "artifacts/release.zip",
    "helper.exe",
    "extension.xpi",
    "session.log",
    ".playwright-cli/session.json",
  ])
    assert.equal(sensitivePath(path), true);
  for (const path of ["docs/STATE.md", "extension/src/manifest.json", "scripts/check-privacy.mjs"])
    assert.equal(sensitivePath(path), false);
});

test("project-board URLs are not mistaken for local home directories", () => {
  assert.deepEqual(inspectText("https://github.com/users/example/projects/1"), []);
  const unixPath = ["", "home", "SyntheticPerson", "file"].join("/");
  assert.equal(inspectText(unixPath)[0].category, "personal-profile-path");
});
