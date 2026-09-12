import assert from "node:assert/strict";
import test from "node:test";
import vm from "node:vm";
import { win32 as path } from "node:path";
import {
  FixedParentLauncher,
  firefoxLauncherPlatform,
} from "../extension/protection-bridge/parent-launcher.js";

const HOST = "com.halcyonxp.firefox_download_manager";
const ID = "download-manager@halcyonxp.local";
const flush = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

function fixture({ holdLookup = false, holdSpawn = false, holdPipes = false, onDisconnect } = {}) {
  const extensionHooks = new Set();
  const contextHooks = new Set();
  const blockers = new Set();
  const events = new Map();
  const nativePorts = new WeakSet();
  const nativePortCalls = [];
  const calls = [];
  const lookups = [];
  const writes = [];
  const closes = [];
  const waits = [];
  const timers = new Set();
  const lookup = deferred();
  const spawn = deferred();
  const exit = deferred();
  const pipes = deferred();
  if (!holdPipes) pipes.resolve();
  const extension = {
    id: ID,
    hasShutdown: false,
    persistentBackground: false,
    permitted: true,
    hasPermission: (name) => name === "nativeMessaging" && extension.permitted,
    baseURI: { resolve: (file) => `moz-extension://owned/${file}` },
    callOnClose: (hook) => extensionHooks.add(hook),
    forgetOnClose: (hook) => extensionHooks.delete(hook),
    on: (name, callback) => events.set(name, callback),
    off: (name, callback) => {
      assert.equal(events.get(name), callback);
      events.delete(name);
    },
  };
  const context = {
    extension,
    active: true,
    unloaded: false,
    envType: "addon_parent",
    viewType: "background",
    isBackgroundContext: true,
    activeNativePorts: nativePorts,
    trackNativeAppPort: (port) => {
      nativePortCalls.push(["track", port]);
      if (port.native && context.isBackgroundContext && !extension.persistentBackground)
        context.activeNativePorts.add(port);
    },
    untrackNativeAppPort: (port) => {
      nativePortCalls.push(["untrack", port]);
      context.activeNativePorts.delete(port);
    },
    isTopContext: true,
    incognito: false,
    uri: { spec: extension.baseURI.resolve("_generated_background_page.html") },
    callOnClose: (hook) => contextHooks.add(hook),
    forgetOnClose: (hook) => contextHooks.delete(hook),
  };
  const info = {
    path: `C:\\owned\\${HOST}.json`,
    manifest: {
      name: HOST,
      type: "stdio",
      path: "C:\\owned\\download-manager-native-host.exe",
      allowed_extensions: [ID],
    },
  };
  const pipe = (name) => {
    const pending = [];
    return {
      read() {
        const read = deferred();
        pending.push(read);
        return read.promise;
      },
      write(bytes) {
        writes.push(bytes.slice());
        return Promise.resolve({ bytesWritten: bytes.length });
      },
      close(force) {
        assert.equal(force, true);
        closes.push(name);
        for (const read of pending) read.reject(new Error("opaque EOF"));
        if (name === "stdin") exit.resolve({ exitCode: 0 });
        return pipes.promise;
      },
    };
  };
  const process = {
    stdin: pipe("stdin"),
    stdout: pipe("stdout"),
    stderr: pipe("stderr"),
    wait: () => {
      waits.push(true);
      return exit.promise;
    },
    kill: () => {
      assert.fail("unexpected forced retirement");
    },
  };
  const shutdown = {
    isClosed: false,
    addBlocker: (name, callback) => {
      assert.equal(name, "Download Manager parent transport");
      blockers.add(callback);
    },
    removeBlocker: (callback) => blockers.delete(callback),
  };
  const platform = firefoxLauncherPlatform(
    {
      lookupManifest: (...args) => {
        lookups.push(args);
        return lookup.promise;
      },
    },
    {
      call: (options) => {
        calls.push(options);
        return spawn.promise;
      },
    },
    { profileBeforeChange: shutdown },
    { isAbsolute: path.isAbsolute, parent: path.dirname, filename: path.basename, join: path.join },
    { platform: "win" },
    {
      setTimeout: (callback, delay) => {
        assert.equal(delay, 3000);
        timers.add(callback);
        return callback;
      },
      clearTimeout: (id) => timers.delete(id),
    },
  );
  if (!holdLookup) lookup.resolve(info);
  if (!holdSpawn) spawn.resolve(process);
  let notifications = 0;
  const owner = new FixedParentLauncher(extension, platform, {
    onMessage: () => {},
    onDisconnect: () => {
      notifications++;
      return onDisconnect?.(owner);
    },
  });
  const hooks = () => contextHooks.size + extensionHooks.size + blockers.size + events.size;
  const finish = async () => {
    lookup.resolve(info);
    spawn.resolve(process);
    pipes.resolve();
    return owner.close();
  };
  return {
    owner,
    extension,
    context,
    info,
    lookup,
    spawn,
    exit,
    pipes,
    process,
    platform,
    contextHooks,
    extensionHooks,
    blockers,
    events,
    nativePorts,
    nativePortCalls,
    calls,
    lookups,
    writes,
    closes,
    waits,
    timers,
    shutdown,
    hooks,
    finish,
    notifications: () => notifications,
  };
}

