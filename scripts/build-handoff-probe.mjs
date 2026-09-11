// Owned diagnostic directory only. Does not modify extension/dist or package payloads.
import { buildSync } from "esbuild";
import { createHash } from "node:crypto";
import { readFile, writeFile, mkdir, lstat } from "node:fs/promises";
import path from "node:path";
import { extensionLicenses } from "./extension-licenses.mjs";

const output = process.argv[2];
if (process.argv.length !== 3 || !output || !path.isAbsolute(output))
  throw new Error("A new absolute owned diagnostic directory is required");
for (let parent = path.dirname(output); ; parent = path.dirname(parent)) {
  if (!(await lstat(parent)).isDirectory() || (await lstat(parent)).isSymbolicLink())
    throw new Error("Diagnostic parent is not ordinary");
  if (parent === path.dirname(parent)) break;
}
await mkdir(output); // Exclusive: never adopt/overwrite a prior directory.
const payloads = {};
if (
  process.env.ESBUILD_WORKER_THREADS !== "0" ||
  process.env.ESBUILD_MAX_BUFFER !== "16777216" ||
  process.env.ESBUILD_BINARY_PATH
)
  throw new Error("Diagnostic build requires bounded synchronous owned compiler execution");
for (const [name, entry] of Object.entries({
  "background.js": "extension/diagnostic/handoff.ts",
  "click.js": "extension/src/capture-click.ts",
  "manager.js": "extension/src/manager.ts",
})) {
  const result = buildSync({
    bundle: true,
    entryPoints: [entry],
    format: "iife",
    write: false,
    legalComments: "none",
    platform: "browser",
    target: "firefox156",
    sourcemap: false,
  });
  if (result.outputFiles.length !== 1) throw new Error("Unexpected diagnostic bundle shape");
  payloads[name] = result.outputFiles[0].contents;
}
const source = JSON.parse(await readFile("extension/src/manifest.json", "utf8"));
const manifest = {
  manifest_version: 3,
  name: "Owned loopback Manager handoff diagnostic",
  version: "0.0.1",
  browser_specific_settings: source.browser_specific_settings,
  permissions: ["nativeMessaging", "menus", "storage", "webRequest", "webRequestBlocking"],
  host_permissions: ["http://127.0.0.1/*"],
  incognito: "not_allowed",
  background: { scripts: ["background.js"], persistent: false },
  action: source.action,
  content_scripts: [
    { matches: ["http://127.0.0.1/page"], js: ["click.js"], run_at: "document_start" },
  ],
  content_security_policy: source.content_security_policy,
};
payloads["manifest.json"] = Buffer.from(JSON.stringify(manifest));
payloads["inspect.html"] = Buffer.from(
  "<!doctype html><meta charset=utf-8><title>Owned handoff diagnostic</title>",
);
for (const name of ["manager.html", "manager.css"])
  payloads[name] = await readFile(`extension/src/${name}`);
for (const [name, text] of Object.entries(await extensionLicenses()))
  payloads[name] = Buffer.from(text);
const hashes = {};
for (const [name, bytes] of Object.entries(payloads)) {
  if (path.basename(name) !== name || !bytes.length || bytes.length > 1024 * 1024)
    throw new Error("Diagnostic payload bound exceeded");
  await writeFile(path.join(output, name), bytes, { flag: "wx" });
  hashes[name] = createHash("sha256").update(bytes).digest("hex");
}
await writeFile(
  path.join(output, "probe.json"),
  JSON.stringify({ qualification: false, version: 1, files: hashes }),
  { flag: "wx" },
);
