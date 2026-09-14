import assert from "node:assert/strict";
import test from "node:test";
import { randomUUID, createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import {
  mkdirSync,
  writeFileSync,
  readFileSync,
  readdirSync,
  lstatSync,
  realpathSync,
} from "node:fs";
import path from "node:path";
import { manifest, fixtureManifest, ADDON, HOST } from "./build-parent-probe.mjs";
import { verifyBundledFixture } from "./parent-fixture-sdk.mjs";

function domain({ image = true } = {}) {
  const artifacts = path.resolve("artifacts");
  try {
    mkdirSync(artifacts);
  } catch (error) {
    if (error.code !== "EEXIST") throw error;
  }
  const parent = lstatSync(artifacts);
  assert.ok(parent.isDirectory() && !parent.isSymbolicLink());
  assert.equal(realpathSync(artifacts), artifacts);
  const id = randomUUID();
  const value = path.resolve("artifacts", `dm-installed-${id}`);
  writeFileSync(
    path.resolve(".git", `parent-builder-${id}.private.json`),
    JSON.stringify({ domain: value, creation_observed: false, qualification: false }),
    { flag: "wx" },
  );
  mkdirSync(value);
  if (image) {
    mkdirSync(path.join(value, "native"));
    const command = path.join(value, "native", "download-manager-native-host.exe");
    // Metadata-only bytes are never executed. This is not an application image.
    writeFileSync(command, "metadata-only stdio fixture; no executable or runtime evidence\n", {
      flag: "wx",
    });
    writeFileSync(
      path.join(value, "native", `${HOST}.json`),
      JSON.stringify(fixtureManifest(command), null, 2) + "\n",
      { flag: "wx" },
    );
  }
  return value;
}

function invoke(root, nonce, overrides = {}) {
  // Configure a NEW CLI process before module initialization. Never toggle an
  // imported esbuild instance's worker environment in this test process.
  const env = {
    ...process.env,
    ESBUILD_WORKER_THREADS: "0",
    ESBUILD_MAX_BUFFER: "16777216",
    ...overrides,
  };
  delete env.ESBUILD_BINARY_PATH;
  const plan = path.resolve(".git", `parent-builder-child-${randomUUID()}.private.json`);
  writeFileSync(
    plan,
    JSON.stringify({ qualification: false, invocation_planned: true, domain: root }),
    { flag: "wx" },
  );
  const result = spawnSync(
    process.execPath,
    [path.resolve("scripts/build-parent-probe.mjs"), root, nonce],
    {
      env,
      encoding: "utf8",
      windowsHide: true,
      maxBuffer: 2 * 1024 * 1024,
    },
  );
  writeFileSync(`${plan}.stdout.txt`, result.stdout ?? "", { flag: "wx" });
  writeFileSync(`${plan}.stderr.txt`, result.stderr ?? "", { flag: "wx" });
  writeFileSync(
    `${plan}.wait.json`,
    JSON.stringify({
      qualification: false,
      pid: result.pid,
      exit_code: result.status,
      signal: result.signal,
      error: result.error?.code ?? null,
      synchronous_return: true,
    }),
    { flag: "wx" },
  );
  assert.equal(
    result.error,
    undefined,
    "owned builder did not return a normal process observation",
  );
  assert.equal(result.signal, null);
  assert.ok(Number.isSafeInteger(result.pid) && result.pid > 0);
  assert.ok(Number.isInteger(result.status));
  return result;
}

const sha = (data) => createHash("sha256").update(data).digest("hex");

test("fileless SDK fixture builder pins five payloads and fixed private configuration", async () => {
  const root = domain();
  const nonce = randomUUID();
  const first = invoke(root, nonce);
  assert.equal(first.status, 0, "owned builder refused its metadata fixture");
  const output = path.join(root, "probe");
  const record = JSON.parse(readFileSync(path.join(output, "probe.json"), "utf8"));
  assert.equal(record.qualification, false);
  assert.equal(record.addon_id, ADDON);
  assert.equal(record.nonce, nonce);
  assert.deepEqual(
    readdirSync(output).sort(),
    ["LICENSE.txt", "api.js", "background.js", "manifest.json", "probe.json", "schema.json"].sort(),
  );
  for (const [name, hash] of Object.entries(record.files))
    assert.equal(sha(readFileSync(path.join(output, name))), hash);
  for (const [name, hash] of Object.entries(record.sources))
    assert.equal(sha(readFileSync(path.resolve(name))), hash);
  assert.deepEqual(
    JSON.parse(readFileSync(path.join(output, "manifest.json"), "utf8")),
    manifest(),
  );
  assert.deepEqual(manifest().permissions, ["nativeMessaging"]);
  assert.equal(manifest().background.persistent, false);
  assert.equal(manifest().incognito, "not_allowed");
  assert.equal(manifest().host_permissions, undefined);
  const api = readFileSync(path.join(output, "api.js"), "utf8");
  assert.ok(
    api.includes(JSON.stringify(path.join(root, "native", "download-manager-native-host.exe"))),
  );
  assert.ok(api.includes(nonce));
  assert.doesNotMatch(api, /__OWNED_FIXTURE_(?:NONCE|COMMAND|MANIFEST)__/u);
  assert.match(api, /importGlobalProperties\(\["TextEncoder", "TextDecoder"\]\)/u);
  assert.match(api, /globalThis.managerParentProbe = class/u);
  assert.doesNotMatch(api, /importESModule\([^)]*NativeManifests/u);
  assert.doesNotMatch(
    readFileSync(path.join(output, "background.js"), "utf8"),
    /\b(?:setInterval|setTimeout|fetch|connectNative)\s*\(/u,
  );
  const schema = JSON.parse(readFileSync(path.join(output, "schema.json"), "utf8"));
  assert.equal(schema[0].namespace, "managerParentProbe");
  assert.deepEqual(
    schema[0].functions.map(({ name, parameters }) => [name, parameters]),
    [["run", []]],
  );
  assert.equal(schema[0].functions[0].returns, undefined);
  await verifyBundledFixture(
    api,
    nonce,
    path.join(root, "native", "download-manager-native-host.exe"),
  );
  const repeat = invoke(root, nonce);
  assert.equal(repeat.status, 1);
  assert.match(repeat.stderr, /EEXIST/u);
  assert.equal(sha(readFileSync(path.join(output, "api.js"))), record.files["api.js"]);
});

test("fixture builder refuses foreign manifest and ambiguous domain before compilation", () => {
  const root = domain();
  const native = path.join(root, "native", `${HOST}.json`);
  writeFileSync(native, JSON.stringify({ name: "other-host" }));
  const wrong = invoke(root, randomUUID());
  assert.equal(wrong.status, 1);
  assert.match(wrong.stderr, /Exact owned fixture manifest/u);
  assert.deepEqual(readdirSync(root), ["native"]);
  for (const [directory, nonce] of [
    [path.join(root, "child"), randomUUID()],
    [root, "not-an-id"],
  ]) {
    const result = invoke(directory, nonce);
    assert.equal(result.status, 1);
    assert.match(result.stderr, /domain and nonce/u);
  }
});

test("unbounded compiler settings refuse before fixture lookup or a compiler invocation", () => {
  // No native input: an omitted environment guard still cannot reach compilation.
  const root = domain({ image: false });
  const result = invoke(root, randomUUID(), { ESBUILD_WORKER_THREADS: "1" });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /synchronous bounded compiler/u);
  assert.deepEqual(readdirSync(root), []);
});
