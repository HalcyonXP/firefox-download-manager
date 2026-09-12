import assert from "node:assert/strict";
import test from "node:test";
import { ParentApi } from "../extension/protection-bridge/parent-api.js";

const flush = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
function fixture(onRetired) {
  const extension = {};
  const context = { active: true };
  const records = [];
  class Connection {
    constructor(ext, platform, callbacks) {
      assert.equal(ext, extension);
      this.record = {
        callbacks,
        start: deferred(),
        close: deferred(),
        starts: 0,
        closes: 0,
        sent: [],
      };
      records.push(this.record);
    }
    assertCaller(ctx) {
      if (ctx !== context || !ctx.active) throw new Error("synthetic guard");
    }
    connect(ctx) {
      this.assertCaller(ctx);
      this.record.starts++;
      return this.record.start.promise;
    }
    transportReady(ctx) {
      this.assertCaller(ctx);
      return true;
    }
    postMessage(ctx, value) {
      this.assertCaller(ctx);
      this.record.sent.push(value);
    }
    close() {
      this.record.closes++;
      this.record.callbacks.onDisconnect();
      return this.record.close.promise;
    }
  }
  return { owner: new ParentApi(extension, {}, Connection, onRetired), context, records };
}

test("foreign and inactive callers cannot reserve the singleton or start a process", async () => {
  const f = fixture();
  assert.throws(() => f.owner.open({ active: true }));
  f.context.active = false;
  assert.throws(() => f.owner.open(f.context));
  assert.ok(f.records.every((r) => r.starts === 0 && r.closes === 0));
  f.context.active = true;
  const id = f.owner.open(f.context);
  assert.ok(Number.isSafeInteger(id) && id > 0);
  const r = f.records.at(-1);
  assert.equal(r.starts, 1);
  assert.throws(() => f.owner.open(f.context));
  r.start.resolve(true);
  assert.equal(await f.owner.ready(f.context, id), true);
  const close = f.owner.close(f.context, id);
  r.close.resolve({ successful: true });
  assert.equal(await close, true);
});

test("one bounded read, queue order and revocation between resolution and delivery", async () => {
  const f = fixture();
  const id = f.owner.open(f.context);
  const r = f.records[0];
  r.start.resolve(true);
  const reading = f.owner.read(f.context, id);
  await assert.rejects(f.owner.read(f.context, id));
  r.callbacks.onMessage({ sequence: 1 });
  assert.deepEqual(await reading, { sequence: 1 });
  r.callbacks.onMessage({ sequence: 2 });
  assert.deepEqual(await f.owner.read(f.context, id), { sequence: 2 });
  const revoked = f.owner.read(f.context, id);
  r.callbacks.onMessage({ sequence: 3 });
  const closing = f.owner.close(f.context, id);
  assert.equal(await revoked, null);
  r.close.resolve({ successful: true });
  assert.equal(await closing, true);
});

test("overflow revokes without leaking or replacing the retained owner", async () => {
  const f = fixture();
  const id = f.owner.open(f.context);
  const r = f.records[0];
  r.start.resolve(true);
  for (let i = 0; i < 32; i++) r.callbacks.onMessage({ sequence: i });
  assert.throws(() => r.callbacks.onMessage({ sequence: 32 }));
  assert.throws(() => f.owner.open(f.context));
  await assert.rejects(f.owner.read(f.context, id));
  await flush();
  assert.equal(r.closes, 1);
  r.close.resolve({ successful: false });
  assert.equal(await f.owner.shutdown(), false);
});

test("native close and startup both settle before a fresh API-local token is issued", async () => {
  const f = fixture();
  const old = f.owner.open(f.context);
  const a = f.records[0];
  const pending = f.owner.read(f.context, old);
  const closing = f.owner.close(f.context, old);
  assert.equal(f.owner.close(f.context, old), closing);
  assert.equal(await pending, null);
  a.close.resolve({ successful: true });
  await flush();
  assert.throws(() => f.owner.open(f.context));
  a.start.resolve(false); // Intentional close before native readiness.
  assert.equal(await closing, true);
  const current = f.owner.open(f.context);
  const b = f.records[1];
  assert.notEqual(old, current);
  assert.throws(() => f.owner.close(f.context, old));
  assert.throws(() => f.owner.ready(f.context, old));
  await assert.rejects(f.owner.read(f.context, old));
  assert.throws(() => f.owner.postMessage(f.context, old, {}));
  assert.equal(b.closes, 0, "stale token must not retire replacement");
  b.start.resolve(true);
  assert.equal(await f.owner.ready(f.context, current), true);
  b.close.resolve({ successful: true });
  assert.equal(await f.owner.close(f.context, current), true);
});

test("caller revocation invalidates a retained successful ready and joins shutdown once", async () => {
  const f = fixture();
  const id = f.owner.open(f.context);
  const r = f.records[0];
  r.start.resolve(true);
  assert.equal(await f.owner.ready(f.context, id), true);
  f.context.active = false;
  assert.throws(() => f.owner.ready(f.context, id));
  await flush();
  assert.equal(r.closes, 1);
  r.close.resolve({ successful: true });
  assert.equal(await f.owner.shutdown(), true);
  assert.throws(() => f.owner.open(f.context));
  assert.throws(() => JSON.stringify(f.owner));
});

test("failed or rejected retirement cannot authorize a replacement", async () => {
  for (const rejected of [false, true]) {
    const f = fixture();
    const id = f.owner.open(f.context);
    const r = f.records[0];
    r.start.resolve(true);
    const closing = f.owner.close(f.context, id);
    if (rejected) r.close.reject(new Error("synthetic close refusal"));
    else r.close.resolve({ successful: false });
    if (rejected) await assert.rejects(closing);
    else assert.equal(await closing, false);
    assert.throws(() => f.owner.open(f.context));
    assert.equal(r.closes, 1);
  }
});

test("retirement notification follows both retained startup and native closure", async () => {
  const seen = [];
  const f = fixture((value) => seen.push(value));
  const id = f.owner.open(f.context);
  const r = f.records[0];
  const closing = f.owner.close(f.context, id);
  await flush();
  assert.deepEqual(seen, []);
  r.close.resolve({ successful: true });
  await flush();
  assert.deepEqual(seen, []);
  r.start.resolve(true);
  assert.equal(await closing, true);
  assert.equal(seen.length, 1);
  assert.deepEqual(Object.keys(seen[0]).sort(), ["connection", "receipt", "version"]);
  assert.equal(seen[0].connection, id);
  assert.equal(Object.isFrozen(seen[0]), true);
  await f.owner.shutdown();
  assert.equal(seen.length, 1);
  assert.equal(r.closes, 1);
});
test("uncertain notification retains failed retirement without replay or replacement", async () => {
  let notifications = 0;
  const f = fixture(() => {
    notifications++;
    throw Error("notification refused");
  });
  const id = f.owner.open(f.context);
  const r = f.records[0];
  r.start.resolve(true);
  const closing = f.owner.close(f.context, id);
  r.close.resolve({ successful: true });
  await assert.rejects(closing);
  assert.throws(() => f.owner.open(f.context));
  await assert.rejects(f.owner.shutdown());
  assert.equal(notifications, 1);
  assert.equal(r.closes, 1);
});
