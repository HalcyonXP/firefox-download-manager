import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
const source = readFileSync(new URL("qualification/startup_review.js", import.meta.url), "utf8");
function fixture() {
  const state = { modal: true, replies: 0 };
  const browser = { currentURI: { spec: "about:blank" } };
  const doc = { documentURI: "chrome://browser/content/spotlight.html" };
  const config = {
    id: "NEW_USER_TOU_ONBOARDING",
    get password() {
      throw new Error("input read refused");
    },
  };
  const frame = { contentDocument: doc, contentWindow: { arguments: [config] } };
  const dialog = { _frame: frame, _openedURL: doc.documentURI, _dialogReady: Promise.resolve() };
  const window = {
    gBrowser: { selectedBrowser: browser, selectedTab: { linkedBrowser: browser } },
    gDialogBox: { dialog },
    document: {
      documentElement: {
        hasAttribute(name) {
          assert.equal(name, "window-modal-open");
          return state.modal;
        },
      },
    },
  };
  frame.ownerDocument = window.document;
  const context = {
    window,
    ChromeUtils: {
      importESModule(name) {
        assert.equal(name, "chrome://remote/content/shared/NavigableManager.sys.mjs");
        return {
          NavigableManager: {
            getIdForBrowser(value) {
              assert.equal(value, browser);
              return "control";
            },
          },
        };
      },
    },
  };
  return {
    state,
    window,
    dialog,
    frame,
    doc,
    config,
    invoke() {
      return new Promise((resolve) => {
        context.arguments = [
          "control",
          (value) => {
            state.replies++;
            resolve(JSON.parse(JSON.stringify(value)));
          },
        ];
        vm.runInNewContext(source, context);
      });
    },
  };
}
test("only original blank window and known terms configuration yields terms, never consent", async () => {
  const f = fixture();
  assert.deepEqual(await f.invoke(), { version: 1, state: "terms" });
  assert.equal(f.state.replies, 1);
});
test("a structurally clear original window needs no configuration or action", async () => {
  const f = fixture();
  f.state.modal = false;
  Object.defineProperty(f.window, "gDialogBox", {
    get() {
      throw new Error("unneeded dialog read");
    },
  });
  assert.deepEqual(await f.invoke(), { version: 1, state: "clear" });
});
test("foreign identity or selected browser refuses before dialog access", async () => {
  for (const change of [
    (f) => {
      f.window.gBrowser.selectedTab.linkedBrowser = {};
    },
    (f) => {
      f.window.gBrowser.selectedBrowser.currentURI.spec = "opaque-url";
    },
  ]) {
    const f = fixture();
    change(f);
    assert.equal((await f.invoke()).state, "unavailable");
  }
});
test("original readiness is retained and replacement is unavailable without adoption", async () => {
  for (const replace of [false, true]) {
    const f = fixture();
    let settle;
    f.dialog._openedURL = null;
    const original = new Promise((resolve) => {
      settle = resolve;
    });
    f.dialog._dialogReady = original;
    const pending = f.invoke();
    await Promise.resolve();
    assert.equal(f.state.replies, 0);
    if (replace) f.dialog._dialogReady = Promise.resolve();
    f.dialog._openedURL = f.doc.documentURI;
    settle();
    assert.equal((await pending).state, replace ? "unavailable" : "terms");
  }
});
test("unknown or accessor IDs and nonexact documents never authorize review", async () => {
  for (const change of [
    (f) => {
      f.config.id = "opaque-id";
    },
    (f) => {
      Object.defineProperty(f.config, "id", {
        get() {
          throw new Error("id getter invoked");
        },
      });
    },
    (f) => {
      f.dialog._openedURL += "?opaque";
    },
    (f) => {
      f.doc.documentURI = "about:blank";
    },
  ]) {
    const f = fixture();
    change(f);
    const value = await f.invoke();
    assert.equal(value.state, "unsupported");
    assert.equal(JSON.stringify(value).includes("opaque"), false);
  }
});
test("ready rejection never serializes exception details", async () => {
  const f = fixture();
  f.dialog._dialogReady = Promise.reject(new Error("opaque-error"));
  assert.deepEqual(await f.invoke(), { version: 1, state: "unavailable" });
  assert.equal(f.state.replies, 1);
});
