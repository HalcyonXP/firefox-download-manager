import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { runInNewContext } from "node:vm";
import test from "node:test";

const source = readFileSync(
  new URL("../extension/protection-probe/api.js", import.meta.url),
  "utf8",
);
function fixture(dispatch = () => {}) {
  const queries = [];
  const operations = [];
  const closes = [];
  const extension = { baseURI: { resolve: (name) => `moz-extension://owned/${name}` } };
  const context = {
    extension,
    envType: "addon_parent",
    viewType: "tab",
    isTopContext: true,
    incognito: false,
    uri: { spec: extension.baseURI.resolve("probe.html") },
    callOnClose: (hook) => closes.push(hook),
  };
  const redirects = {};
  const scope = {
    ExtensionAPI: class {
      constructor(value) {
        this.extension = value;
      }
    },
    Ci: {
      nsIApplicationReputationService: "service",
      nsIIOService: "io",
      nsIMutableArray: "array",
    },
    Cc: {
      "@mozilla.org/reputationservice/application-reputation-service;1": {
        getService: (kind) => {
          operations.push("service");
          assert.equal(kind, "service");
          return {
            queryReputation: (query, callback) => {
              queries.push({ query, callback });
              dispatch(callback);
            },
          };
        },
      },
      "@mozilla.org/network/io-service;1": {
        getService: (kind) => {
          operations.push("uri");
          assert.equal(kind, "io");
          return { newURI: (spec) => ({ spec }) };
        },
      },
      "@mozilla.org/array;1": {
        createInstance: (kind) => {
          operations.push("array");
          assert.equal(kind, "array");
          return redirects;
        },
      },
    },
  };
  runInNewContext(source, scope, { timeout: 1000 });
  const instance = new scope.managerProtection(extension);
  return {
    instance,
    context,
    queries,
    operations,
    closes,
    redirects,
    api: () => instance.getAPI(context).managerProtection,
  };
}
const plain = (value) => JSON.parse(JSON.stringify(value));

test("fixed query binds exact empty bytes, unsigned metadata and loopback source; one dispatch only", async () => {
  const f = fixture();
  const api = f.api();
  assert.equal((await api.snapshot()).stage, "idle");
  assert.equal((await api.start()).stage, "pending");
  await Promise.all(Array.from({ length: 32 }, () => api.start()));
  assert.equal(f.queries.length, 1);
  const q = f.queries[0].query;
  assert.deepEqual(
    Object.keys(q).sort(),
    [
      "sourceURI",
      "referrerInfo",
      "suggestedFileName",
      "fileSize",
      "sha256Hash",
      "signatureInfo",
      "redirects",
    ].sort(),
  );
  assert.equal(q.sourceURI.spec, "http://127.0.0.1/download-manager-protection-probe.txt");
  assert.equal(q.referrerInfo, null);
  assert.equal(q.fileSize, 0);
  assert.equal(q.suggestedFileName, "download-manager-protection-probe.txt");
  assert.equal(
    Buffer.from(q.sha256Hash, "latin1").toString("hex"),
    createHash("sha256").update("").digest("hex"),
  );
  assert.deepEqual(plain(q.signatureInfo), []);
  assert.equal(q.redirects, f.redirects);
  f.queries[0].callback(false, 0, 0);
  assert.deepEqual(plain(await api.snapshot()), {
    version: 1,
    qualification: false,
    scope: "fixed-empty-loopback-text",
    stage: "settled",
    result: "not-blocked",
    attempted: true,
    callbacks: 1,
  });
  await api.start();
  assert.equal(f.queries.length, 1);
});

test("all caller boundaries refuse before any privileged service construction", () => {
  for (const patch of [
    { extension: {} },
    { envType: "content_parent" },
    { viewType: "background" },
    { isTopContext: false },
    { incognito: true },
    { incognito: undefined },
    { uri: { spec: "https://example.invalid/probe.html" } },
    { uri: { spec: "moz-extension://owned/probe.html?path=elsewhere" } },
  ]) {
    const f = fixture();
    Object.assign(f.context, patch);
    assert.throws(() => f.api(), /caller refused/);
    assert.equal(f.queries.length, 0);
    assert.equal(f.operations.length, 0);
  }
  const f = fixture();
  f.api();
  assert.throws(() => f.instance.getAPI({ ...f.context }), /caller refused/);
});

test("no argument can supply a path, URL, digest or alternate operation", async () => {
  const f = fixture();
  const api = f.api();
  for (const value of [undefined, null, "file:///other", { url: "https://example.invalid" }]) {
    await assert.rejects(api.start(value), /arguments refused/);
    await assert.rejects(api.snapshot(value), /arguments refused/);
  }
  assert.equal(f.queries.length, 0);
  assert.deepEqual(Object.keys(api).sort(), ["snapshot", "start"]);
});

