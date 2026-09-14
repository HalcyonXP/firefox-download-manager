import assert from "node:assert/strict";
import test from "node:test";
import { ParentFixtureSession } from "../extension/parent-probe/session.js";

const MARKER = "owned-parent-stdio-v1";
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
  process = { pid: 44 },
  holdStart = false,
  holdRetirement = false,
  notify,
} = {}) {
  const starting = deferred();
  const retirement = deferred();
  if (!holdStart) starting.resolve(process);
  if (!holdRetirement) retirement.resolve();
  const owners = [];
  const sent = [];
  const events = [];
  let calls = 0;
  const launchers = [];
  class ModeledLauncher {
    constructor(extension, platform, callbacks) {
      this.platform = platform;
      this.callbacks = callbacks;
      this.started = null;
      this.retired = null;
      this.closed = false;
      this.failed = false;
      launchers.push(this);
    }
    start() {
      this.started = this.platform.spawn({ fixed: true }).then(
        (value) => {
          owners.push(value);
          if (this.closed) return false;
          this.emit({ fixture: MARKER, op: "ready", pid: 44 });
          return !this.closed;
        },
        () => {
          this.failed = true;
          void this.close();
          return false;
        },
      );
      return this.started;
    }
    emit(message) {
      try {
        this.callbacks.onMessage(message);
      } catch {
        this.failed = true;
        void this.close();
      }
    }
    postMessage(context, message) {
      sent.push(message);
      this.emit({ fixture: MARKER, op: "pong", value: "π" });
    }
    close() {
      if (this.retired !== null) return this.retired;
      this.closed = true;
      this.retired = Promise.resolve().then(async () => {
        if (this.started !== null) await this.started;
        await retirement.promise;
        return Object.freeze({ successful: !this.failed, model: true });
      });
      // Returning session.close() here would create a real orchestration cycle.
      assert.equal(this.callbacks.onDisconnect(), undefined);
      return this.retired;
    }
  }
  const session = new ParentFixtureSession(
    {},
    {
      spawn: () => {
        calls++;
        return starting.promise;
      },
    },
    (kind, value) => {
      notify?.(kind, value);
      events.push([kind, value]);
    },
    ModeledLauncher,
  );
  return {
    session,
    starting,
    retirement,
    owners,
    sent,
    events,
    launcher: () => launchers[0],
    calls: () => calls,
  };
}

test("fixture orchestration retains one operation and native retirement without callback cycles", async () => {
  const f = fixture({ holdRetirement: true });
  const value = await f.session.run({});
  assert.deepEqual(value, {
    version: 1,
    qualification: false,
    scope: MARKER,
    stage: "echoed",
    pid: 44,
  });
  assert.deepEqual(f.sent, [{ fixture: MARKER, op: "ping", value: "π" }]);
  await assert.rejects(f.session.run({}), /fixture refused/u);
  assert.equal(f.calls(), 1);
  const retired = f.session.close();
  assert.equal(f.session.close(), retired);
  let settled = false;
  void retired.then(() => {
    settled = true;
  });
  await flush();
  const premature = settled;
  f.retirement.resolve();
  const receipt = await retired;
  assert.equal(premature, false);
  assert.equal(receipt.successful, true);
  assert.equal(receipt.qualification, false);
  assert.equal(receipt.launcher.model, true);
  assert.deepEqual(
    f.events.map(([kind]) => kind),
    ["ready", "retired"],
  );
  assert.throws(() => JSON.stringify(f.session), /not serializable/u);
  await assert.rejects(f.session.run({}), /fixture refused/u);
  assert.equal(f.calls(), 1);
});

