import assert from "node:assert/strict";
import { validateExtensionPolicy } from "./extension-policy.mjs";
import { extensionLicenses } from "./extension-licenses.mjs";
import { access, readFile } from "node:fs/promises";

const source = JSON.parse(await readFile("extension/src/manifest.json", "utf8"));
const built = JSON.parse(await readFile("extension/dist/manifest.json", "utf8"));

assert.deepEqual(built, source, "built manifest differs from its source");
validateExtensionPolicy(source);

for (const script of source.background?.scripts ?? []) {
  assert.equal(script.includes(".."), false, "background path traverses out of extension");
  await access(`extension/dist/${script}`);
}

for (const [name, text] of Object.entries(await extensionLicenses()))
  assert.equal(
    await readFile(`extension/dist/${name}`, "utf8"),
    text,
    `missing or changed ${name}`,
  );

console.log("Validated built Manifest V3 extension, referenced assets and license notices.");
