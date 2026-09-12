import assert from "node:assert/strict";
import test from "node:test";
import vm from "node:vm";
import { RetainedNativeTransport } from "../extension/protection-bridge/native-transport.js";

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

function fixture({
  holdClose = false,
  autoExit = true,
  holdWrites = false,
  spawnFailure = false,
  onMessage,
  onDisconnect,
} = {}) {
  const exit = deferred();
  const closeGate = deferred();
  if (!holdClose) closeGate.resolve();
  const reads = { stdout: [], stderr: [] };
  const writes = [];
  const messages = [];
  const closes = [];
  const timers = [];
  let starts = 0;
  let waits = 0;
  let kills = 0;
  let disconnects = 0;
  const pipe = (name) => {
    let closed = false;
    return {
      read(length) {
        if (closed) return Promise.reject(new Error("private raw EOF"));
        const read = deferred();
        reads[name].push({ length, ...read });
        return read.promise;
      },
      write(bytes) {
        assert.equal(name, "stdin");
        const pending = deferred();
        // Model the SDK's ownership transfer/detachment, not just a copied view.
        const buffer = structuredClone(bytes.buffer, { transfer: [bytes.buffer] });
        const count = buffer.byteLength;
        writes.push({ buffer, ...pending });
        if (!holdWrites) pending.resolve({ bytesWritten: count });
        return pending.promise;
      },
      close(force) {
        assert.equal(force, true);
        closes.push(name);
        closed = true;
        for (const read of reads[name] ?? []) read.reject(new Error("private raw EOF"));
        if (name === "stdin") {
          for (const write of writes) write.reject(new Error("private raw write cancellation"));
          if (autoExit) exit.resolve({ exitCode: 0 });
        }
        return closeGate.promise;
      },
    };
  };
  const process = {
    stdin: pipe("stdin"),
    stdout: pipe("stdout"),
    stderr: pipe("stderr"),
    wait() {
      waits++;
      return exit.promise;
    },
    kill(force) {
      assert.equal(force, 0);
      kills++;
      exit.resolve({ exitCode: -9 });
      return exit.promise;
    },
  };
  const spawn = deferred();
  const owner = new RetainedNativeTransport({
    spawn: () => {
      starts++;
      return spawn.promise;
    },
    timer: (milliseconds, callback) => {
      assert.equal(milliseconds, 3000);
      const timer = { callback, cancelled: false };
      timers.push(timer);
      return () => {
        timer.cancelled = true;
      };
    },
    onMessage: (message) => {
      messages.push(message);
      return onMessage?.(message);
    },
    onDisconnect: () => {
      disconnects++;
      return onDisconnect?.();
    },
  });
  async function start() {
    const ready = owner.start();
    if (spawnFailure) spawn.reject(new Error("private raw post-spawn failure"));
    else spawn.resolve(process);
    return ready;
  }
  async function finish() {
    spawn.resolve(process);
    const retiring = owner.close();
    exit.resolve({ exitCode: 0 });
    closeGate.resolve();
    return retiring;
  }
  async function deliver(message, crossRealm = false) {
    const bytes = new TextEncoder().encode(JSON.stringify(message));
    const header = new ArrayBuffer(4);
    new DataView(header).setUint32(0, bytes.length, true);
    const convert = (buffer) =>
      crossRealm
        ? vm.runInNewContext("Uint8Array.from(bytes).buffer", {
            bytes: [...new Uint8Array(buffer)],
          })
        : buffer;
    const first = reads.stdout.shift();
    assert.equal(first.length, 4);
    first.resolve(convert(header));
    await flush();
    const body = reads.stdout.shift();
    if (body) {
      assert.equal(body.length, bytes.length);
      body.resolve(convert(bytes.buffer));
    }
    await flush();
  }
  return {
    owner,
    process,
    spawn,
    exit,
    closeGate,
    reads,
    writes,
    messages,
    closes,
    timers,
    start,
    finish,
    deliver,
    counts: () => ({ starts, waits, kills, disconnects }),
  };
}

