import assert from "node:assert/strict";
import test from "node:test";
import { HandoffContexts } from "../extension/protection-bridge/handoff-context.js";
import { ParentConnection } from "../extension/protection-bridge/parent-connection.js";

const source = "http://127.0.0.1/attachment";
const uuid = (n) => `00000000-0000-4000-8000-${n.toString(16).padStart(12, "0")}`;
const error = /Download protection handoff context refused/u;
function fixture() {
  const hooks = [];
  const extension = {
    policy: {},
    baseURI: { resolve: (name) => `moz-extension://fixture/${name}` },
  };
  const context = {
    extension,
    active: true,
    unloaded: false,
    envType: "addon_parent",
    viewType: "background",
    isTopContext: true,
    incognito: false,
    uri: { spec: extension.baseURI.resolve("_generated_background_page.html") },
    xulBrowser: { frameLoader: { remoteTab: {} } },
    callOnClose: (hook) => hooks.push(hook),
  };
  const attrs = { userContextId: 0, privateBrowsingId: 0 };
  const uri = (spec) => Object.freeze({ spec });
  const principal = Object.freeze({
    isContentPrincipal: true,
    URI: uri(source),
    originAttributes: Object.freeze(attrs),
  });
  const browser = {};
  const channel = {
    URI: uri(source),
    loadInfo: { originAttributes: attrs, redirectChain: [{ principal }] },
  };
  const state = { ready: true, lookups: 0 };
  const platform = {
    lookup: (id, policy, remote) => {
      state.lookups++;
      assert.equal(policy, extension.policy);
      assert.equal(remote, context.xulBrowser.frameLoader.remoteTab);
      return {
        id,
        channel,
        browserElement: browser,
        method: "GET",
        type: "main_frame",
        frameId: 0,
        parentFrameId: -1,
        statusCode: 200,
        canModify: true,
        errorString: "",
        finalURL: source,
        matches: () => true,
      };
    },
    browserData: (value) => {
      assert.equal(value, browser);
      return { tabId: 4 };
    },
    cookieStore: () => "firefox-default",
    uri,
    httpReferrer: () => null,
    historyPrincipal: (entry) => entry.principal,
  };
  const connection = {
    transportReady: (caller) => {
      assert.equal(caller, context);
      return state.ready;
    },
  };
  const registry = new HandoffContexts(extension, platform, connection);
  const bind = (n = 1, requestId = String(n), url = source) =>
    registry.bind(context, uuid(n), requestId, 4, url);
  const snapshot = (binding, n = 1, url = source) =>
    registry.snapshot(context, binding, uuid(n), url);
  return {
    registry,
    context,
    extension,
    platform,
    connection,
    state,
    channel,
    hooks,
    bind,
    snapshot,
    principal,
  };
}

test("actual registered reader binds task/request/tab/source and returns metadata, not a decision", () => {
  const f = fixture();
  const binding = f.bind();
  const view = f.snapshot(binding);
  assert.equal(view.epoch, 1);
  assert.equal(view.metadata.requestId, "1");
  assert.equal(view.metadata.tabId, 4);
  assert.equal(view.metadata.sourceURI.spec, source);
  assert.equal(view.metadata.redirects[0], f.principal);
  assert.equal(view.metadata.referrerInfo, null);
  assert.deepEqual(Object.keys(view).sort(), ["epoch", "metadata", "toJSON"]);
  for (const value of [binding, view, view.metadata, f.registry]) {
    assert.throws(() => JSON.stringify(value), /not serializable/u);
  }
  assert(Object.isFrozen(binding));
  assert(Object.isFrozen(view));
  assert(Object.isFrozen(view.metadata));
  f.registry.close();
  assert.throws(() => f.snapshot(binding), error);
  assert.equal(view.metadata.sourceURI.spec, source); // A previous copy is not revocable authority.
});

test("foreign owner/token and changed native task/source cannot read a binding", () => {
  const f = fixture(),
    other = fixture();
  const binding = f.bind(),
    foreign = other.bind();
  assert.throws(() => f.snapshot(foreign), error);
  assert.throws(() => f.registry.snapshot({ ...f.context }, binding, uuid(1), source), error);
  assert.throws(() => f.snapshot(binding, 2), error);
  for (const url of [
    source + "?different",
    source + "#fragment",
    source.replace("127.0.0.1", "localhost"),
  ]) {
    assert.throws(() => f.snapshot(binding, 1, url), error);
  }
  assert.equal(f.snapshot(binding).epoch, 1);
  assert.equal(f.state.lookups, 1);
});