test("fixed SDK lookup and private arguments are pinned before one retained launch", async () => {
  const f = fixture();
  const first = f.owner.start(f.context);
  assert.equal(f.owner.start(f.context), first);
  assert.equal(await first, true);
  assert.equal(f.hooks(), 4);
  assert.deepEqual(f.lookups, [["stdio", HOST, f.context]]);
  assert.deepEqual(f.calls, [
    {
      command: f.info.manifest.path,
      arguments: ["--browser-parent", f.info.path, ID],
      workdir: "C:\\owned",
      stderr: "pipe",
      disclaim: true,
    },
  ]);
  assert.equal(Object.isFrozen(f.calls[0]), true);
  assert.equal(Object.isFrozen(f.calls[0].arguments), true);
  assert.throws(() => JSON.stringify(f.owner), /not serializable/u);
  const receipt = await f.finish();
  assert.equal(receipt.successful, true);
  assert.equal(receipt.spawn_called, true);
  assert.equal(receipt.transport.process_waited, true);
  assert.equal(receipt.transport.pipes_closed, true);
  assert.equal(f.waits.length, 1);
  assert.deepEqual(f.closes, ["stdin", "stdout", "stderr"]);
  assert.equal(f.hooks(), 0);
  assert.equal(f.timers.size, 0);
  assert.equal(f.notifications(), 1);
  assert.equal(await f.owner.start(f.context), false);
  assert.equal(f.calls.length, 1);
});

test("wrong caller and exact lifetime or nativeMessaging failures precede every hook and lookup", async () => {
  for (const change of [
    (f) => {
      f.context.extension = {};
    },
    (f) => {
      f.context.envType = "content_parent";
    },
    (f) => {
      f.context.viewType = "tab";
    },
    (f) => {
      f.context.isTopContext = 1;
    },
    (f) => {
      f.context.incognito = true;
    },
    (f) => {
      f.context.uri.spec += "?other";
    },
    (f) => {
      f.context.active = 1;
    },
    (f) => {
      f.context.unloaded = undefined;
    },
    (f) => {
      f.extension.hasShutdown = true;
    },
    (f) => {
      f.extension.permitted = false;
    },
    (f) => {
      f.extension.id = "foreign";
    },
  ]) {
    const f = fixture();
    change(f);
    assert.throws(() => f.owner.start(f.context), /parent launch refused/u);
    const receipt = await f.finish();
    assert.equal(receipt.spawn_called, false);
    assert.equal(f.lookups.length, 0);
    assert.equal(f.calls.length, 0);
    assert.equal(f.hooks(), 0);
  }
});

test("context extension permission and shutdown hooks revoke pending lookup without a native call", async () => {
  for (const kind of ["context", "extension", "permission", "shutdown"]) {
    const f = fixture({ holdLookup: true });
    const starting = f.owner.start(f.context);
    await flush();
    let barrier;
    if (kind === "context") {
      f.context.unloaded = true;
      [...f.contextHooks][0].close();
    }
    if (kind === "extension") {
      f.extension.hasShutdown = true;
      [...f.extensionHooks][0].close();
    }
    if (kind === "permission") f.events.get("remove-permissions")();
    if (kind === "shutdown") barrier = [...f.blockers][0]();
    const retiring = f.owner.close();
    let settled = false;
    void retiring.then(() => {
      settled = true;
    });
    await flush();
    assert.equal(settled, false);
    f.lookup.resolve(f.info);
    assert.equal(await starting, false);
    const receipt = await retiring;
    if (barrier) await barrier;
    assert.equal(receipt.successful, true);
    assert.equal(receipt.spawn_called, false);
    assert.equal(f.calls.length, 0);
    assert.equal(f.hooks(), 0);
  }
});