test("one retained process serializes frames and waits for actual pipe closure after exit", async () => {
  const f = fixture({ holdClose: true });
  let early;
  let result;
  try {
    assert.equal(await f.start(), true);
    assert.equal(await f.owner.start(), true);
    f.owner.postMessage({ fixed: "first" });
    f.owner.postMessage({ fixed: "second" });
    await flush();
    await f.deliver({ native: "receipt" });
    const retiring = f.owner.close();
    let settled = false;
    void retiring.then(() => {
      settled = true;
    });
    await flush();
    early = settled;
    f.closeGate.resolve();
    result = await retiring;
  } finally {
    await f.finish();
  }
  assert.equal(early, false, "exit alone must not retire still-owned pipe operations");
  assert.deepEqual(result, {
    startup: "started",
    process_waited: true,
    exit_code: 0,
    pipes_closed: true,
    io_settled: true,
    forced: false,
    successful: true,
  });
  assert.equal(Object.isFrozen(result), true);
  assert.deepEqual(f.counts(), { starts: 1, waits: 1, kills: 0, disconnects: 1 });
  assert.deepEqual(f.messages, [{ native: "receipt" }]);
  assert.deepEqual(f.closes.sort(), ["stderr", "stdin", "stdout"]);
  const decoded = f.writes.map(({ buffer }) => {
    const view = new DataView(buffer);
    assert.equal(view.getUint32(0, true), buffer.byteLength - 4);
    return JSON.parse(new TextDecoder().decode(new Uint8Array(buffer, 4)));
  });
  assert.deepEqual(decoded, [{ fixed: "first" }, { fixed: "second" }]);
  assert.equal(f.timers[0].cancelled, true);
});

test("SDK buffers from another realm are accepted without weakening length validation", async () => {
  const f = fixture();
  try {
    await f.start();
    await f.deliver({ native: "cross-realm" }, true);
  } finally {
    await f.finish();
  }
  assert.deepEqual(f.messages, [{ native: "cross-realm" }]);
});

test("close during pending startup retains and retires the eventual exact process", async () => {
  const f = fixture();
  const ready = f.owner.start();
  await flush();
  const retiring = f.owner.close();
  let settled = false;
  void retiring.then(() => {
    settled = true;
  });
  await flush();
  const early = settled;
  f.spawn.resolve(f.process);
  const started = await ready;
  const result = await retiring;
  await f.finish();
  assert.equal(early, false);
  assert.equal(started, false);
  assert.equal(result.process_waited, true);
  assert.equal(result.successful, true);
  assert.deepEqual(f.counts(), { starts: 1, waits: 1, kills: 0, disconnects: 1 });
  assert.equal(f.messages.length, 0);
});

test("pre-start close launches nothing; post-spawn rejection is indeterminate, not no-child success", async () => {
  const first = fixture();
  const stopped = await first.owner.close();
  assert.equal(await first.owner.start(), false);
  assert.equal(stopped.startup, "not-started");
  assert.equal(first.counts().starts, 0);
  const second = fixture({ spawnFailure: true });
  assert.equal(await second.start(), false);
  const result = await second.owner.close();
  assert.equal(await second.owner.start(), false);
  assert.deepEqual(result, {
    startup: "indeterminate",
    process_waited: false,
    exit_code: null,
    pipes_closed: false,
    io_settled: true,
    forced: false,
    successful: false,
  });
  assert.equal(second.counts().starts, 1);
  assert.throws(() => second.owner.postMessage({ ignored: true }), /transport refused/u);
  assert.throws(() => JSON.stringify(second.owner), /not serializable/u);
});

test("all 32 slots include an in-flight write; close drops pending frames without replay", async () => {
  const f = fixture({ holdWrites: true });
  try {
    await f.start();
    for (let index = 0; index < 32; index++) f.owner.postMessage({ index });
    assert.throws(() => f.owner.postMessage({ extra: true }), /transport refused/u);
    assert.equal(f.writes.length, 1);
  } finally {
    await f.finish();
  }
  assert.equal(f.writes.length, 1);
  assert.equal(f.counts().starts, 1);
  assert.throws(() => f.owner.postMessage({ late: true }), /transport refused/u);
});