test("handoff and request IDs cannot be rebound, including after release", () => {
  const f = fixture();
  const binding = f.bind();
  for (const release of [false, true]) {
    if (release) f.registry.release(binding);
    assert.throws(() => f.bind(1, "2"), error);
    assert.throws(() => f.bind(2, "1"), error);
  }
  assert.equal(f.state.lookups, 1);
  const next = f.bind(2, "2");
  f.registry.release(binding); // Old inverse cannot detach the new record.
  assert.equal(f.snapshot(next, 2).epoch, 2);
  assert.throws(() => f.snapshot(binding), error);
});

test("failed capture consumes identity and releases its metadata slot", () => {
  const f = fixture();
  assert.throws(() => f.bind(1, "1", source + "?wrong"), error);
  assert.throws(() => f.bind(), error);
  assert.throws(() => f.bind(2, "1"), error);
  assert.doesNotThrow(() => {
    for (let n = 2; n <= 33; n++) f.bind(n);
  }, "failed capture must release the original metadata slot");
  assert.equal(f.state.lookups, 33); // Failed snapshot did not consume a live slot.
});

test("bounded live slots and lifetime tombstones never authorize recycling", () => {
  const f = fixture();
  const bindings = [];
  for (let n = 1; n <= 32; n++) bindings.push(f.bind(n));
  assert.throws(() => f.bind(33), error);
  f.registry.release(bindings[0]);
  assert.doesNotThrow(() => {
    assert.equal(f.snapshot(f.bind(33), 33).epoch, 33);
  }, "capacity refusal must happen before consuming the pending identity");
  for (const binding of bindings.slice(1)) f.registry.release(binding);
  // Separate registry: exercise the actual bounded consumed-history limit.
  const g = fixture();
  for (let n = 1; n <= 10000; n++) {
    const binding = g.bind(n);
    g.registry.release(binding);
  }
  assert.equal(g.state.lookups, 10000);
  assert.throws(() => g.bind(10001), error);
  assert.equal(g.state.lookups, 10000);
});

test("readiness and lifetime flags require exact booleans; lost readiness is sticky", () => {
  for (const value of [false, 1, {}, Promise.resolve(true)]) {
    const f = fixture();
    f.state.ready = value;
    assert.throws(() => f.bind(), error);
    f.state.ready = true;
    assert.throws(() => f.bind(2), error);
    assert.equal(f.state.lookups, 0);
  }
  for (const [key, value] of [
    ["active", 1],
    ["active", false],
    ["unloaded", 0],
    ["unloaded", true],
  ]) {
    const f = fixture();
    f.context[key] = value;
    assert.throws(() => f.bind(), error);
    assert.equal(f.state.lookups, 0);
  }
  const f = fixture();
  const binding = f.bind();
  f.state.ready = false;
  assert.throws(() => f.snapshot(binding), error);
  f.state.ready = true;
  assert.throws(() => f.snapshot(binding), error);
  assert.throws(() => f.bind(2), error);
});

test("context close, uncertain registration and immediate close prevent any later lookup", () => {
  const f = fixture();
  const binding = f.bind();
  for (const hook of f.hooks) hook.close();
  assert.throws(() => f.snapshot(binding), error);
  assert.throws(() => f.bind(2), error);
  assert.equal(f.state.lookups, 1);
  for (const fail of [false, true]) {
    const g = fixture();
    g.context.callOnClose = (hook) => {
      hook.close();
      if (fail) throw new Error("private browser detail");
    };
    assert.throws(() => g.bind(), error);
    assert.throws(() => g.bind(2), error);
    assert.equal(g.state.lookups, 0);
  }
  const g = fixture();
  g.context.callOnClose = () => {
    throw new Error("unknown hook delivery");
  };
  assert.throws(() => g.bind(), error);
  assert.throws(() => g.bind(2), error);
  assert.equal(g.state.lookups, 0);
});