test("late process ownership and actual pipe closure remain retained beyond context close", async () => {
  const f = fixture({ holdSpawn: true, holdPipes: true });
  const starting = f.owner.start(f.context);
  await flush();
  assert.equal(f.calls.length, 1);
  const retiring = f.owner.close();
  let settled = false;
  void retiring.then(() => {
    settled = true;
  });
  await flush();
  assert.equal(settled, false);
  f.spawn.resolve(f.process);
  assert.equal(await starting, false);
  await flush();
  assert.equal(settled, false);
  assert.equal(f.hooks(), 4);
  f.pipes.resolve();
  const receipt = await retiring;
  assert.equal(receipt.successful, true);
  assert.equal(receipt.transport.process_waited, true);
  assert.equal(receipt.transport.pipes_closed, true);
  assert.equal(f.hooks(), 0);
});

test("invalid manifest path host type and allowlist refuse before native invocation", async () => {
  for (const change of [
    (i) => {
      i.manifest.name = "foreign";
    },
    (i) => {
      i.manifest.type = "storage";
    },
    (i) => {
      i.manifest.allowed_extensions = [];
    },
    (i) => {
      i.manifest.allowed_extensions.push("foreign");
    },
    (i) => {
      i.manifest.path = "download-manager-native-host.exe";
    },
    (i) => {
      i.manifest.path = "\\\\server\\share\\download-manager-native-host.exe";
    },
    (i) => {
      i.manifest.path = "C:\\owned\\cmd.exe";
    },
    (i) => {
      i.path = `C:\\other\\${HOST}.json`;
    },
    (i) => {
      i.manifest.path = "C:\\owned\\..\\owned\\download-manager-native-host.exe";
    },
    (i) => {
      i.manifest.path += "\0";
    },
  ]) {
    const f = fixture({ holdLookup: true });
    change(f.info);
    const starting = f.owner.start(f.context);
    f.lookup.resolve(f.info);
    assert.equal(await starting, false);
    const receipt = await f.finish();
    assert.equal(receipt.spawn_called, false);
    assert.equal(f.calls.length, 0);
    assert.equal(f.hooks(), 0);
  }
});

test("metadata and JSON getters cannot bypass final authority or revive a closed owner", async () => {
  const f = fixture({ holdLookup: true });
  const starting = f.owner.start(f.context);
  await flush();
  const original = f.info.manifest.path;
  Object.defineProperty(f.info.manifest, "path", {
    get() {
      void f.owner.close();
      return original;
    },
  });
  f.lookup.resolve(f.info);
  assert.equal(await starting, false);
  await f.finish();
  assert.equal(f.calls.length, 0);
  const g = fixture();
  assert.equal(await g.owner.start(g.context), true);
  assert.throws(
    () =>
      g.owner.postMessage(g.context, {
        toJSON() {
          g.context.active = false;
          return { permit: true };
        },
      }),
    /parent launch refused/u,
  );
  assert.equal(g.writes.length, 0);
  await g.finish();
});

test("rejected SDK startup remains indeterminate without replacement or fabricated joins", async () => {
  const f = fixture({ holdSpawn: true });
  const starting = f.owner.start(f.context);
  await flush();
  f.spawn.reject(new Error("opaque post-creation failure"));
  assert.equal(await starting, false);
  const receipt = await f.owner.close();
  assert.equal(receipt.spawn_called, true);
  assert.equal(receipt.transport.startup, "indeterminate");
  assert.equal(receipt.transport.process_waited, false);
  assert.equal(receipt.successful, false);
  assert.equal(await f.owner.start(f.context), false);
  assert.equal(f.calls.length, 1);
  assert.equal(f.hooks(), 1); // Failed owner remains registered with shutdown.
});

test("partial hook registration and cleanup failures remain unsuccessful and reentrant close is singular", async () => {
  const f = fixture();
  f.context.callOnClose = (hook) => {
    f.contextHooks.add(hook);
    throw new Error("opaque registration failure");
  };
  assert.equal(await f.owner.start(f.context), false);
  const failed = await f.finish();
  assert.equal(failed.successful, false);
  assert.equal(f.calls.length, 0);
  assert.equal(f.hooks(), 1);
  let reentered;
  const g = fixture({
    onDisconnect: (owner) => {
      reentered = owner.close();
    },
  });
  assert.equal(await g.owner.start(g.context), true);
  g.context.forgetOnClose = () => {
    throw new Error("opaque cleanup failure");
  };
  const closing = g.owner.close();
  assert.equal(reentered, closing);
  const receipt = await closing;
  assert.equal(receipt.successful, false);
  assert.equal(receipt.hooks_removed, false);
  assert.equal(g.blockers.size, 1);
  assert.equal(g.extensionHooks.size, 0);
  assert.equal(g.events.size, 0);
  assert.equal(g.notifications(), 1);
});

