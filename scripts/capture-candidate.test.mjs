import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { validateCaptureCandidate } from "./capture-candidate-policy.mjs";
import { validateExtensionPolicy } from "./extension-policy.mjs";
const candidate = JSON.parse(await readFile("extension/candidate/manifest.json", "utf8"));
test("candidate authority is explicit and cannot pass the manual package policy", () => {
  validateCaptureCandidate(candidate);
  assert.throws(() => validateExtensionPolicy(candidate));
});
test("candidate refuses added authority, private/all-frame execution and remote surfaces", () => {
  for (const patch of [
    { permissions: [...candidate.permissions, "cookies"] },
    { host_permissions: ["<all_urls>"] },
    { optional_permissions: ["cookies", "downloads"] },
    { incognito: "spanning" },
    { content_scripts: [{ ...candidate.content_scripts[0], all_frames: true }] },
    { content_scripts: [{ ...candidate.content_scripts[0], match_about_blank: true }] },
    { content_scripts: [{ ...candidate.content_scripts[0], js: ["background.js"] }] },
    { update_url: "https://example.test/update" },
    { web_accessible_resources: [] },
  ])
    assert.throws(() => validateCaptureCandidate({ ...candidate, ...patch }));
});
test("default build remains manual and candidate compiler/output are explicit", async () => {
  const normal = await readFile("scripts/build-extension.mjs", "utf8");
  assert.ok(!normal.includes("automatic-background"));
  const build = await readFile("scripts/build-capture-candidate.mjs", "utf8");
  assert.ok(build.includes('"background.js": "extension/src/automatic-background.ts"'));
  assert.ok(!build.includes('"extension/diagnostic/'));
  assert.ok(build.includes('process.env.ESBUILD_WORKER_THREADS = "0"'));
  assert.ok(build.includes('process.env.ESBUILD_MAX_BUFFER = "16777216"'));
  assert.ok(build.includes("delete process.env.ESBUILD_BINARY_PATH"));
  assert.ok(build.includes("await mkdir(output)"));
  assert.ok(build.includes('flag: "wx"'));
});

test("UI requests only website origins while authority checks retain required APIs", async () => {
  assert.deepEqual(candidate.optional_permissions, ["cookies"]);
  const manager = await readFile("extension/src/manager.ts", "utf8");
  assert.ok(manager.includes("browser.permissions.request(captureSitePermissions())"));
  assert.ok(!manager.includes("browser.permissions.request(capturePermissions())"));
  const background = await readFile("extension/src/background.ts", "utf8");
  assert.ok(background.includes("browser.permissions.contains(capturePermissions())"));
});
