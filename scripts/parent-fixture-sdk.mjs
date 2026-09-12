// SDK-shaped model for the actual compiled bootstrap/launcher/transport bundle.
// No Firefox, native process, registry, download or policy operation is performed.
import assert from "node:assert/strict";
import vm from "node:vm";
import { win32 as path } from "node:path";

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
const flush = () => new Promise((resolve) => setImmediate(resolve));
const plain = (value) => JSON.parse(JSON.stringify(value));

export async function verifyBundledFixture(source, nonce, command) {
  const closes = [];
  const calls = [];
  const writes = [];
  const observations = [];
  const imports = [];
  const blockers = new Set();
  const hooks = new Set();
  const listeners = new Map();
  const tracked = [];
  const timers = new Set();
  const exit = deferred();
  const closed = deferred();
  const extension = {
    id: "download-manager@halcyonxp.local",
    hasShutdown: false,
    persistentBackground: false,
    hasPermission: (name) => name === "nativeMessaging",
    baseURI: { resolve: (name) => `moz-extension://owned/${name}` },
    callOnClose: (hook) => hooks.add(hook),
    forgetOnClose: (hook) => hooks.delete(hook),
    on: (name, callback) => listeners.set(name, callback),
    off: (name, callback) => {
      assert.equal(listeners.get(name), callback);
      listeners.delete(name);
    },
  };
  const context = {
    extension,
    active: true,
    unloaded: false,
    envType: "addon_parent",
    viewType: "background",
    isBackgroundContext: true,
    isTopContext: true,
    incognito: false,
    uri: { spec: extension.baseURI.resolve("_generated_background_page.html") },
    activeNativePorts: new WeakSet(),
    trackNativeAppPort: (owner) => {
      assert.equal(owner.native, true);
      tracked.push(owner);
      context.activeNativePorts.add(owner);
    },
    untrackNativeAppPort: (owner) => context.activeNativePorts.delete(owner),
    callOnClose: (hook) => hooks.add(hook),
    forgetOnClose: (hook) => hooks.delete(hook),
  };
  function pipe(name) {
    let buffered = Buffer.alloc(0);
    let pending = null;
    const deliver = () => {
      if (pending !== null && buffered.length >= pending.length) {
        const { length, resolve } = pending;
        pending = null;
        const bytes = Uint8Array.from(buffered.subarray(0, length));
        buffered = buffered.subarray(length);
        resolve(bytes.buffer);
      }
    };
    return {
      push(value) {
        buffered = Buffer.concat([buffered, value]);
        deliver();
      },
      read(length) {
        assert.equal(pending, null);
        const read = deferred();
        pending = { ...read, length };
        deliver();
        return read.promise;
      },
      close(force) {
        assert.equal(force, true);
        closes.push(name);
        pending?.reject(new Error("modeled actual pipe closure"));
        pending = null;
        buffered = Buffer.alloc(0);
        if (name === "stdin") exit.resolve({ exitCode: 0 });
        return closed.promise;
      },
    };
  }
  const stdout = pipe("stdout");
  function message(value) {
    const body = Buffer.from(JSON.stringify(value));
    const frame = Buffer.alloc(body.length + 4);
    frame.writeUInt32LE(body.length);
    body.copy(frame, 4);
    stdout.push(frame);
  }
  const process = {
    pid: 44,
    stdout,
    stderr: pipe("stderr"),
    stdin: {
      ...pipe("stdin"),
      write: (bytes) => {
        const frame = Buffer.from(bytes);
        assert.equal(frame.readUInt32LE(), frame.length - 4);
        const value = JSON.parse(frame.subarray(4).toString("utf8"));
        writes.push(value);
        message({ fixture: "owned-parent-stdio-v1", op: "pong", value: "π" });
        const length = bytes.length;
        structuredClone(bytes.buffer, { transfer: [bytes.buffer] });
        return Promise.resolve({ bytesWritten: length });
      },
    },
    wait: () => exit.promise,
    kill: () => assert.fail("modeled fixture must not require forced retirement"),
  };
  const modules = {
    "resource://gre/modules/Subprocess.sys.mjs": {
      Subprocess: {
        call: (options) => {
          calls.push(plain(options));
          message({ fixture: "owned-parent-stdio-v1", op: "ready", pid: 44 });
          return Promise.resolve(process);
        },
      },
    },
    "resource://gre/modules/AsyncShutdown.sys.mjs": {
      AsyncShutdown: {
        profileBeforeChange: {
          isClosed: false,
          addBlocker: (name, blocker) => blockers.add(blocker),
          removeBlocker: (blocker) => blockers.delete(blocker),
        },
      },
    },
    "resource://gre/modules/AppConstants.sys.mjs": { AppConstants: { platform: "win" } },
    "resource://gre/modules/Timer.sys.mjs": {
      setTimeout: (callback, delay) => {
        assert.equal(delay, 3000);
        timers.add(callback);
        return callback;
      },
      clearTimeout: (callback) => timers.delete(callback),
    },
  };
  const sandbox = vm.createContext({
    ExtensionAPI: class {
      constructor(value) {
        this.extension = value;
      }
    },
    ChromeUtils: {
      importESModule: (name) => {
        imports.push(name);
        assert.ok(Object.hasOwn(modules, name));
        return modules[name];
      },
    },
    PathUtils: {
      parent: path.dirname,
      filename: path.basename,
      join: path.join,
      isAbsolute: path.isAbsolute,
    },
    Services: {
      obs: {
        notifyObservers: (subject, topic, data) => {
          assert.equal(subject, null);
          assert.equal(topic, "download-manager-owned-parent-fixture");
          observations.push(JSON.parse(data));
        },
      },
    },
  });
  sandbox.Cu = {
    importGlobalProperties: (names) => {
      assert.deepEqual(plain(names), ["TextEncoder", "TextDecoder"]);
      Object.assign(sandbox, { TextEncoder, TextDecoder });
    },
  };
  vm.runInContext(source, sandbox);
  const api = new sandbox.managerParentProbe(extension);
  const caller = api.getAPI(context).managerParentProbe;
  let result = null;
  let error = null;
  let running = Promise.resolve();
  try {
    await assert.rejects(caller.run({ host: "other-host" }), /fixture API refused/u);
    assert.equal(calls.length, 0);
    running = caller.run().then(
      (value) => {
        result = value;
      },
      (value) => {
        error = value;
      },
    );
    for (let count = 0; count < 20 && result === null && error === null; count++) await flush();
    assert.equal(error, null, "compiled fixture refused its modeled SDK");
    assert.equal(result?.stage, "echoed");
    assert.equal(result.qualification, false);
    assert.equal(observations.length, 1);
    assert.equal(observations[0].nonce, nonce);
    assert.equal(observations[0].kind, "ready");
    assert.deepEqual(writes, [{ fixture: "owned-parent-stdio-v1", op: "ping", value: "π" }]);
    assert.deepEqual(calls, [
      {
        command,
        arguments: [
          "--browser-parent",
          path.join(path.dirname(command), "com.halcyonxp.firefox_download_manager.json"),
          extension.id,
        ],
        workdir: path.dirname(command),
        stderr: "pipe",
        disclaim: true,
      },
    ]);
    assert.equal(tracked.length, 1);
    assert.equal(context.activeNativePorts.has(tracked[0]), true);
    assert.equal(blockers.size, 1);
    api.onShutdown();
    await flush();
    assert.deepEqual(closes.slice().sort(), ["stderr", "stdin", "stdout"]);
    assert.equal(observations.length, 1, "early pipe rejection is not retirement");
    assert.equal(context.activeNativePorts.has(tracked[0]), true);
    assert.equal(blockers.size, 1);
  } finally {
    api.onShutdown();
    // Retire these modeled context/extension hooks even if an API assertion
    // failed. This is not enumeration of any browser or native-port registry.
    for (const hook of [...hooks]) hook.close();
    closed.resolve();
    await running;
    for (let count = 0; count < 20 && observations.at(-1)?.kind !== "retired"; count++)
      await flush();
  }
  const receipt = observations.at(-1);
  assert.equal(receipt.kind, "retired");
  assert.equal(receipt.nonce, nonce);
  assert.equal(receipt.value.successful, true);
  assert.equal(receipt.value.launcher.transport.process_waited, true);
  assert.equal(receipt.value.launcher.transport.pipes_closed, true);
  assert.equal(receipt.value.launcher.transport.forced, false);
  assert.deepEqual(closes.sort(), ["stderr", "stdin", "stdout"]);
  assert.equal(context.activeNativePorts.has(tracked[0]), false);
  assert.equal(blockers.size + hooks.size + listeners.size + timers.size, 0);
  assert.equal(imports.length, 4);
  await assert.rejects(caller.run(), /fixture API refused/u);
  assert.equal(calls.length, 1);
}
