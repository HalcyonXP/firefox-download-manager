import assert from "node:assert/strict";
import { validateExtensionPolicy } from "./extension-policy.mjs";

export const parentPayloads = Object.freeze([
  "background.js",
  "manager.js",
  "manager.html",
  "manager.css",
  "manifest.json",
  "parent-api.js",
  "parent-schema.json",
  "LICENSE.txt",
  "THIRD-PARTY-NOTICES.txt",
]);
export function validateParentCandidate(manifest, schema) {
  const { experiment_apis: experiments, ...ordinary } = manifest;
  validateExtensionPolicy(ordinary);
  assert.equal(manifest.version, "0.3.0");
  assert.equal(manifest.name, "Download Manager parent transport candidate");
  assert.deepEqual(
    Object.keys(manifest).sort(),
    [
      "manifest_version",
      "name",
      "description",
      "version",
      "browser_specific_settings",
      "permissions",
      "optional_permissions",
      "optional_host_permissions",
      "background",
      "action",
      "incognito",
      "content_security_policy",
      "experiment_apis",
    ].sort(),
  );
  assert.deepEqual(experiments, {
    managerParentTransport: {
      schema: "parent-schema.json",
      parent: {
        scopes: ["addon_parent"],
        paths: [["managerParentTransport"]],
        script: "parent-api.js",
      },
    },
  });
  assert.equal(schema.length, 1);
  assert.deepEqual(Object.keys(schema[0]).sort(), [
    "description",
    "functions",
    "namespace",
    "types",
  ]);
  assert.equal(schema[0].namespace, "managerParentTransport");
  assert.deepEqual(schema[0].types, [
    { id: "Connection", type: "integer", minimum: 1, maximum: 9007199254740991 },
  ]);
  const connection = { name: "connection", $ref: "Connection" };
  assert.deepEqual(schema[0].functions, [
    { name: "open", type: "function", async: true, parameters: [] },
    { name: "ready", type: "function", async: true, parameters: [connection] },
    { name: "read", type: "function", async: true, parameters: [connection] },
    {
      name: "postMessage",
      type: "function",
      async: true,
      parameters: [
        connection,
        { name: "message", type: "object", additionalProperties: { type: "any" } },
      ],
    },
    { name: "close", type: "function", async: true, parameters: [connection] },
  ]);
}
