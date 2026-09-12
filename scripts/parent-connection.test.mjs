import assert from "node:assert/strict";
import test from "node:test";
import { buildSync } from "esbuild";
import { ParentConnection } from "../extension/protection-bridge/parent-connection.js";

const ID = "11111111-1111-4111-8111-111111111111";
const offer = { parent_transport: 1, admission_id: ID, kind: "offer", capture_ready: false };
const ready = { ...offer, kind: "ready" };
const hello = {
  protocol_version: 2,
  kind: "command",
  command: "hello",
  correlation_id: "hello",
  payload: { supported_versions: [2], client_name: "owned", client_version: "0.1.0" },
};
const add = {
  ...hello,
  command: "add",
  correlation_id: "add",
  payload: { url: "http://127.0.0.1:1/never-requested" },
};
const response = {
  protocol_version: 2,
  kind: "response",
  command: "hello",
  correlation_id: "hello",
  ok: true,
  result: {},
};
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

function fixture({
  holdStart = false,
  holdClose = false,
  onMessage = () => {},
  onDisconnect = () => {},
} = {}) {
  const extension = {};
  const context = { active: true };
  const startup = deferred();
  const retirement = deferred();
  if (!holdStart) startup.resolve(true);
  if (!holdClose) retirement.resolve({ successful: true });
  let callbacks;
  let sendEffect = () => {};
  let closed = false;
  let closing;
  let starts = 0;
  let closes = 0;
  let disconnected = 0;
  const writes = [];
  const delivered = [];
  class Launcher {
    constructor(ext, platform, sink) {
      assert.equal(ext, extension);
      callbacks = sink;
    }
    assertCaller(ctx) {
      if (ctx !== context || !context.active || closed) throw Error("guard");
    }
    start(ctx) {
      this.assertCaller(ctx);
      starts++;
      return startup.promise;
    }
    postMessage(ctx, value) {
      this.assertCaller(ctx);
      writes.push(structuredClone(value));
      sendEffect(value);
    }
    close() {
      if (closing) return closing;
      closed = true;
      closes++;
      closing = retirement.promise;
      // Synchronous SDK hook; returning broker.close() here would self-cycle.
      callbacks.onDisconnect();
      return closing;
    }
  }
  const owner = new ParentConnection(
    extension,
    {},
    {
      onMessage: (message) => {
        delivered.push(message);
        return onMessage(message);
      },
      onDisconnect: () => {
        disconnected++;
        return onDisconnect();
      },
    },
    Launcher,
  );
  return {
    owner,
    context,
    startup,
    retirement,
    writes,
    delivered,
    message: (value) => callbacks.onMessage(value),
    effect: (callback) => {
      sendEffect = callback;
    },
    counts: () => ({ starts, closes, disconnected }),
  };
}

async function admitted(f) {
  const connecting = f.owner.connect(f.context);
  f.owner.postMessage(f.context, hello);
  await flush();
  f.message(offer);
  f.message(ready);
  assert.equal(await connecting, true);
}

test("native acceptance precedes one queued Hello and ordinary wire delivery", async () => {
  const f = fixture();
  const connecting = f.owner.connect(f.context);
  f.owner.postMessage(f.context, hello);
  assert.throws(() => f.owner.postMessage(f.context, add));
  assert.throws(() => f.owner.postMessage(f.context, hello));
  await flush();
  assert.deepEqual(f.writes, []);
  assert.equal(f.owner.transportReady(f.context), false);
  f.message(offer);
  assert.deepEqual(f.writes, [{ parent_transport: 1, admission_id: ID, kind: "accept" }]);
  assert.equal(f.owner.transportReady(f.context), false);
  f.message(ready);
  assert.equal(await connecting, true);
  assert.deepEqual(f.writes.slice(1), [hello]);
  assert.deepEqual(f.delivered, []);
  f.message(response);
  assert.deepEqual(f.delivered, [response]);
  f.owner.postMessage(f.context, add);
  assert.deepEqual(f.writes.at(-1), add);
  const receipt = await f.owner.close();
  assert.equal(receipt.successful, true);
  assert.equal(receipt.capture_ready, false);
  assert.equal(receipt.transport_admission_observed, true);
  assert.throws(() => f.owner.transportReady(f.context));
  assert.throws(() => f.owner.connect(f.context));
  assert.deepEqual(f.counts(), { starts: 1, closes: 1, disconnected: 1 });
});

