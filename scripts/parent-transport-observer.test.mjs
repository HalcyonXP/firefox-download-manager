import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { observerFixture, collector, other } from "./parent-observer-fixture.mjs";
const source = readFileSync("scripts/qualification/parent_transport_observer.js", "utf8");
const topic = "download-manager-parent-retirement";
const fixture = (mode) => observerFixture(source, topic, "__ownedParentTransportObserverV1", mode);
const raw = JSON.stringify({ version: 1, connection: 1, receipt: { successful: true } }); // Collector strings only; Python separately rejects incomplete receipts.

test("installed parent collector keeps one original string, not an API object or native join", () => {
  const f = fixture();
  assert.equal(f.invoke("install").state, "active");
  f.send(raw);
  const value = f.invoke("snapshot");
  assert.deepEqual(value.records, [raw]);
  value.records.length = 0;
  assert.deepEqual(f.invoke("snapshot").records, [raw]);
  const closed = f.invoke("remove");
  assert.equal(closed.removed, true);
  assert.equal(closed.failed, false);
  assert.equal(f.observers.size, 0);
  assert.deepEqual(f.calls, ["add", "notify", "notify", "remove", "notify"]);
  assert.deepEqual(f.invoke("remove"), closed);
});
test("collector controls are independently correlated and cannot borrow another collector", () => {
  const f = fixture();
  f.invoke("install");
  f.send(JSON.stringify({ nonce: other, collector, control: "before-remove-check" }));
  assert.deepEqual(f.invoke("snapshot").records, []);
  assert.throws(() => f.invoke("remove", other), /refused/);
  assert.throws(() => f.invoke("snapshot", collector, other), /refused/);
  assert.equal(f.invoke("remove").removed, true);
});
test("second native record, malformed data, subject and non-ASCII remain failed", () => {
  for (const [body, subject] of [
    [raw, null],
    ["{", null],
    [raw, {}],
    ["π", null],
  ]) {
    const f = fixture();
    f.invoke("install");
    f.send(raw);
    f.send(body, subject);
    assert.equal(f.invoke("snapshot").failed, true);
    const closed = f.invoke("remove");
    assert.equal(closed.removed, true);
    assert.equal(closed.failed, true);
  }
});
test("suppressed notification, missing inverse and throwing inverse cannot become removal", () => {
  for (const mode of [{ noNotify: true }, { noRemove: true }, { removeThrows: true }]) {
    const f = fixture(mode);
    f.invoke("install");
    const value = f.invoke("remove");
    assert.equal(value.failed, true);
    assert.equal(value.removed, false);
    assert.deepEqual(f.invoke("remove"), value);
    assert.equal(f.calls.filter((c) => c === "remove").length, 1);
  }
});
