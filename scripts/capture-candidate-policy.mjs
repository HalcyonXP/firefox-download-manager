import assert from "node:assert/strict";
import { validateExtensionPolicy } from "./extension-policy.mjs";

export const candidatePayloads = [
  "background.js",
  "click.js",
  "manager.js",
  "manifest.json",
  "manager.html",
  "manager.css",
  "LICENSE.txt",
  "THIRD-PARTY-NOTICES.txt",
];
export function validateCaptureCandidate(manifest) {
  assert.deepEqual(Object.keys(manifest).sort(), [
    "action",
    "background",
    "browser_specific_settings",
    "content_scripts",
    "content_security_policy",
    "description",
    "host_permissions",
    "incognito",
    "manifest_version",
    "name",
    "optional_permissions",
    "permissions",
    "version",
  ]);
  assert.equal(manifest.name, "Download Manager capture candidate");
  assert.equal(manifest.version, "0.2.0");
  assert.deepEqual(manifest.permissions, [
    "nativeMessaging",
    "menus",
    "storage",
    "webRequest",
    "webRequestBlocking",
  ]);
  assert.deepEqual(manifest.host_permissions, ["http://*/*", "https://*/*"]);
  assert.deepEqual(manifest.content_scripts, [
    {
      matches: ["http://*/*", "https://*/*"],
      js: ["click.js"],
      run_at: "document_start",
      all_frames: false,
      match_about_blank: false,
    },
  ]);
  // Validate all unchanged authority as well, without broadening the manual policy.
  validateExtensionPolicy({
    ...manifest,
    permissions: ["nativeMessaging", "menus", "storage"],
    host_permissions: [],
    optional_host_permissions: ["http://*/*", "https://*/*"],
    content_scripts: [],
  });
}