test("a retirement timer requests exact-owner termination but still awaits process and pipes", async () => {
  const f = fixture({ autoExit: false, holdClose: true });
  let result;
  let early;
  try {
    await f.start();
    const retiring = f.owner.close();
    let settled = false;
    void retiring.then(() => {
      settled = true;
    });
    await flush();
    f.timers[0].callback();
    await flush();
    early = settled;
    f.closeGate.resolve();
    result = await retiring;
    f.timers[0].callback();
  } finally {
    await f.finish();
  }
  assert.equal(early, false);
  assert.equal(result.forced, true);
  assert.equal(result.process_waited, true);
  assert.equal(result.exit_code, -9);
  assert.equal(result.successful, false);
  assert.equal(f.counts().kills, 1);
});

test("malformed lengths, short buffers, invalid UTF-8 and JSON fail closed without raw errors", async () => {
  for (const variant of ["empty", "oversize", "short", "utf8", "json"]) {
    const f = fixture();
    let result;
    try {
      await f.start();
      const header = new ArrayBuffer(4);
      new DataView(header).setUint32(
        0,
        variant === "empty" ? 0 : variant === "oversize" ? 1048577 : variant === "utf8" ? 3 : 1,
        true,
      );
      f.reads.stdout.shift().resolve(variant === "short" ? new ArrayBuffer(3) : header);
      await flush();
      if (variant === "utf8" || variant === "json") {
        f.reads.stdout
          .shift()
          .resolve((variant === "utf8" ? Uint8Array.of(34, 255, 34) : Uint8Array.of(123)).buffer);
        await flush();
      }
      result = await f.owner.close();
    } finally {
      await f.finish();
    }
    assert.equal(result.successful, false, variant);
    assert.equal(result.process_waited, true, variant);
    assert.equal(f.messages.length, 0, variant);
    assert.equal(JSON.stringify(result).includes("private raw"), false);
  }
});

test("serialization reentry cannot enqueue after close and caller mutation cannot alter queued bytes", async () => {
  const f = fixture({ holdWrites: true });
  try {
    await f.start();
    const message = { value: "before" };
    f.owner.postMessage(message);
    message.value = "after";
    assert.throws(
      () =>
        f.owner.postMessage({
          toJSON() {
            void f.owner.close();
            return {};
          },
        }),
      /transport refused/u,
    );
  } finally {
    await f.finish();
  }
  const body = new Uint8Array(f.writes[0].buffer, 4);
  assert.deepEqual(JSON.parse(new TextDecoder().decode(body)), { value: "before" });
  assert.equal(f.writes.length, 1);
});

test("unsolicited successful exit and failed exit are not successful intentional retirement", async () => {
  for (const exitCode of [0, 7]) {
    const f = fixture({ autoExit: false });
    await f.start();
    f.exit.resolve({ exitCode });
    await flush();
    const result = await f.owner.close();
    await f.finish();
    assert.equal(result.process_waited, true);
    assert.equal(result.exit_code, exitCode);
    assert.equal(result.successful, false);
  }
});

test("callbacks are independently retained without delaying the native close request", async () => {
  for (const kind of ["message", "disconnect"]) {
    const pending = deferred();
    const f = fixture({
      onMessage: () => (kind === "message" ? pending.promise : undefined),
      onDisconnect: () => (kind === "disconnect" ? pending.promise : undefined),
    });
    let before;
    let result;
    try {
      await f.start();
      await f.deliver({ native: "pending callback" });
      const retiring = f.owner.close();
      let settled = false;
      void retiring.then(() => {
        settled = true;
      });
      await flush();
      before = { settled, closes: f.closes.length };
      pending.resolve();
      result = await retiring;
    } finally {
      pending.resolve();
      await f.finish();
    }
    assert.deepEqual(before, { settled: false, closes: 3 }, kind);
    assert.equal(result.successful, true, kind);
  }
});

test("wait and pipe-close failures remain failed observations, not joined success", async () => {
  for (const mode of ["wait", "pipe"]) {
    const f = fixture();
    if (mode === "wait")
      f.process.wait = () => Promise.reject(new Error("private raw wait failure"));
    else {
      const close = f.process.stdout.close;
      f.process.stdout.close = async (force) => {
        await close(force);
        throw new Error("private raw pipe close failure");
      };
    }
    await f.start();
    await flush();
    const result = await f.owner.close();
    await f.finish();
    assert.equal(result.successful, false);
    assert.equal(mode === "wait" ? result.process_waited : result.pipes_closed, false);
    if (mode === "wait") assert.equal(f.counts().kills, 1);
    assert.equal(JSON.stringify(result).includes("private raw"), false);
  }
});