test("closed private frame types, IDs, order and capture refusal remain sticky", async () => {
  for (const messages of [
    [ready],
    [response],
    [offer, offer],
    [{ ...offer, parent_transport: true }],
    [{ ...offer, parent_transport: 2 }],
    [{ ...offer, capture_ready: true }],
    [{ ...offer, admission_id: ID.toUpperCase().replace("4111", "A111") }],
    [{ ...offer, extra: null }],
    [offer, { ...ready, admission_id: "22222222-2222-4222-8222-222222222222" }],
    [offer, { ...ready, capture_ready: true }],
  ]) {
    const f = fixture();
    const connecting = f.owner.connect(f.context);
    await flush();
    for (const message of messages) f.message(message);
    assert.throws(() => f.owner.transportReady(f.context));
    f.message(offer);
    f.message(ready); // Later valid observations cannot repair failure.
    assert.equal(await connecting, false);
    assert.equal((await f.owner.close()).successful, false);
    assert.deepEqual(f.delivered, []);
  }
});

test("private messages and unprotected captured handoffs cannot enter public send", async () => {
  const f = fixture();
  await admitted(f);
  const before = f.writes.length;
  for (const value of [
    offer,
    { ...hello, parent_transport: 1 },
    { ...hello, command: "prepare_handoff" },
    { ...hello, command: "commit_handoff" },
    { ...hello, command: "get_handoff" },
    { ...hello, command: "abort_handoff" },
    { ...hello, command: "policy_decision" },
    { ...add, protocol_version: true },
    { ...add, correlation_id: "" },
    { ...add, payload: null },
    { ...add, payload: [] },
    { ...add, payload: { padding: "π".repeat(524288) } },
  ])
    assert.throws(() => f.owner.postMessage(f.context, value));
  assert.equal(f.writes.length, before);
  assert.equal((await f.owner.close()).successful, true);
});

test("private frames and mixed ordinary envelopes never escape after admission", async () => {
  for (const value of [
    offer,
    ready,
    { ...response, parent_transport: 1 },
    { ...response, ok: 1 },
  ]) {
    const f = fixture();
    await admitted(f);
    f.message(value);
    assert.equal((await f.owner.close()).successful, false);
    assert.deepEqual(f.delivered, []);
  }
});

test("live context is rechecked on retention, delivery and after serialization", async () => {
  const f = fixture();
  const connecting = f.owner.connect(f.context);
  await flush();
  assert.throws(() => f.owner.postMessage({}, hello));
  const value = {
    toJSON() {
      f.context.active = false;
      return hello;
    },
  };
  assert.throws(() => f.owner.postMessage(f.context, value));
  assert.deepEqual(f.writes, []);
  f.message(offer);
  assert.equal(await connecting, false);
  assert.equal((await f.owner.close()).successful, false);
});

test("reentrant native readiness cannot authorize a failed acceptance write", async () => {
  const f = fixture();
  const connecting = f.owner.connect(f.context);
  await flush();
  f.effect(() => {
    f.message(ready);
    throw Error("uncertain queue");
  });
  f.message(offer);
  assert.equal(await connecting, false);
  assert.equal((await f.owner.close()).successful, false);
  assert.deepEqual(f.delivered, []);
});

test("uncertain Hello flush never resolves successful connection", async () => {
  const f = fixture();
  const connecting = f.owner.connect(f.context);
  f.owner.postMessage(f.context, hello);
  await flush();
  f.message(offer);
  f.effect(() => {
    throw Error("uncertain queue");
  });
  f.message(ready);
  assert.equal(await connecting, false);
  assert.equal((await f.owner.close()).successful, false);
  assert.equal(f.writes.filter((value) => value.command === "hello").length, 1);
});

