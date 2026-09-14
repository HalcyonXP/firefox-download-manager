import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import { buildSync } from "esbuild";
import { validateParentCandidate, parentPayloads } from "./parent-candidate-policy.mjs";
import { validateExtensionPolicy } from "./extension-policy.mjs";

const manifest = JSON.parse(readFileSync("extension/parent-bridge/manifest.json", "utf8"));
const schema = JSON.parse(readFileSync("extension/parent-bridge/schema.json", "utf8"));
test("parent candidate selects only the fixed namespace; ordinary artifacts reject it", () => {
  validateParentCandidate(manifest, schema);
  assert.equal(parentPayloads.length, 9);
  assert.throws(() => validateExtensionPolicy(manifest));
  for (const mutate of [
    (m) => {
      m.permissions.push("webRequestBlocking");
    },
    (m) => {
      m.host_permissions = ["<all_urls>"];
    },
    (m) => {
      m.incognito = "spanning";
    },
    (m) => {
      m.experiment_apis.foreign = {};
    },
    (m) => {
      m.experiment_apis.managerParentTransport.parent.scopes = ["addon_child"];
    },
    (m) => {
      m.background.persistent = true;
    },
    (m) => {
      m.experiment_apis.managerParentTransport.parent.script = "foreign.js";
    },
  ]) {
    const copy = structuredClone(manifest);
    mutate(copy);
    assert.throws(() => validateParentCandidate(copy, schema));
  }
  for (const mutate of [
    (s) => {
      s[0].functions[0].parameters.push({ name: "host", type: "string" });
    },
    (s) => {
      s[0].functions.push({ name: "verdict", type: "function", parameters: [] });
    },
    (s) => {
      s[0].functions[3].parameters[1].type = "any";
    },
    (s) => {
      s[0].types[0].minimum = 0;
    },
  ]) {
    const copy = structuredClone(schema);
    mutate(copy);
    assert.throws(() => validateParentCandidate(manifest, copy));
  }
});

test("actual privileged wrapper binds SDK modules/context, checks arity and sanitizes failures", async () => {
  const source = readFileSync("extension/parent-bridge/api.js", "utf8");
  assert.equal((source.match(/^import .+;$/gmu) ?? []).length, 2);
  const calls = [];
  const imports = [];
  const owners = [];
  const extension = {};
  class ParentApi {
    constructor(ext, platform) {
      assert.equal(ext, extension);
      assert.equal(platform.fixed, true);
      owners.push(this);
    }
    open(context) {
      calls.push(["open", context]);
      return 1;
    }
    ready(context, id) {
      calls.push(["ready", context, id]);
      return true;
    }
    read(context, id) {
      calls.push(["read", context, id]);
      return null;
    }
    postMessage(context, id, message) {
      calls.push(["post", context, id, message]);
    }
    close() {
      throw new Error("synthetic native detail must not cross API");
    }
    shutdown() {
      calls.push(["shutdown"]);
      return Promise.resolve(true);
    }
  }
  const modules = {};
  const scope = vm.createContext({
    ParentApi,
    firefoxLauncherPlatform: (...args) => {
      assert.equal(args.length, 6);
      return { fixed: true };
    },
    ExtensionAPI: class {
      constructor() {
        this.extension = extension;
      }
    },
    Cu: {
      importGlobalProperties: (names) =>
        assert.deepEqual(Array.from(names), ["TextEncoder", "TextDecoder"]),
    },
    PathUtils: {},
    ChromeUtils: {
      importESModule: (uri) => {
        imports.push(uri);
        const key = uri.split("/").at(-1).split(".")[0];
        modules[key] = {};
        return { [key]: modules[key] };
      },
    },
  });
  vm.runInContext(source.replace(/^import .+;\r?\n/gmu, ""), scope);
  const instance = new scope.managerParentTransport();
  const a = {};
  const b = {};
  const first = instance.getAPI(a).managerParentTransport;
  const second = instance.getAPI(b).managerParentTransport;
  assert.equal(owners.length, 1);
  assert.equal(await first.open(), 1);
  assert.equal(await second.ready(1), true);
  assert.deepEqual(calls.slice(0, 2), [
    ["open", a],
    ["ready", b, 1],
  ]);
  const before = calls.length;
  for (const [method, args] of [
    ["open", ["host"]],
    ["ready", []],
    ["read", [1, {}]],
    ["postMessage", [1, {}, "verdict"]],
    ["close", [1, "host"]],
  ]) {
    await assert.rejects(first[method](...args), /^Error: Download Manager parent API refused$/u);
  }
  assert.equal(calls.length, before);
  await assert.rejects(first.close(1), /^Error: Download Manager parent API refused$/u);
  assert.deepEqual(
    imports,
    ["NativeManifests", "Subprocess", "AsyncShutdown", "AppConstants", "Timer"].map(
      (name) => `resource://gre/modules/${name}.sys.mjs`,
    ),
  );
  instance.onShutdown();
  instance.onShutdown();
  await assert.rejects(first.open(), /parent API refused/u);
  assert.throws(() => instance.getAPI(a), /parent API refused/u);
  assert.equal(calls.filter((call) => call[0] === "shutdown").length, 1);
});

test("actual build-selected provider never downgrades when runtime manifest lacks experiment metadata", async () => {
  const builder = readFileSync("scripts/build-parent-candidate.mjs", "utf8");
  const define = builder.match(/define: \{ __DM_PARENT_TRANSPORT__: "(true|false)" \}/u)?.[1];
  for (const file of [
    "scripts/build-extension.mjs",
    "scripts/build-capture-candidate.mjs",
    "scripts/build-handoff-probe.mjs",
  ])
    assert.match(readFileSync(file, "utf8"), /define: \{ __DM_PARENT_TRANSPORT__: "false" \}/u);
  const bundle = buildSync({
    stdin: {
      contents: 'export {createNativeConnection} from "./extension/src/native-provider";',
      resolveDir: process.cwd(),
    },
    bundle: true,
    write: false,
    format: "esm",
    platform: "node",
    define: { __DM_PARENT_TRANSPORT__: define },
  });
  const { createNativeConnection } = await import(
    `data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].text).toString("base64")}`
  );
  const previous = Object.getOwnPropertyDescriptor(globalThis, "browser");
  let ordinary = 0;
  let client;
  try {
    globalThis.browser = {
      runtime: {
        getManifest: () => ({ version: "0.3.0" }),
        connectNative: () => {
          ordinary++;
          throw Error("ordinary refused");
        },
      },
    };
    client = createNativeConnection();
    await assert.rejects(client.connect());
    assert.equal(ordinary, 0);
    assert.equal(define, "true");
  } finally {
    client?.disconnect();
    await new Promise((resolve) => setImmediate(resolve));
    if (previous) Object.defineProperty(globalThis, "browser", previous);
    else delete globalThis.browser;
  }
});