test("reentrant close returns one memoized retirement and callback failure is sanitized", async () => {
  let reentrant;
  let f;
  f = fixture({
    onDisconnect: () => {
      reentrant = f.owner.close();
      throw new Error("private raw disconnect failure");
    },
  });
  await f.start();
  const retiring = f.owner.close();
  const result = await retiring;
  await f.finish();
  assert.equal(reentrant, retiring);
  assert.equal(f.owner.close(), retiring);
  assert.equal(f.counts().disconnects, 1);
  assert.equal(result.process_waited, true);
  assert.equal(result.successful, false);
  assert.equal(JSON.stringify(result).includes("private raw"), false);
});

test("buffer branding cannot be spoofed, and oversize or non-JSON outbound values are refused", async () => {
  const f = fixture();
  let result;
  try {
    await f.start();
    const cycle = {};
    cycle.self = cycle;
    for (const message of [undefined, cycle, { large: "x".repeat(1048576) }]) {
      assert.throws(() => f.owner.postMessage(message), /transport refused/u);
    }
    f.reads.stdout.shift().resolve({ byteLength: 4, [Symbol.toStringTag]: "ArrayBuffer" });
    await flush();
    result = await f.owner.close();
  } finally {
    await f.finish();
  }
  assert.equal(result.successful, false);
  assert.equal(f.messages.length, 0);
  assert.equal(f.writes.length, 0);
});

test("short writes and early stderr failure retire rather than keeping a partially observed port", async () => {
  for (const mode of ["write", "stderr"]) {
    const f = fixture({ holdWrites: true });
    await f.start();
    if (mode === "write") {
      f.owner.postMessage({ fixed: "partial write" });
      f.writes[0].resolve({ bytesWritten: 1 });
    } else f.reads.stderr.shift().reject(new Error("private raw stderr failure"));
    await flush();
    const result = await f.owner.close();
    await f.finish();
    assert.equal(result.process_waited, true);
    assert.equal(result.successful, false);
    assert.equal(f.counts().starts, 1);
  }
});

test("a frame already received but not dispatched cannot cross the close boundary", async () => {
  const f = fixture();
  let result;
  try {
    await f.start();
    const bytes = new TextEncoder().encode(JSON.stringify({ native: "late receipt" }));
    const header = new ArrayBuffer(4);
    new DataView(header).setUint32(0, bytes.length, true);
    f.reads.stdout.shift().resolve(header);
    await flush();
    f.reads.stdout.shift().resolve(bytes.buffer);
    // Revoke before the already-fulfilled read's continuation executes.
    result = await f.owner.close();
  } finally {
    await f.finish();
  }
  assert.equal(result.successful, true);
  assert.deepEqual(f.messages, []);
});

test("intentional retirement with a nonzero exit and late callback failure cannot report success", async () => {
  for (const mode of ["exit", "callback"]) {
    const pending = deferred();
    const f = fixture({ autoExit: false, onMessage: () => pending.promise });
    let result;
    try {
      await f.start();
      if (mode === "callback") await f.deliver({ native: "callback in progress" });
      const retiring = f.owner.close();
      f.exit.resolve({ exitCode: mode === "exit" ? 7 : 0 });
      pending.reject(new Error("private raw late callback failure"));
      // In the exit-only variant the callback was never invoked.
      if (mode === "exit") await pending.promise.catch(() => {});
      result = await retiring;
    } finally {
      pending.resolve();
      await f.finish();
    }
    assert.equal(result.process_waited, true, mode);
    assert.equal(result.successful, false, mode);
    assert.equal(JSON.stringify(result).includes("private raw"), false);
  }
});

test("a retired owner cannot return historical startup success or launch a replacement", async () => {
  const f = fixture();
  assert.equal(await f.start(), true);
  const result = await f.finish();
  const restarted = await f.owner.start();
  assert.equal(result.successful, true);
  assert.equal(restarted, false);
  assert.equal(f.counts().starts, 1);
});
