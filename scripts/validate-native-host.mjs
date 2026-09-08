import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const hostName = "com.halcyonxp.firefox_download_manager";
const extensionId = "download-manager@halcyonxp.local";
const template = JSON.parse(await readFile("native-host/manifest.template.json", "utf8"));
const extension = JSON.parse(await readFile("extension/src/manifest.json", "utf8"));
const rustHost = await readFile("crates/native-host/src/lib.rs", "utf8");
const installScript = await readFile("scripts/install-native-host.ps1", "utf8");
const uninstallScript = await readFile("scripts/uninstall-native-host.ps1", "utf8");

assert.deepEqual(
  Object.keys(template).sort(),
  ["allowed_extensions", "description", "name", "path", "type"],
  "native-host manifest should contain only Firefox-supported fields",
);
assert.equal(template.name, hostName, "native-host name drifted");
assert.match(template.name, /^(?!\.)(?!.*\.\.)(?!.*\.$)[a-z0-9._]+$/u);
assert.equal(template.type, "stdio", "native host must use stdio");
assert.equal(template.path, "__NATIVE_HOST_EXECUTABLE__", "template path marker drifted");
assert.equal(typeof template.description, "string");
assert.ok(template.description.length > 0 && template.description.length <= 256);
assert.deepEqual(
  template.allowed_extensions,
  [extensionId],
  "native host must allow only the fixed extension ID",
);
assert.equal(
  extension.browser_specific_settings?.gecko?.id,
  extensionId,
  "extension ID and native-host allowlist drifted",
);
assert.deepEqual(extension.permissions, ["nativeMessaging", "menus"]);
assert.deepEqual(extension.host_permissions ?? [], []);
assert.match(rustHost, new RegExp(`NATIVE_HOST_NAME: &str = "${hostName.replaceAll(".", "\\.")}"`));
assert.match(
  rustHost,
  new RegExp(`ALLOWED_EXTENSION_ID: &str = "${extensionId.replace(".", "\\.")}"`),
);

const setup = await readFile("crates/setup/src/lib.rs", "utf8");
const registry = await readFile("crates/setup/src/registry.rs", "utf8");
assert.ok(setup.includes(`HOST_NAME: &str = "${hostName}"`));
assert.ok(setup.includes(`EXTENSION_ID: &str = "${extensionId}"`));
assert.ok(registry.includes(`NativeMessagingHosts\\${hostName}`));
assert.ok(registry.includes("HKEY_CURRENT_USER"));
assert.ok(!registry.includes("HKEY_LOCAL_MACHINE"));
assert.ok(!registry.includes("delete_subkey_all"));
for (const script of [installScript, uninstallScript]) {
  assert.ok(script.includes("retired"), "unsafe development fallback must stay retired");
  assert.ok(script.includes("throw"));
  for (const forbidden of [
    "New-Item",
    "Copy-Item",
    "Remove-Item",
    "Set-ItemProperty",
    "SetExecutionPolicy",
  ]) {
    assert.ok(!script.includes(forbidden), "retired script must not mutate installation");
  }
}
console.log(
  "Validated native manifest/constants and retired-script guards; lifecycle qualification is separate.",
);