test("failed native retirement retains its shutdown blocker instead of silently disarming failure", async () => {
  const f = fixture({ holdSpawn: true });
  const starting = f.owner.start(f.context);
  await flush();
  f.spawn.reject(new Error("opaque indeterminate startup"));
  assert.equal(await starting, false);
  const receipt = await f.owner.close();
  assert.equal(receipt.successful, false);
  assert.equal(f.blockers.size, 1);
  await assert.rejects([...f.blockers][0](), /parent launch refused/u);
});

test("replacement contexts and reentrant lifetime getters cannot select a caller", async () => {
  const f = fixture();
  assert.equal(await f.owner.start(f.context), true);
  assert.throws(() => f.owner.start({ ...f.context }), /parent launch refused/u);
  assert.throws(() => f.owner.postMessage({ ...f.context }, {}), /parent launch refused/u);
  const receipt = await f.finish();
  assert.equal(receipt.successful, true);
  const g = fixture();
  Object.defineProperty(g.context, "active", {
    get() {
      void g.owner.close();
      return true;
    },
  });
  assert.throws(() => g.owner.start(g.context), /parent launch refused/u);
  await g.finish();
  assert.equal(g.lookups.length, 0);
  assert.equal(g.calls.length, 0);
});

test("phase closure and hook-time permission loss refuse before lookup", async () => {
  const f = fixture();
  f.shutdown.isClosed = true;
  assert.equal(await f.owner.start(f.context), false);
  const first = await f.finish();
  assert.equal(first.spawn_called, false);
  assert.equal(f.lookups.length, 0);
  const g = fixture();
  g.context.callOnClose = (hook) => {
    g.contextHooks.add(hook);
    g.extension.permitted = false;
  };
  assert.equal(await g.owner.start(g.context), false);
  const second = await g.finish();
  assert.equal(second.spawn_called, false);
  assert.equal(g.lookups.length, 0);
  assert.equal(g.hooks(), 0);
});

test("native barrier removal failure is observed instead of treated as cleanup delivery", async () => {
  const f = fixture();
  assert.equal(await f.owner.start(f.context), true);
  f.shutdown.removeBlocker = () => false;
  const receipt = await f.finish();
  assert.equal(receipt.successful, false);
  assert.equal(receipt.hooks_removed, false);
  assert.equal(f.blockers.size, 1);
  await assert.rejects([...f.blockers][0](), /parent launch refused/u);
});

test("opaque JSON is frozen before queueing and does not change fixed launch options", async () => {
  const f = fixture();
  assert.equal(await f.owner.start(f.context), true);
  const message = {
    protocol_version: 2,
    command: "hello",
    payload: { host: "untrusted", argv: ["other"] },
  };
  f.owner.postMessage(f.context, message);
  message.payload.host = "changed";
  await flush();
  assert.equal(f.writes.length, 1);
  const frame = f.writes[0];
  assert.equal(new DataView(frame.buffer, frame.byteOffset).getUint32(0, true), frame.length - 4);
  assert.equal(JSON.parse(new TextDecoder().decode(frame.slice(4))).payload.host, "untrusted");
  assert.deepEqual(f.calls[0].arguments, ["--browser-parent", f.info.path, ID]);
  assert.throws(
    () => f.owner.postMessage(f.context, "a".repeat(1024 * 1024)),
    /parent launch refused/u,
  );
  await f.finish();
  assert.equal(f.calls.length, 1);
});

test("own native port keepalive precedes lookup and retains late process and pipe retirement", async () => {
  const f = fixture({ holdLookup: true, holdSpawn: true, holdPipes: true });
  const started = f.owner.start(f.context);
  await flush();
  const beforeLookup = f.nativePorts.has(f.owner);
  f.lookup.resolve(f.info);
  await flush();
  const retired = f.owner.close();
  const duringStartup = f.nativePorts.has(f.owner);
  f.spawn.resolve(f.process);
  await flush();
  const duringPipes = f.nativePorts.has(f.owner);
  f.pipes.resolve();
  assert.equal(await started, false);
  const receipt = await retired;
  assert.equal(receipt.successful, true);
  assert.equal(beforeLookup, true, "owned native port was not registered before lookup");
  assert.equal(duringStartup, true);
  assert.equal(duringPipes, true);
  assert.equal(f.nativePorts.has(f.owner), false);
  assert.deepEqual(f.nativePortCalls, [
    ["track", f.owner],
    ["untrack", f.owner],
  ]);
});