test("diagnostic PID refusal never discards a returned SDK process owner", async () => {
  for (const value of [0, 1.5, 0x100000000, 99]) {
    const process = { pid: value };
    const f = fixture({ process });
    await assert.rejects(f.session.run({}), /fixture refused/u);
    const receipt = await f.session.close();
    assert.equal(receipt.successful, false);
    assert.equal(f.owners.length, 1);
    assert.equal(f.owners[0], process);
    assert.equal(f.sent.length, 0);
  }
  const process = {
    get pid() {
      throw new Error("opaque PID read");
    },
  };
  const f = fixture({ process });
  await assert.rejects(f.session.run({}), /fixture refused/u);
  assert.equal((await f.session.close()).successful, false);
  assert.equal(f.owners[0], process);
});

test("closing pending startup retains the later owner and cannot fabricate fixture completion", async () => {
  const f = fixture({ holdStart: true, holdRetirement: true });
  const running = f.session.run({});
  const observedRun = running.then(
    () => true,
    () => false,
  );
  await flush();
  const retired = f.session.close();
  let settled = false;
  void retired.then(() => {
    settled = true;
  });
  await flush();
  const beforeOwner = settled;
  const process = { pid: 44 };
  f.starting.resolve(process);
  await flush();
  const beforePipes = settled;
  f.retirement.resolve();
  const receipt = await retired;
  assert.equal(beforeOwner, false);
  assert.equal(beforePipes, false);
  assert.equal(await observedRun, false);
  assert.equal(receipt.successful, false);
  assert.equal(receipt.echoed, false);
  assert.equal(f.owners[0], process);
  assert.equal(f.sent.length, 0);
});

test("rejected startup and closure before the scheduled operation do not report success", async () => {
  const f = fixture({ holdStart: true });
  const running = f.session.run({});
  const rejection = assert.rejects(running, /fixture refused/u);
  await flush();
  f.starting.reject(new Error("opaque SDK start failure"));
  await rejection;
  assert.equal((await f.session.close()).successful, false);
  assert.equal(f.owners.length, 0);
  const early = fixture();
  const start = early.session.run({});
  const stopped = early.session.close();
  await assert.rejects(start, /fixture refused/u);
  assert.equal((await stopped).successful, false);
  assert.equal(early.calls(), 0);
});

test("only the closed fixture pong envelope can finish the roundtrip", async () => {
  for (const message of [
    null,
    [],
    { fixture: "other", op: "pong", value: "π" },
    { fixture: MARKER, op: "pong", value: "other" },
    { fixture: MARKER, op: "pong", value: "π", permit: true },
    { fixture: MARKER, op: "verdict", value: "π" },
  ]) {
    const f = fixture();
    f.launcher().postMessage = () => f.launcher().emit(message);
    await assert.rejects(f.session.run({}), /fixture refused/u);
    assert.equal((await f.session.close()).successful, false);
    assert.equal(f.owners.length, 1);
  }
});

test("failed retirement notification stays memoized after native retirement", async () => {
  const f = fixture({
    notify: (kind) => {
      if (kind === "retired") throw new Error("modeled retirement notification refused");
    },
  });
  await f.session.run({});
  // Model the synchronous transport hook, with no returned promise consumer.
  f.launcher().callbacks.onDisconnect();
  await flush();
  const retirement = f.session.close();
  await assert.rejects(retirement, /retirement notification refused/u);
  assert.equal(f.session.close(), retirement);
  assert.equal((await f.launcher().close()).successful, true);
  assert.deepEqual(
    f.events.map(([kind]) => kind),
    ["ready"],
  );
});

test("late duplicate data and failed ready notification remain unsuccessful observations", async () => {
  const f = fixture();
  await f.session.run({});
  f.launcher().emit({ fixture: MARKER, op: "pong", value: "π" });
  assert.equal((await f.session.close()).successful, false);
  const failure = fixture({
    notify: (kind) => {
      if (kind === "ready") throw new Error("opaque observer refusal");
    },
  });
  await assert.rejects(failure.session.run({}), /fixture refused/u);
  assert.equal((await failure.session.close()).successful, false);
  assert.deepEqual(
    failure.events.map(([kind]) => kind),
    ["retired"],
  );
  assert.equal(failure.owners.length, 1);
});