test("uncertain ordinary send revokes admission without replay", async () => {
  const f = fixture();
  await admitted(f);
  f.effect(() => {
    throw Error("uncertain queue");
  });
  assert.throws(() => f.owner.postMessage(f.context, add));
  assert.throws(() => f.owner.transportReady(f.context));
  assert.equal((await f.owner.close()).successful, false);
  assert.equal(f.writes.filter((value) => value.command === "add").length, 1);
});

test("close retains startup and launcher retirement instead of awaiting its own callback", async () => {
  const f = fixture({ holdStart: true, holdClose: true });
  const connecting = f.owner.connect(f.context);
  f.owner.postMessage(f.context, hello);
  await flush();
  const closing = f.owner.close();
  assert.equal(f.owner.close(), closing);
  let settled = false;
  void closing.then(() => {
    settled = true;
  });
  f.retirement.resolve({ successful: true });
  await flush();
  assert.equal(settled, false);
  f.startup.resolve(true);
  assert.equal(await connecting, false);
  await closing;
  assert.equal(settled, true);
  assert.deepEqual(f.writes, []);
  assert.equal(f.counts().closes, 1);
});

test("startup and retirement failures never acquire a replacement owner", async () => {
  const f = fixture({ holdStart: true, holdClose: true });
  const connecting = f.owner.connect(f.context);
  await flush();
  f.startup.reject(Error("unknown start"));
  assert.equal(await connecting, false);
  f.retirement.reject(Error("retirement refused"));
  await assert.rejects(f.owner.close(), /retirement refused/u);
  assert.throws(() => f.owner.connect(f.context));
  assert.equal(f.counts().starts, 1);
  assert.throws(() => JSON.stringify(f.owner), /not serializable/u);
});

test("actual NativeConnection consumes only wire2 and cannot acquire capture capability", async () => {
  const bundle = buildSync({
    entryPoints: ["extension/src/native-connection.ts"],
    bundle: true,
    write: false,
    format: "esm",
    platform: "node",
  });
  const { NativeConnection } = await import(
    `data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].text).toString("base64")}`
  );
  const messages = new Set();
  const disconnects = new Set();
  const f = fixture({
    onMessage: (message) => {
      for (const listener of messages) listener(message);
    },
    onDisconnect: () => {
      for (const listener of disconnects) listener();
    },
  });
  const port = {
    onMessage: { addListener: (listener) => messages.add(listener) },
    onDisconnect: { addListener: (listener) => disconnects.add(listener) },
    postMessage: (message) => f.owner.postMessage(f.context, message),
    disconnect: () => {
      void f.owner.close();
    },
  };
  const client = new NativeConnection(() => port, "0.1.0");
  const connecting = f.owner.connect(f.context);
  const state = client.connect();
  try {
    await flush();
    f.message(offer);
    f.message(ready);
    assert.equal(await connecting, true);
    assert.equal(client.state().connected, false);
    const sent = f.writes.find((message) => message.command === "hello");
    f.message({
      ...response,
      correlation_id: sent.correlation_id,
      result: {
        selected_version: 2,
        helper_version: "0.1.0",
        max_message_bytes: 1048576,
        capabilities: [
          "snapshots",
          "coalesced_progress",
          "authenticated_requests",
          "sha256",
          "task_handoff_phase",
        ],
      },
    });
    f.message({
      protocol_version: 2,
      correlation_id: "event-0",
      kind: "event",
      event: "snapshot",
      sequence: 0,
      emitted_at: "2026-09-05T00:00:00.000Z",
      data: {
        snapshot_id: "snapshot-0",
        page_index: 0,
        tasks: [],
        next_cursor: null,
        complete: true,
      },
    });
    assert.equal((await state).connected, true);
    assert.equal(client.supports("prepared_handoff"), false);
    assert.equal(client.supports("task_handoff_phase"), true);
  } finally {
    await f.owner.close();
    await state.catch(() => {});
  }
  assert.equal(client.state().connected, false);
  assert.equal(f.delivered.length, 2);
});