test("errors, malformed status and contradictory callbacks never become not-blocked", async () => {
  for (const args of [
    [false, 1, 0],
    [false, "0", 0],
    [false, 0, 5],
    [false, 0, -1],
    [false, 0, 1.5],
    [true, 0, 0],
    [0, 0, 0],
    [false, 0, undefined],
  ]) {
    const f = fixture();
    const api = f.api();
    await api.start();
    f.queries[0].callback(...args);
    assert.equal((await api.snapshot()).result, "unavailable");
  }
  for (const verdict of [1, 2, 3, 4]) {
    const f = fixture((cb) => cb(true, 0, verdict));
    assert.equal((await f.api().start()).result, "blocked");
  }
  // A configured nonblocking verdict is distinct from VERDICT_SAFE.
  const f = fixture((cb) => cb(false, 0, 2));
  assert.equal((await f.api().start()).result, "not-blocked");
});

test("uncertain throw and duplicate callbacks consume the attempt and stay unavailable", async () => {
  const f = fixture(() => {
    throw new Error("private provider details");
  });
  const api = f.api();
  assert.equal((await api.start()).result, "unavailable");
  f.queries[0].callback(false, 0, 0);
  await api.start();
  assert.equal((await api.snapshot()).result, "unavailable");
  assert.equal(f.queries.length, 1);
  const g = fixture();
  const other = g.api();
  await other.start();
  for (let i = 0; i < 10; i++) g.queries[0].callback(false, 0, 0);
  assert.equal((await other.snapshot()).result, "unavailable");
  assert.equal((await other.snapshot()).callbacks, 2);
});

test("context and extension shutdown revoke observation without pretending to cancel the service", async () => {
  for (const closeContext of [false, true]) {
    const f = fixture();
    const api = f.api();
    await api.start();
    if (closeContext) f.closes[0].close();
    else f.instance.onShutdown();
    f.queries[0].callback(false, 0, 0);
    const result = await api.start();
    assert.equal(result.stage, "closed");
    assert.equal(result.result, "unavailable");
    assert.equal(f.queries.length, 1);
    assert.throws(() => f.api(), /caller refused/);
  }
});

