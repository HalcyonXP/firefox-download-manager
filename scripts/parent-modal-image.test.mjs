import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

const source = readFileSync(
  new URL("qualification/parent_modal_image.js", import.meta.url),
  "utf8",
);
function fixture() {
  const state = {
    draws: 0,
    closed: 0,
    canvases: 0,
    replies: 0,
    type: "prompt:alert",
    hidden: true,
  };
  const uri = "chrome://global/content/commonDialog.xhtml";
  const doc = {
    documentURI: uri,
    getElementById(id) {
      if (id === "commonDialog") return { getAttribute: () => state.type };
      assert.ok(["loginContainer", "password1Container"].includes(id));
      return {
        hidden: state.hidden,
        get value() {
          throw new Error("input read refused");
        },
      };
    },
  };
  const rectangle = { x: 20, y: 30, width: 100, height: 80 };
  const frame = { contentDocument: doc, getBoundingClientRect: () => ({ ...rectangle }) };
  const dialog = { _openedURL: uri, _frame: frame, _dialogReady: Promise.resolve() };
  const browser = { currentURI: { spec: "about:blank" } };
  const bitmap = {
    close() {
      state.closed++;
      if (state.closeError) throw new Error("close refused");
    },
  };
  const canvas = {
    width: 0,
    height: 0,
    getContext(type) {
      assert.equal(type, "2d");
      return {
        drawImage(value, x, y) {
          assert.equal(value, bitmap);
          assert.equal(x, 0);
          assert.equal(y, 0);
        },
      };
    },
    toDataURL(type) {
      assert.equal(type, "image/png");
      return "data:image/png;base64,cGl4ZWxz";
    },
  };
  const window = {
    innerWidth: 800,
    innerHeight: 600,
    gBrowser: { selectedBrowser: browser },
    gDialogBox: { dialog },
    document: {
      documentElement: {
        hasAttribute(name) {
          assert.equal(name, "window-modal-open");
          return true;
        },
      },
      createElementNS(ns, name) {
        assert.equal(ns, "http://www.w3.org/1999/xhtml");
        assert.equal(name, "canvas");
        state.canvases++;
        return canvas;
      },
    },
    browsingContext: {
      currentWindowGlobal: {
        async drawSnapshot(rect, scale, color, options) {
          state.draws++;
          assert.deepEqual({ ...rect }, rectangle);
          assert.equal(scale, 1);
          assert.equal(color, "rgb(255,255,255)");
          assert.equal(options.drawView, true);
          if (state.afterDraw) await state.afterDraw();
          return bitmap;
        },
      },
    },
  };
  frame.ownerDocument = window.document;
  const context = {
    window,
    DOMRect: class {
      constructor(x, y, width, height) {
        Object.assign(this, { x, y, width, height });
      }
    },
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
    doc,
    rectangle,
    canvas,
    async invoke() {
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

test("only the retained non-input dialog rectangle produces private pixels and releases resources", async () => {
  const f = fixture();
  const value = await f.invoke();
  assert.deepEqual(value, {
    version: 2,
    kind: "common",
    prompt: "alert",
    message: "unknown",
    png: "cGl4ZWxz",
  });
  assert.equal(f.state.draws, 1);
  assert.equal(f.state.closed, 1);
  assert.equal(f.state.replies, 1);
  assert.equal(f.canvas.width, 0);
  assert.equal(f.canvas.height, 0);
});

test("authentication, input and unknown prompt kinds never capture or read values", async () => {
  for (const type of [
    "prompt:promptUserAndPass",
    "prompt:promptPassword",
    "prompt:prompt",
    "untrusted-value",
  ]) {
    const f = fixture();
    f.state.type = type;
    const result = await f.invoke();
    assert.equal(result.png, null);
    assert.equal(f.state.draws, 0);
    assert.equal(f.state.canvases, 0);
    assert.equal(JSON.stringify(result).includes("untrusted-value"), false);
  }
});

test("foreign windows, different dialog URIs and visible input containers refuse pixels", async () => {
  const changes = [
    (f) => {
      f.window.gBrowser.selectedBrowser.currentURI.spec = "opaque-url";
    },
    (f) => {
      f.dialog._openedURL = "opaque-url";
    },
    (f) => {
      f.doc.documentURI = "opaque-url";
    },
    (f) => {
      f.state.hidden = false;
    },
  ];
  for (const change of changes) {
    const f = fixture();
    change(f);
    const result = await f.invoke();
    assert.equal(result.png, null);
    assert.equal(f.state.draws, 0);
    assert.equal(JSON.stringify(result).includes("opaque-url"), false);
  }
});

test("off-window, nonfinite and oversized rectangles cannot expand capture", async () => {
  for (const patch of [
    { x: -1 },
    { y: -1 },
    { width: 1601 },
    { height: 1201 },
    { width: 900 },
    { height: 900 },
    { width: NaN },
    { width: 0 },
  ]) {
    const f = fixture();
    Object.assign(f.rectangle, patch);
    assert.equal((await f.invoke()).png, null);
    assert.equal(f.state.draws, 0);
  }
});

test("replacement or movement while awaiting snapshot discards pixels and closes original bitmap", async () => {
  for (const change of [
    (f) => {
      f.window.gDialogBox.dialog = {};
    },
    (f) => {
      f.rectangle.x++;
    },
    (f) => {
      f.state.type = "prompt:promptPassword";
    },
  ]) {
    const f = fixture();
    f.state.afterDraw = () => change(f);
    assert.equal((await f.invoke()).png, null);
    assert.equal(f.state.closed, 1);
    assert.equal(f.state.canvases, 0);
    assert.equal(f.state.replies, 1);
  }
});

test("bitmap close failure refuses pixels while detached canvas is cleared without replay", async () => {
  const f = fixture();
  f.state.closeError = true;
  const result = await f.invoke();
  assert.equal(result.kind, "unavailable");
  assert.equal(result.png, null);
  assert.equal(f.state.closed, 1);
  assert.equal(f.canvas.width, 0);
  assert.equal(f.canvas.height, 0);
  assert.equal(f.state.replies, 1);
});

test("an opening original dialog waits for its own readiness before classifying or capturing", async () => {
  const f = fixture();
  let settle;
  f.dialog._openedURL = null;
  f.dialog._dialogReady = new Promise((resolve) => {
    settle = resolve;
  });
  const pending = f.invoke();
  await Promise.resolve();
  assert.equal(f.state.draws, 0);
  assert.equal(f.state.replies, 0);
  f.dialog._openedURL = f.doc.documentURI;
  settle();
  assert.equal((await pending).kind, "common");
  assert.equal(f.state.draws, 1);
  assert.equal(f.state.closed, 1);
});

test("original readiness rejection or replacement refuses without adopting another dialog", async () => {
  for (const mode of ["reject", "dialog", "frame", "uri"]) {
    const f = fixture();
    let settle, reject;
    f.dialog._dialogReady = new Promise((a, b) => {
      settle = a;
      reject = b;
    });
    const pending = f.invoke();
    await Promise.resolve();
    if (mode === "reject") reject(new Error("opaque-ready-error"));
    else {
      if (mode === "dialog") f.window.gDialogBox.dialog = {};
      if (mode === "frame") f.dialog._frame = {};
      if (mode === "uri") f.dialog._openedURL = "chrome://browser/content/spotlight.html";
      settle();
    }
    const result = await pending;
    assert.equal(result.kind, "unavailable");
    assert.equal(result.png, null);
    assert.equal(f.state.draws, 0);
    assert.equal(f.state.replies, 1);
    assert.equal(JSON.stringify(result).includes("opaque-ready-error"), false);
  }
});

function spotlight(f, config) {
  f.dialog._openedURL = f.doc.documentURI = "chrome://browser/content/spotlight.html";
  f.dialog._frame.contentWindow = { arguments: [config] };
}

test("known Spotlight configuration gets a fixed class only, never pixels or form values", async () => {
  const entries = [
    ["NEW_USER_TOU_ONBOARDING", "new-user-terms"],
    ["PRE_ONBOARDING_SPLASH", "startup-splash"],
    ["AI_WINDOW_TOU_EXISTING_USERS_MODAL", "ai-window-terms"],
    ["LOGIN_STATUS_ADVISORY", "login-advisory"],
    ["BROWSER_BACKUP_OPTIN_SPOTLIGHT", "backup-optin"],
    ["FX_MR_106_UPGRADE", "upgrade"],
  ];
  for (const [id, message] of entries) {
    const f = fixture();
    spotlight(f, {
      id,
      get password() {
        throw new Error("input read refused");
      },
    });
    const result = await f.invoke();
    assert.deepEqual(result, {
      version: 2,
      kind: "spotlight",
      prompt: "unknown",
      message,
      png: null,
    });
    assert.equal(f.state.draws, 0);
    assert.equal(f.state.canvases, 0);
  }
});

test("unknown and accessor message identities do not leak or invoke the id accessor", async () => {
  for (const config of [
    { id: "opaque-untrusted-id" },
    {
      get id() {
        throw new Error("accessor invoked");
      },
    },
    null,
    {
      id: {
        toString() {
          throw new Error("coercion refused");
        },
      },
    },
  ]) {
    const f = fixture();
    spotlight(f, config);
    const result = await f.invoke();
    assert.equal(result.kind, "spotlight");
    assert.equal(result.message, "unknown");
    assert.equal(result.png, null);
    assert.equal(JSON.stringify(result).includes("opaque-untrusted-id"), false);
    assert.equal(f.state.draws, 0);
  }
});

test("Spotlight identity requires exact URL and original content document", async () => {
  for (const url of [
    "chrome://browser/content/spotlight.html?opaque",
    "chrome://browser/content/spotlight.html#opaque",
    "https://example.invalid/spotlight.html",
  ]) {
    const f = fixture();
    spotlight(f, { id: "NEW_USER_TOU_ONBOARDING" });
    f.dialog._openedURL = url;
    const result = await f.invoke();
    assert.equal(result.kind, "other");
    assert.equal(result.message, "unknown");
    assert.equal(result.png, null);
  }
  const f = fixture();
  spotlight(f, { id: "NEW_USER_TOU_ONBOARDING" });
  f.doc.documentURI = "about:blank";
  let argumentReads = 0;
  Object.defineProperty(f.dialog._frame, "contentWindow", {
    get() {
      argumentReads++;
      return { arguments: [{ id: "NEW_USER_TOU_ONBOARDING" }] };
    },
  });
  assert.equal((await f.invoke()).kind, "unavailable");
  assert.equal(argumentReads, 0);
  assert.equal(f.state.draws, 0);
});
