// Separate owned-domain fixture only; never extension/dist or paired payloads.
import { createRequire } from "node:module";
import { createHash } from "node:crypto";
import {
  openSync,
  closeSync,
  readSync,
  fstatSync,
  lstatSync,
  realpathSync,
  mkdirSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

export const ADDON = "download-manager@halcyonxp.local";
export const HOST = "com.halcyonxp.firefox_download_manager";
const IMAGE = "download-manager-native-host.exe";
const LIMIT = 64 * 1024;
const require = createRequire(import.meta.url);
const ROOT = path.resolve(".");
const ARTIFACTS = path.join(ROOT, "artifacts");
const SOURCES = [
  "extension/parent-probe/api.js",
  "extension/parent-probe/session.js",
  "extension/parent-probe/schema.json",
  "extension/parent-probe/background.js",
  "extension/protection-bridge/parent-launcher.js",
  "extension/protection-bridge/native-transport.js",
  "LICENSE",
];
const sha = (bytes) => createHash("sha256").update(bytes).digest("hex");

function ordinary(file, directory = false) {
  for (let value = file; ; value = path.dirname(value)) {
    const item = lstatSync(value);
    if (
      item.isSymbolicLink() ||
      (value === file && !directory ? !item.isFile() : !item.isDirectory())
    )
      throw new Error("Owned fixture path refused");
    if (value === path.dirname(value)) break;
  }
  if (realpathSync(file) !== file) throw new Error("Owned fixture alias refused");
}

function bytes(file, limit = LIMIT) {
  ordinary(file);
  const descriptor = openSync(file, "r");
  try {
    const size = fstatSync(descriptor).size;
    if (!Number.isSafeInteger(size) || size <= 0 || size > limit)
      throw new Error("Owned fixture input size refused");
    const data = Buffer.alloc(size + 1);
    let count = 0;
    while (count < data.length) {
      const read = readSync(descriptor, data, count, data.length - count);
      if (read === 0) break;
      count += read;
    }
    if (count !== size || fstatSync(descriptor).size !== size)
      throw new Error("Owned fixture input changed");
    return data.subarray(0, count);
  } finally {
    closeSync(descriptor);
  }
}

export function fixtureManifest(command) {
  return { name: HOST, type: "stdio", path: command, allowed_extensions: [ADDON] };
}

export function manifest() {
  return {
    manifest_version: 3,
    name: "Owned fileless parent stdio fixture",
    version: "0.0.1",
    browser_specific_settings: { gecko: { id: ADDON, strict_min_version: "156.0" } },
    incognito: "not_allowed",
    permissions: ["nativeMessaging"],
    background: { scripts: ["background.js"], persistent: false },
    content_security_policy: {
      extension_pages: "default-src 'none'; script-src 'self'; object-src 'none'",
    },
    experiment_apis: {
      managerParentProbe: {
        schema: "schema.json",
        parent: {
          scopes: ["addon_parent"],
          paths: [["managerParentProbe"]],
          script: "api.js",
        },
      },
    },
  };
}

function build(domain, nonce) {
  if (
    process.env.ESBUILD_WORKER_THREADS !== "0" ||
    process.env.ESBUILD_MAX_BUFFER !== "16777216" ||
    process.env.ESBUILD_BINARY_PATH
  )
    throw new Error("Owned synchronous bounded compiler required");
  const relative = path.relative(ARTIFACTS, domain);
  if (
    !path.isAbsolute(domain) ||
    path.dirname(domain) !== ARTIFACTS ||
    relative.startsWith(".") ||
    path.isAbsolute(relative) ||
    !/^dm-installed-[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/u.test(
      path.basename(domain),
    ) ||
    !/^[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/u.test(nonce)
  )
    throw new Error("New owned fixture domain and nonce required");
  ordinary(domain, true);
  const command = path.join(domain, "native", IMAGE);
  const nativeManifest = path.join(domain, "native", `${HOST}.json`);
  const image = bytes(command, 16 * 1024 * 1024);
  const expected = Buffer.from(JSON.stringify(fixtureManifest(command), null, 2) + "\n");
  if (!bytes(nativeManifest).equals(expected))
    throw new Error("Exact owned fixture manifest required");
  const sourceHashes = Object.fromEntries(
    SOURCES.map((name) => [name, sha(bytes(path.join(ROOT, name)))]),
  );
  const output = path.join(domain, "probe");
  mkdirSync(output); // Exclusive; preserve partial/failed domains instead of adopting them.
  // CLI-only compilation in a fresh process configured before Node startup.
  // Do not import esbuild and subsequently change its worker environment.
  const { buildSync } = require("esbuild");
  const bundle = buildSync({
    bundle: true,
    entryPoints: ["extension/parent-probe/api.js"],
    format: "iife",
    platform: "browser",
    target: "firefox156",
    write: false,
    legalComments: "none",
    sourcemap: false,
    define: {
      __OWNED_FIXTURE_NONCE__: JSON.stringify(nonce),
      __OWNED_FIXTURE_COMMAND__: JSON.stringify(command),
      __OWNED_FIXTURE_MANIFEST__: JSON.stringify(nativeManifest),
    },
  });
  if (bundle.outputFiles.length !== 1) throw new Error("Owned fixture bundle shape refused");
  const payloads = {
    "api.js": Buffer.from(bundle.outputFiles[0].contents),
    "background.js": bytes(path.join(ROOT, "extension/parent-probe/background.js")),
    "schema.json": bytes(path.join(ROOT, "extension/parent-probe/schema.json")),
    "manifest.json": Buffer.from(JSON.stringify(manifest(), null, 2) + "\n"),
    "LICENSE.txt": bytes(path.join(ROOT, "LICENSE")),
  };
  if (
    !bytes(command, 16 * 1024 * 1024).equals(image) ||
    !bytes(nativeManifest).equals(expected) ||
    SOURCES.some((name) => sha(bytes(path.join(ROOT, name))) !== sourceHashes[name])
  )
    throw new Error("Owned fixture inputs changed during compilation");
  const hashes = {};
  for (const [name, data] of Object.entries(payloads)) {
    if (data.length === 0 || data.length > LIMIT)
      throw new Error("Owned fixture payload bound exceeded");
    writeFileSync(path.join(output, name), data, { flag: "wx" });
    hashes[name] = sha(data);
  }
  const result = {
    version: 1,
    qualification: false,
    addon_id: ADDON,
    nonce,
    fixture_image_sha256: sha(image),
    files: hashes,
    sources: sourceHashes,
  };
  writeFileSync(path.join(output, "probe.json"), JSON.stringify(result, null, 2) + "\n", {
    flag: "wx",
  });
  return result;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  if (process.argv.length !== 4) throw new Error("Owned fixture domain and nonce required");
  build(process.argv[2], process.argv[3]);
  console.log(
    "Built fileless parent fixture payloads; qualification:false; no browser/native execution",
  );
}
