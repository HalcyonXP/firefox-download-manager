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

for (const script of [installScript, uninstallScript]) {
  assert.ok(script.includes(hostName), "registration script host name drifted");
  assert.ok(script.includes("HKCU:\\Software\\Mozilla\\NativeMessagingHosts"));
  for (const forbidden of [
    "HKLM:",
    "New-NetFirewallRule",
    "route.exe",
    "netsh",
    "sc.exe",
    "schtasks",
    "VpnConnection",
  ]) {
    assert.ok(
      !script.includes(forbidden),
      `registration script unexpectedly contains ${forbidden}`,
    );
  }
}

console.log("Validated least-privilege Firefox native-host manifest and registration scripts.");