test("retirement during metadata copying cannot create a usable binding", () => {
  for (const close of [true, false]) {
    const f = fixture();
    const original = f.platform.uri;
    f.platform.uri = (value) => {
      if (close) f.registry.close();
      else f.state.ready = false;
      return original(value);
    };
    assert.throws(() => f.bind(), error);
    f.state.ready = true;
    assert.throws(() => f.bind(2), error);
  }
});

test("release during the final live check cannot return the retired snapshot", () => {
  const f = fixture();
  const binding = f.bind();
  let checks = 0;
  f.connection.transportReady = () => {
    if (++checks === 2) f.registry.release(binding);
    return true;
  };
  assert.throws(() => f.snapshot(binding), error);
  assert.equal(checks, 2);
});

test("reentrant capture refuses without consuming a second identity", () => {
  const f = fixture();
  const original = f.platform.uri;
  f.platform.uri = (value) => {
    assert.throws(() => f.bind(2), error);
    return original(value);
  };
  assert.equal(f.snapshot(f.bind()).epoch, 1);
  f.platform.uri = original;
  assert.doesNotThrow(() => {
    assert.equal(f.snapshot(f.bind(2), 2).epoch, 2);
  }, "reentrant refusal must not consume the next identity");
});

test("actual broker admission and disconnect delimit the same context registry", async () => {
  const f = fixture();
  let sink, registry;
  const writes = [];
  let closed = false;
  class Launcher {
    constructor(extension, platform, callbacks) {
      assert.equal(extension, f.extension);
      sink = callbacks;
    }
    assertCaller(context) {
      assert.equal(context, f.context);
      assert.equal(closed, false);
    }
    start(context) {
      this.assertCaller(context);
      return Promise.resolve(true);
    }
    postMessage(context, value) {
      this.assertCaller(context);
      writes.push(value);
    }
    close() {
      closed = true;
      sink.onDisconnect();
      return Promise.resolve({ successful: true });
    }
  }
  const broker = new ParentConnection(
    f.extension,
    {},
    {
      onMessage: () => assert.fail("no ordinary traffic expected"),
      onDisconnect: () => registry?.close(),
    },
    Launcher,
  );
  registry = new HandoffContexts(f.extension, f.platform, broker);
  const connecting = broker.connect(f.context);
  await new Promise((resolve) => setImmediate(resolve));
  const offer = {
    parent_transport: 1,
    admission_id: uuid(99),
    capture_ready: false,
    kind: "offer",
  };
  sink.onMessage(offer);
  assert.equal(broker.transportReady(f.context), false);
  assert.deepEqual(writes, [{ parent_transport: 1, admission_id: uuid(99), kind: "accept" }]);
  sink.onMessage({ ...offer, kind: "ready" });
  assert.equal(await connecting, true);
  const binding = registry.bind(f.context, uuid(1), "1", 4, source);
  assert.equal(registry.snapshot(f.context, binding, uuid(1), source).epoch, 1);
  const retirement = broker.close();
  assert.throws(() => registry.snapshot(f.context, binding, uuid(1), source), error);
  const result = await retirement;
  assert.equal(result.successful, true);
  assert.equal(result.capture_ready, false);
  assert.throws(() => registry.bind(f.context, uuid(2), "2", 4, source), error);
  assert.equal(f.state.lookups, 1);
  assert.equal(writes.length, 1); // Association emits no Add, cancellation or verdict.
});

test("bounded typed identities and metadata errors refuse without sensitive text", () => {
  for (const args of [
    [uuid(1).toUpperCase().replace("4000", "5000"), "1", 4, source],
    [uuid(1), "01", 4, source],
    [uuid(1), "9007199254740992", 4, source],
    [uuid(1), "1", true, source],
    [uuid(1), "1", -1, source],
    [uuid(1), "1", 4, "x".repeat(16385)],
  ]) {
    const f = fixture();
    assert.throws(() => f.registry.bind(f.context, ...args), error);
    assert.equal(f.state.lookups, 0);
  }
  const f = fixture();
  Object.defineProperty(f.channel, "URI", {
    get: () => {
      throw new Error("sensitive source and path");
    },
  });
  assert.throws(() => f.bind(), { message: "Download protection handoff context refused" });
  assert.throws(() => f.bind(), error);
  assert.equal(f.state.lookups, 1);
});
