import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { validateExtensionPolicy } from "./extension-policy.mjs";

const source = JSON.parse(
  await readFile(new URL("../extension/src/manifest.json", import.meta.url), "utf8"),
);
test("reviewed manifest has only deliberate authority", () => validateExtensionPolicy(source));
test("additional permissions, remote surfaces, weakened CSP and compatibility claims fail review", () => {
  for (const patch of [
    { permissions: [...source.permissions, "webRequest"] },
    { host_permissions: ["<all_urls>"] },
    { optional_permissions: ["cookies", "tabs"] },
    { incognito: "spanning" },
    { content_security_policy: { extension_pages: "script-src 'self' https://example.test" } },
    { content_scripts: [{ matches: ["<all_urls>"], js: ["background.js"] }] },
    { web_accessible_resources: [{ resources: ["manager.html"], matches: ["<all_urls>"] }] },
    { externally_connectable: { ids: ["*"] } },
    { update_url: "https://example.test/update" },
    {
      browser_specific_settings: {
        gecko: { ...source.browser_specific_settings.gecko, strict_min_version: "128.0" },
      },
    },
    {
      browser_specific_settings: {
        gecko: {
          ...source.browser_specific_settings.gecko,
          update_url: "https://example.test/update",
        },
      },
    },
  ])
    assert.throws(() => validateExtensionPolicy({ ...source, ...patch }));
});