test("pre-existing exact native-port registration is not adopted or reported as clean retirement", async () => {
  const f = fixture();
  f.nativePorts.add(f.owner);
  assert.equal(await f.owner.start(f.context), false);
  const receipt = await f.owner.close();
  assert.equal(f.calls.length, 0);
  assert.equal(f.lookups.length, 0);
  assert.equal(f.nativePorts.has(f.owner), true);
  assert.equal(f.nativePortCalls.length, 0);
  assert.equal(receipt.successful, false, "unknown pre-existing registration reported as clean");
  assert.equal(f.blockers.size, 1);
});

test("native-port scheduling requires exact nonpersistent background booleans", async () => {
  for (const value of [false, 1, undefined]) {
    const f = fixture();
    f.context.isBackgroundContext = value;
    assert.throws(() => f.owner.start(f.context), /parent launch refused/u);
    assert.equal((await f.finish()).successful, true);
    assert.equal(f.hooks(), 0);
    assert.equal(f.lookups.length, 0);
    assert.equal(f.nativePortCalls.length, 0);
  }
  for (const value of [true, 0, undefined]) {
    const f = fixture();
    f.extension.persistentBackground = value;
    assert.throws(() => f.owner.start(f.context), /parent launch refused/u);
    assert.equal((await f.finish()).successful, true);
    assert.equal(f.hooks(), 0);
    assert.equal(f.lookups.length, 0);
    assert.equal(f.nativePortCalls.length, 0);
  }
});

test("uncertain native-port registration or removal retains its exact shutdown guard", async () => {
  for (const kind of ["missing-add", "throw-after-add", "missing-remove", "replaced-registry"]) {
    const f = fixture();
    if (kind === "missing-add") f.context.trackNativeAppPort = () => {};
    if (kind === "throw-after-add") {
      f.context.trackNativeAppPort = (port) => {
        f.nativePorts.add(port);
        throw new Error("opaque registration failure");
      };
    }
    if (kind === "missing-remove") f.context.untrackNativeAppPort = () => Promise.resolve(true);
    const started = await f.owner.start(f.context);
    if (kind === "replaced-registry") f.context.activeNativePorts = new WeakSet();
    const receipt = await f.owner.close();
    assert.equal(receipt.successful, false, kind);
    assert.equal(receipt.hooks_removed, false, kind);
    assert.equal(f.blockers.size, 1, kind);
    assert.equal(f.nativePorts.has(f.owner), kind !== "missing-add", kind);
    assert.equal(started, kind === "missing-remove" || kind === "replaced-registry", kind);
    assert.equal(f.calls.length, started ? 1 : 0, kind);
    if (started) assert.equal(receipt.transport.successful, true, kind);
    assert.equal(f.nativePortCalls.filter(([operation]) => operation === "untrack").length, 0);
    const blocker = [...f.blockers][0];
    await assert.rejects(blocker(), /parent launch refused/u);
  }
});

test("only the exact owner is tracked in a genuine cross-realm native-port registry", async () => {
  const f = fixture();
  const ports = vm.runInNewContext("new WeakSet()");
  const other = { native: true };
  ports.add(other);
  f.context.activeNativePorts = ports;
  assert.equal(await f.owner.start(f.context), true);
  assert.equal(f.owner.native, true);
  assert.throws(() => {
    f.owner.native = false;
  }, TypeError);
  assert.equal(ports.has(f.owner), true);
  assert.equal((await f.owner.close()).successful, true);
  assert.equal(ports.has(f.owner), false);
  assert.equal(ports.has(other), true);
  assert.deepEqual(f.nativePortCalls, [
    ["track", f.owner],
    ["untrack", f.owner],
  ]);

  const fake = fixture();
  fake.context.activeNativePorts = { [Symbol.toStringTag]: "WeakSet", has: () => false };
  assert.equal(await fake.owner.start(fake.context), false);
  assert.equal((await fake.owner.close()).successful, true);
  assert.equal(fake.nativePortCalls.length, 0);
  assert.equal(fake.lookups.length, 0);
});

test("indeterminate SDK startup retains native-port scheduling without claiming native readiness", async () => {
  const f = fixture({ holdSpawn: true });
  const started = f.owner.start(f.context);
  await flush();
  f.spawn.reject(new Error("opaque post-invocation failure"));
  assert.equal(await started, false);
  const receipt = await f.owner.close();
  assert.equal(receipt.successful, false);
  assert.equal(receipt.transport.startup, "indeterminate");
  assert.equal(receipt.transport.process_waited, false);
  assert.equal(f.nativePorts.has(f.owner), true);
  assert.equal(f.blockers.size, 1);
  assert.equal(await f.owner.start(f.context), false);
  assert.equal(f.calls.length, 1);
});