test("schema exposes only two no-argument observations; source has no general privileged surfaces", () => {
  const schema = JSON.parse(
    readFileSync(new URL("../extension/protection-probe/schema.json", import.meta.url), "utf8"),
  );
  assert.equal(schema.length, 1);
  assert.equal(schema[0].namespace, "managerProtection");
  assert.deepEqual(
    schema[0].functions.map((f) => [f.name, f.parameters]),
    [
      ["start", []],
      ["snapshot", []],
    ],
  );
  assert.equal(schema[0].types[0].additionalProperties, false);
  for (const forbidden of [
    /IOUtils/u,
    /PathUtils/u,
    /Services\.prefs/u,
    /newChannel/u,
    /fetch\(/u,
    /XMLHttpRequest/u,
    /Subprocess/u,
    /console\./u,
    /importESModule/u,
  ])
    assert.doesNotMatch(source, forbidden);
  const page = readFileSync(
    new URL("../extension/protection-probe/probe.js", import.meta.url),
    "utf8",
  );
  assert.equal((page.match(/managerProtection\.start\(/gu) ?? []).length, 1);
  assert.match(page, /count < 150/u);
  assert.doesNotMatch(page, /eval\(|innerHTML|fetch\(/u);
});

test("Firefox API schema uses integer bounds and no synchronous returns on async functions", () => {
  const schema = JSON.parse(
    readFileSync(new URL("../extension/protection-probe/schema.json", import.meta.url), "utf8"),
  )[0];
  // Matching Schemas.sys.mjs IntegerType and FunctionType rules, not generic JSON Schema.
  for (const property of Object.values(schema.types[0].properties)) {
    if (property.type === "integer") assert.equal("enum" in property, false);
  }
  assert.deepEqual(schema.types[0].properties.version, { type: "integer", minimum: 1, maximum: 1 });
  for (const fn of schema.functions) {
    assert.equal(fn.async, true);
    assert.equal("returns" in fn, false);
    assert.deepEqual(fn.parameters, []);
  }
});

const loaderSource = readFileSync(
  new URL("./qualification/protection_loader.js", import.meta.url),
  "utf8",
);
async function loadModel({
  addon = { id: "owned" },
  error,
  bootstrap = false,
  fileInit = false,
  sync = false,
} = {}) {
  let calls = 0;
  const replies = [];
  const promise = new Promise((resolve) => {
    runInNewContext(`(function(){${loaderSource}}).apply(null, args)`, {
      args: [
        "owned.xpi",
        "owned",
        (value) => {
          replies.push(JSON.parse(JSON.stringify(value)));
          resolve();
        },
      ],
      ChromeUtils: {
        importESModule(path) {
          assert.equal(path, "resource://gre/modules/AddonManager.sys.mjs");
          if (bootstrap) throw error;
          return {
            AddonManager: {
              installTemporaryAddon() {
                calls++;
                if (sync) throw error;
                return error ? Promise.reject(error) : Promise.resolve(addon);
              },
            },
          };
        },
      },
      Components: {
        interfaces: { nsIFile: 1 },
        classes: {
          "@mozilla.org/file/local;1": {
            createInstance() {
              return {
                initWithPath(path) {
                  assert.equal(path, "owned.xpi");
                  if (fileInit) throw error;
                },
              };
            },
          },
        },
      },
    });
  });
  await promise;
  assert.equal(replies.length, 1);
  return { calls, value: replies[0] };
}

test("temporary loader distinguishes identity, refusal and dispatch stages without retry", async () => {
  const loaded = await loadModel();
  assert.equal(loaded.calls, 1);
  assert.deepEqual(loaded.value, {
    version: 1,
    state: "loaded",
    phase: "identity",
    terms: [],
    complete: true,
  });
  assert.equal((await loadModel({ addon: { id: "foreign" } })).value.state, "refused");
  for (const [options, phase, calls] of [
    [{ bootstrap: true }, "bootstrap", 0],
    [{ fileInit: true }, "file-init", 0],
    [{ sync: true }, "install", 1],
    [{}, "install", 1],
  ]) {
    const result = await loadModel({ ...options, error: new Error("Extension is invalid") });
    assert.equal(result.calls, calls);
    assert.deepEqual(result.value, {
      version: 1,
      state: "refused",
      phase,
      terms: ["invalid-extension"],
      complete: true,
    });
  }
});

test("temporary loader records only bounded fixed terms from the exact load error", async () => {
  const error = new Error("Extension is invalid");
  error.additionalErrors = [
    "Using 'experiment_apis' requires a privileged add-on. opaque-fixture-value",
  ];
  const result = await loadModel({ error });
  assert.deepEqual(result.value.terms, [
    "experiment-apis",
    "invalid-extension",
    "privilege-required",
  ]);
  assert.equal(result.value.complete, true);
  assert.equal(JSON.stringify(result).includes("opaque-fixture-value"), false);
  for (const additionalErrors of [
    Array(9).fill("enum"),
    ["x".repeat(2049)],
    [null],
    "not-an-array",
  ]) {
    error.additionalErrors = additionalErrors;
    assert.equal((await loadModel({ error })).value.complete, false);
  }
  const unreadable = {
    get message() {
      throw new Error("opaque-fixture-value");
    },
  };
  assert.deepEqual((await loadModel({ error: unreadable })).value.terms, []);
  assert.equal((await loadModel({ error: unreadable })).value.complete, false);
});

test("owned policy snapshot reads effective/default/user branches without preference writes", () => {
  const python = readFileSync(
    new URL("./qualification/firefox_policy.py", import.meta.url),
    "utf8",
  );
  const snapshot = python.match(/SNAPSHOT = """([\s\S]*?)"""/u)[1];
  const names = [
    "xpinstall.signatures.required",
    "extensions.experiments.enabled",
    "browser.safebrowsing.malware.enabled",
    "browser.safebrowsing.phishing.enabled",
    "browser.safebrowsing.downloads.enabled",
    "browser.safebrowsing.downloads.remote.enabled",
    "browser.safebrowsing.blockedURIs.enabled",
    "app.update.disabledForTesting",
    "extensions.update.enabled",
    "extensions.systemAddon.update.enabled",
  ];
  for (const name of names) assert.ok(python.includes(`'${name}'`));
  const defaults = Object.fromEntries(names.map((name) => [name, true]));
  defaults["extensions.experiments.enabled"] = false;
  const values = {
    ...defaults,
    "remote.prefs.recommended": false,
    "browser.safebrowsing.downloads.enabled": false,
  };
  const branch = (data) => ({
    getPrefType(name) {
      return !(name in data) ? 0 : typeof data[name] === "boolean" ? 128 : 32;
    },
    getBoolPref(name) {
      assert.equal(typeof data[name], "boolean");
      return data[name];
    },
  });
  const context = {
    args: [names],
    Services: {
      prefs: {
        ...branch(values),
        getDefaultBranch(prefix) {
          assert.equal(prefix, "");
          return branch(defaults);
        },
        prefHasUserValue(name) {
          return name === "browser.safebrowsing.downloads.enabled";
        },
      },
    },
  };
  const execute = () =>
    JSON.parse(
      JSON.stringify(runInNewContext(`(function(){${snapshot}}).apply(null, args)`, context)),
    );
  const observed = execute();
  assert.equal(observed.recommended, false);
  assert.equal(observed.applied, null);
  assert.deepEqual(Object.keys(observed.preferences), names);
  assert.deepEqual(observed.preferences["browser.safebrowsing.downloads.enabled"], {
    value: false,
    default: true,
    user: true,
  });
  assert.deepEqual(observed.preferences["extensions.experiments.enabled"], {
    value: false,
    default: false,
    user: false,
  });
  values["remote.prefs.recommended.applied"] = true;
  assert.equal(execute().applied, true);
  values["extensions.experiments.enabled"] = "malformed";
  assert.throws(execute, /owned policy preference type refused/u);
});
