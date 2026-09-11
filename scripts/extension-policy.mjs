import assert from "node:assert/strict";

export const extensionCsp =
  "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/** Reviewed authority: native I/O, own pending-handoff storage, optional selected-site session access. */
export function validateExtensionPolicy(source) {
  assert.equal(source.manifest_version, 3);
  assert.equal(source.browser_specific_settings?.gecko?.id, "download-manager@halcyonxp.local");
  assert.equal(
    source.browser_specific_settings?.gecko?.strict_min_version,
    "156.0",
    "older Firefox API compatibility is not qualified",
  );
  assert.equal(source.incognito, "not_allowed", "private-window task history is unsupported");
  assert.deepEqual(source.permissions, ["nativeMessaging", "menus", "storage"]);
  assert.deepEqual(source.host_permissions ?? [], [], "no mandatory site authority");
  assert.deepEqual(source.optional_permissions, ["cookies"]);
  assert.deepEqual(source.optional_host_permissions, ["http://*/*", "https://*/*"]);
  assert.deepEqual(source.content_security_policy, { extension_pages: extensionCsp });
  assert.deepEqual(source.background, { scripts: ["background.js"], persistent: false });
  assert.deepEqual(source.content_scripts ?? [], []);
  assert.deepEqual(source.web_accessible_resources ?? [], []);
  assert.equal(source.externally_connectable, undefined);
  assert.equal(source.update_url, undefined);
  assert.equal(source.browser_specific_settings?.gecko?.update_url, undefined);
}
