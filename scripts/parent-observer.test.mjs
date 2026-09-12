import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
const source = readFileSync("scripts/qualification/parent_observer.js", "utf8");
const nonce = "11111111-1111-4111-8111-111111111111";
const collector = "22222222-2222-4222-8222-222222222222";
const other = "33333333-3333-4333-8333-333333333333";
const topic = "download-manager-owned-parent-fixture";
function fixture(mode = {}) {
  const observers = new Set();
  const calls = [];
  const obs = {
    addObserver(observer, name, weak) {
      assert.equal(name, topic);
      assert.equal(weak, false);
      calls.push("add");
      observers.add(observer);
      if (mode.addThrows) throw Error("modeled add refused");
    },
    removeObserver(observer, name) {
      assert.equal(name, topic);
      assert(observers.has(observer));
      calls.push("remove");
      if (mode.removeThrows) throw Error("modeled removal refused");
      if (!mode.noRemove) observers.delete(observer);
    },
    notifyObservers(subject, name, data) {
      calls.push("notify");
      if (!mode.noNotify) for (const observer of observers) observer.observe(subject, name, data);
    },
  };
  const context = vm.createContext({
    Services: { obs, appinfo: { processType: mode.processType ?? 0 } },
  });
  if (mode.foreignSlot) context.__ownedParentFixtureObserverV1 = mode.foreignSlot;
  function invoke(operation, token = collector, n = nonce) {
    let result;
    let delivered = 0;
    context.arguments = [
      operation,
      n,
      token,
      (value) => {
        result = JSON.parse(JSON.stringify(value));
        delivered++;
      },
    ];
    new vm.Script(source).runInContext(context);
    assert.equal(delivered, 1);
    return result;
  }
  return {
    invoke,
    calls,
    observers,
    send: (data, subject = null, name = topic) => obs.notifyObservers(subject, name, data),
  };
}
const record = (kind) =>
  JSON.stringify({ nonce, kind, value: { stage: kind === "ready" ? "echoed" : "retired" } });
test("collector retains exact strings, not API objects, and confirms only its removal", () => {
  const f = fixture();
  const initial = f.invoke("install");
  assert.equal(initial.state, "active");
  assert.deepEqual(initial.records, []);
  const ready = record("ready"),
    retired = record("retired");
  f.send(ready);
  f.send(retired);
  const copy = f.invoke("snapshot");
  assert.deepEqual(copy.records, [ready, retired]);
  copy.records.length = 0;
  assert.equal(f.invoke("snapshot").records.length, 2);
  const closed = f.invoke("remove");
  assert.equal(closed.removed, true);
  assert.equal(closed.failed, false);
  assert.equal(f.observers.size, 0);
  assert.deepEqual(f.invoke("remove"), closed);
  assert.deepEqual(f.calls, ["add", "notify", "notify", "notify", "remove", "notify"]);
  assert.throws(() => f.invoke("install"), /refused/);
});
test("foreign correlation is ignored; wrong collector cannot observe or remove ownership", () => {
  const f = fixture();
  f.invoke("install");
  f.send(JSON.stringify({ nonce: other, kind: "ready" }));
  f.send(record("ready"), null, "other-topic");
  assert.throws(() => f.invoke("snapshot", other), /refused/);
  assert.throws(() => f.invoke("remove", other), /refused/);
  assert.throws(() => f.invoke("snapshot", collector, other), /refused/);
  assert.deepEqual(f.invoke("snapshot").records, []);
  assert.equal(f.observers.size, 1);
  assert.equal(f.invoke("remove").removed, true);
});
test("registration acting before throwing keeps exact removal closure", () => {
  const f = fixture({ addThrows: true });
  const value = f.invoke("install");
  assert.equal(value.state, "uncertain");
  assert.equal(value.failed, true);
  assert.equal(f.observers.size, 1);
  const closed = f.invoke("remove");
  assert.equal(closed.failed, true);
  assert.equal(closed.removed, true);
  assert.equal(f.observers.size, 0);
});
test("removal throwing or silently retaining the observer cannot claim removal", () => {
  for (const mode of [{ removeThrows: true }, { noRemove: true }]) {
    const f = fixture(mode);
    f.invoke("install");
    const closed = f.invoke("remove");
    assert.equal(closed.failed, true);
    assert.equal(closed.removed, false);
    assert.equal(f.observers.size, 1);
    f.invoke("remove");
    assert.equal(f.calls.filter((x) => x === "remove").length, 1);
  }
});
test("suppressed control delivery cannot turn a failed removal into success", () => {
  const f = fixture({ noRemove: true, noNotify: true });
  f.invoke("install");
  const closed = f.invoke("remove");
  assert.equal(closed.failed, true);
  assert.equal(closed.removed, false);
  assert.equal(f.observers.size, 1);
});
test("malformed, oversized, non-ASCII and subject-bearing data cause sticky refusal", () => {
  for (const [data, subject] of [
    ["{", null],
    ["x".repeat(8193), null],
    ["π", null],
    [record("ready"), {}],
  ]) {
    const f = fixture();
    f.invoke("install");
    f.send(data, subject);
    f.send(record("ready"));
    assert.equal(f.invoke("snapshot").failed, true);
    assert.equal(f.invoke("remove").removed, true);
  }
});
test("third correlated notification cannot overwrite or grow the two-record bound", () => {
  const f = fixture();
  f.invoke("install");
  f.send(record("ready"));
  f.send(record("retired"));
  f.send(record("retired"));
  const snapshot = f.invoke("snapshot");
  assert.equal(snapshot.failed, true);
  assert.equal(snapshot.records.length, 2);
  assert.equal(f.invoke("remove").removed, true);
});
test("content-process or nonnumeric process classification cannot register", () => {
  for (const processType of [1, false, "0"]) {
    const f = fixture({ processType });
    assert.throws(() => f.invoke("install"), /refused/);
    assert.deepEqual(f.calls, []);
  }
});
test("invalid commands and absent slots are not adopted", () => {
  const f = fixture();
  for (const [op, token, n] of [
    ["snapshot", collector, nonce],
    ["install", nonce, nonce],
    ["install", collector, "invalid"],
    ["other", collector, nonce],
  ])
    assert.throws(() => f.invoke(op, token, n), /refused/);
  assert.equal(f.observers.size, 0);
});
test("a foreign collector with the public nonce is not adopted for cleanup", () => {
  const f = fixture({
    foreignSlot: {
      nonce,
      collector: other,
      remove() {
        assert.fail("foreign removal");
      },
      snapshot() {
        assert.fail("foreign read");
      },
    },
  });
  assert.throws(() => f.invoke("install"), /refused/);
  assert.throws(() => f.invoke("remove"), /refused/);
  assert.deepEqual(f.calls, []);
});
test("exact duplicate-key text reaches the external strict parser without normalization", () => {
  const f = fixture();
  f.invoke("install");
  const raw = `{"nonce":"${other}","nonce":"${nonce}","kind":"ready"}`;
  f.send(raw);
  assert.deepEqual(f.invoke("snapshot").records, [raw]);
  assert.equal(f.invoke("remove").removed, true);
});
