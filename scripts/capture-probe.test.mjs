import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

const source = readFileSync("scripts/qualification/capture_probe/background.js", "utf8");
const origin = "http://127.0.0.1:12345";
const own = { id: "probe", url: "moz-extension://probe/inspect.html" };
function fixture() {
  const listeners = {};
  const timers = [];
  const event = (name) => ({
    addListener: (callback) => {
      listeners[name] = callback;
    },
  });
  const browser = {
    runtime: {
      id: "probe",
      getURL: (name) => `moz-extension://probe/${name}`,
      onMessage: event("message"),
    },
    webRequest: Object.fromEntries(
      [
        "onBeforeRequest",
        "onHeadersReceived",
        "onBeforeRedirect",
        "onCompleted",
        "onErrorOccurred",
      ].map((name) => [name, event(name)]),
    ),
  };
  vm.runInNewContext(source, { browser, URL, setTimeout: (callback) => timers.push(callback) });
  const message = (value, sender = own) => listeners.message(value, sender);
  const click = (target = "/direct", trusted = true) =>
    message(
      { action: "click", target: origin + target, trusted },
      { id: "probe", tab: { id: 1 }, frameId: 0, url: origin + "/page" },
    );
  return { listeners, timers, message, click };
}
function request(changes = {}) {
  return {
    requestId: "first",
    url: origin + "/direct",
    tabId: 1,
    frameId: 0,
    originUrl: origin + "/page",
    incognito: false,
    cookieStoreId: "firefox-default",
    method: "GET",
    type: "main_frame",
    statusCode: 200,
    responseHeaders: [{ name: "Content-Disposition", value: 'attachment; filename="owned.bin"' }],
    ...changes,
  };
}

test("diagnostic cancellation really waits on a promise and consumes one click", async () => {
  const probe = fixture();
  await probe.message({ action: "reset", cancel: true });
  probe.click();
  const first = probe.listeners.onHeadersReceived(request());
  let settled = false;
  void first.then(() => {
    settled = true;
  });
  await Promise.resolve();
  assert.equal(settled, false);
  assert.equal(probe.timers.length, 1);
  probe.timers.shift()();
  assert.equal((await first).cancel, true);
  const second = probe.listeners.onHeadersReceived(request({ requestId: "second" }));
  assert.equal(second.cancel, undefined);
  assert.equal(typeof second.then, "undefined");
  assert.equal(probe.timers.length, 0);
});

test("redirect retains initial request identity, not a new click for the final target", async () => {
  const probe = fixture();
  await probe.message({ action: "reset", cancel: true });
  probe.click("/redirect");
  const initial = request({ url: origin + "/redirect", statusCode: 302, responseHeaders: [] });
  probe.listeners.onBeforeRequest(initial);
  assert.equal(probe.listeners.onHeadersReceived(initial).cancel, undefined);
  const result = probe.listeners.onHeadersReceived(request({ url: origin + "/attachment" }));
  probe.timers.shift()();
  assert.equal((await result).cancel, true);
});

test("unsafe or missing context, untrusted click, unarmed and overflow never cancel", async () => {
  for (const changes of [
    { method: "POST" },
    { frameId: 1 },
    { type: "sub_frame" },
    { incognito: true },
    { incognito: undefined },
    { cookieStoreId: undefined },
    { cookieStoreId: "firefox-container-1" },
    { originUrl: origin + "/other" },
    { tabId: 2 },
    { statusCode: 302 },
    { responseHeaders: [] },
  ]) {
    const probe = fixture();
    await probe.message({ action: "reset", cancel: true });
    probe.click();
    assert.equal(probe.listeners.onHeadersReceived(request(changes)).cancel, undefined);
    assert.equal(probe.timers.length, 0);
  }
  for (const mode of ["untrusted", "unarmed", "overflow"]) {
    const probe = fixture();
    await probe.message({ action: "reset", cancel: mode !== "unarmed" });
    probe.click("/direct", mode !== "untrusted");
    if (mode === "overflow") for (let i = 0; i < 8; i++) probe.click();
    const result = probe.listeners.onHeadersReceived(request());
    assert.equal(result.cancel, undefined);
    assert.equal(typeof result.then, "undefined");
    assert.equal(probe.timers.length, 0);
  }
});

test("observer controls require its own page and snapshots do not emit URLs or errors", async () => {
  const probe = fixture();
  assert.equal(
    probe.message({ action: "reset", cancel: true }, { id: "foreign", url: own.url }),
    undefined,
  );
  assert.equal(
    probe.message({ action: "snapshot" }, { id: "probe", url: origin + "/page" }),
    undefined,
  );
  await probe.message({ action: "reset", cancel: false });
  probe.listeners.onErrorOccurred(
    request({ url: origin + "/direct?private=canary", error: "sensitive-canary" }),
  );
  const snapshot = await probe.message({ action: "snapshot" });
  const encoded = JSON.stringify(snapshot);
  assert.equal(encoded.includes(origin), false);
  assert.equal(encoded.includes("canary"), false);
  assert.equal(snapshot.records[0].errorKind, "other");
});
