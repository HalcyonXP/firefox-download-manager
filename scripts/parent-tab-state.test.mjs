import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

const source = readFileSync(new URL("qualification/parent_tab_state.js", import.meta.url), "utf8");
function fixture() {
  const browser = Object.freeze({ currentURI: Object.freeze({ spec: "about:blank" }) });
  const tab = Object.freeze({ linkedBrowser: browser });
  const calls = [];
  const window = {
    gBrowser: Object.freeze({ selectedTab: tab, selectedBrowser: browser }),
    gNavToolbox: Object.freeze({ collapsed: false }),
    document: Object.freeze({
      documentElement: Object.freeze({
        hasAttribute(name) {
          assert.equal(name, "window-modal-open");
          calls.push("modal-marker");
          return false;
        },
      }),
    }),
  };
  const context = {
    window,
    arguments: ["control"],
    ChromeUtils: Object.freeze({
      importESModule(name) {
        assert.equal(name, "chrome://remote/content/shared/NavigableManager.sys.mjs");
        calls.push("module");
        return {
          NavigableManager: {
            getIdForBrowser(value) {
              assert.equal(value, window.gBrowser.selectedBrowser);
              calls.push("original-selected-browser");
              return "control";
            },
          },
        };
      },
    }),
  };
  return {
    window,
    context,
    calls,
    invoke() {
      return JSON.parse(JSON.stringify(vm.runInNewContext(`(() => {${source}})()`, context)));
    },
  };
}

test("tab state returns only fixed flags from the original chrome window", () => {
  const f = fixture();
  assert.deepEqual(f.invoke(), {
    version: 1,
    window_modal: false,
    navigation_collapsed: false,
    selection_consistent: true,
    selected_control: true,
    selected_blank: true,
  });
  assert.deepEqual(f.calls, ["module", "original-selected-browser", "modal-marker"]);
});

test("modal and collapsed flags are observed without dismissing or selecting", () => {
  const f = fixture();
  f.window.gNavToolbox = Object.freeze({ collapsed: true });
  f.window.document = Object.freeze({
    documentElement: Object.freeze({
      hasAttribute(name) {
        assert.equal(name, "window-modal-open");
        return true;
      },
    }),
  });
  const result = f.invoke();
  assert.equal(result.window_modal, true);
  assert.equal(result.navigation_collapsed, true);
  assert.equal(result.selected_control, true);
});

test("absent selected-window facilities remain unknown, not negative evidence", () => {
  const f = fixture();
  delete f.window.gBrowser;
  delete f.window.document;
  f.window.gNavToolbox = { collapsed: 0 };
  assert.deepEqual(f.invoke(), {
    version: 1,
    window_modal: null,
    navigation_collapsed: null,
    selection_consistent: null,
    selected_control: null,
    selected_blank: null,
  });
  assert.deepEqual(f.calls, []);
});

test("different selection and opaque URL/identity never leave the snapshot", () => {
  const f = fixture();
  f.window.gBrowser = Object.freeze({
    selectedTab: Object.freeze({ linkedBrowser: {} }),
    selectedBrowser: Object.freeze({ currentURI: Object.freeze({ spec: "opaque-private-url" }) }),
  });
  f.context.ChromeUtils = {
    importESModule() {
      return { NavigableManager: { getIdForBrowser: () => "opaque-private-handle" } };
    },
  };
  const result = f.invoke();
  assert.equal(result.selection_consistent, false);
  assert.equal(result.selected_control, false);
  assert.equal(result.selected_blank, false);
  assert.equal(JSON.stringify(result).includes("opaque-private"), false);
  f.context.ChromeUtils.importESModule = () => ({
    NavigableManager: { getIdForBrowser: () => null },
  });
  assert.equal(f.invoke().selected_control, null);
});

test("invalid expected identity refuses before reading window state", () => {
  for (const value of [null, 1, true, "", "bad handle", "é", "a".repeat(129)]) {
    const f = fixture();
    f.context.arguments = [value];
    assert.throws(() => f.invoke(), /Owned tab observation refused/u);
    assert.deepEqual(f.calls, []);
  }
});

test("observation exceptions propagate without fallback or window enumeration", () => {
  const f = fixture();
  f.context.ChromeUtils = {
    importESModule() {
      throw new Error("unavailable-module");
    },
  };
  assert.throws(() => f.invoke(), /unavailable-module/u);
  assert.deepEqual(f.calls, []);
});
