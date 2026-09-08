import assert from "node:assert/strict";
import { access, readFile } from "node:fs/promises";

const source = JSON.parse(await readFile("extension/src/manifest.json", "utf8"));
const built = JSON.parse(await readFile("extension/dist/manifest.json", "utf8"));

assert.deepEqual(built, source, "built manifest differs from its source");
assert.equal(source.manifest_version, 3, "extension must use Manifest V3");
assert.equal(
  source.browser_specific_settings?.gecko?.id,
  "download-manager@halcyonxp.local",
  "native-host allowlist requires the stable extension ID",
);
assert.equal(source.background?.persistent, false, "Firefox MV3 background must be restartable");
assert.deepEqual(
  source.permissions ?? [],
  ["nativeMessaging", "menus"],
  "extension should request only Native Messaging and explicit link menu permissions",
);
assert.deepEqual(source.host_permissions ?? [], [], "no mandatory host access");
assert.deepEqual(source.optional_permissions, ["cookies"], "session access is optional");
assert.deepEqual(
  source.optional_host_permissions,
  ["http://*/*", "https://*/*"],
  "only direct HTTP(S) sites may be requested on demand",
);

for (const script of source.background?.scripts ?? []) {
  assert.equal(script.includes(".."), false, "background path traverses out of extension");
  await access(`extension/dist/${script}`);
}

console.log("Validated built Manifest V3 extension and referenced assets.");
