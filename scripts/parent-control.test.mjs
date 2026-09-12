import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

const source = readFileSync(new URL("qualification/parent_control.js", import.meta.url), "utf8");
function fixture() {
  const state = { disables: 0, lookup: 0, channel: "aurora" };
  const addon = {
    id: "download-manager@halcyonxp.local",
    version: "0.0.1",
    type: "extension",
    temporarilyInstalled: true,
    isActive: true,
    userDisabled: false,
    async disable() {
      state.disables++;
      this.isActive = false;
      this.userDisabled = true;
    },
  };
  const extension = {
    id: addon.id,
    hasShutdown: false,
    privateBrowsingAllowed: false,
    persistentBackground: false,
    hasPermission(name) {
      assert.equal(name, "nativeMessaging");
      return true;
    },
  };
  async function invoke(operation, extra = []) {
    return new Promise((resolve) => {
      const context = vm.createContext({
        arguments: [operation, ...extra, resolve],
        ChromeUtils: {
          importESModule(name) {
            if (name === "resource://gre/modules/AddonManager.sys.mjs")
              return {
                AddonManager: {
                  async getAddonByID(id) {
                    assert.equal(id, addon.id);
                    state.lookup++;
                    return state.absent ? null : addon;
                  },
                },
              };
            if (name === "resource://gre/modules/ExtensionParent.sys.mjs")
              return {
                ExtensionParent: {
                  GlobalManager: {
                    getExtension(id) {
                      assert.equal(id, addon.id);
                      return extension;
                    },
                  },
                },
              };
            assert.equal(name, "resource://gre/modules/AppConstants.sys.mjs");
            return { AppConstants: { MOZ_UPDATE_CHANNEL: state.channel } };
          },
        },
      });
      new vm.Script(source).runInContext(context, { timeout: 1000 });
    });
  }
  return { state, addon, extension, invoke };
}

test("fixed temporary identity, metadata and explicit disable are distinct", async () => {
  const f = fixture();
  const value = await f.invoke("info");
  assert.equal(value.state, "active");
  assert.equal(value.temporary, true);
  assert.equal(f.state.disables, 0);
  assert.equal((await f.invoke("disable")).state, "disabled");
  assert.equal(f.state.disables, 1);
  assert.equal(await f.invoke("info"), null);
});

test("closed operations never accept extra arguments", async () => {
  const f = fixture();
  for (const name of ["enable", "uninstall", true, null]) assert.equal(await f.invoke(name), null);
  assert.equal(await f.invoke("disable", ["foreign"]), null);
  assert.equal(f.state.lookup, 0);
});

test("absence is a cleanup observation, never active or disabled proof", async () => {
  const f = fixture();
  f.state.absent = true;
  assert.equal(await f.invoke("info"), null);
  assert.equal((await f.invoke("disable")).state, "absent");
  assert.equal(f.state.disables, 0);
});

test("non-temporary and wrong version/type refuse before disable", async () => {
  for (const [name, value] of [
    ["temporarilyInstalled", false],
    ["temporarilyInstalled", 1],
    ["version", "0.2.0"],
    ["type", "theme"],
  ]) {
    const f = fixture();
    f.addon[name] = value;
    assert.equal(await f.invoke("info"), null);
    assert.equal(await f.invoke("disable"), null);
    assert.equal(f.state.disables, 0);
  }
});

test("parent scheduling and authority metadata require exact values", async () => {
  for (const [name, value] of [
    ["hasShutdown", true],
    ["hasShutdown", 0],
    ["privateBrowsingAllowed", true],
    ["privateBrowsingAllowed", 0],
    ["persistentBackground", true],
    ["persistentBackground", 0],
    ["hasPermission", () => false],
    ["id", "foreign"],
  ]) {
    const f = fixture();
    f.extension[name] = value;
    assert.equal(await f.invoke("info"), null);
  }
  const f = fixture();
  f.state.channel = "release";
  assert.equal(await f.invoke("info"), null);
});

test("disabled result requires actual post-action state and rejects failure", async () => {
  for (const disable of [
    async () => {},
    async () => {
      throw Error("private modeled error");
    },
  ]) {
    const f = fixture();
    f.addon.disable = disable;
    assert.equal(await f.invoke("disable"), null);
  }
});

test("disable completion is awaited before observing state", async () => {
  const f = fixture();
  let release;
  let returned = false;
  f.addon.disable = async () => {
    await new Promise((resolve) => {
      release = resolve;
    });
    f.addon.isActive = false;
    f.addon.userDisabled = true;
  };
  const pending = f.invoke("disable").then((value) => {
    returned = true;
    return value;
  });
  try {
    await new Promise(setImmediate);
    assert.equal(typeof release, "function");
    assert.equal(returned, false);
  } finally {
    release?.();
  }
  assert.equal((await pending).state, "disabled");
});

test("preload absence check never treats an installed ID as absent", async () => {
  const f = fixture();
  assert.equal(await f.invoke("absent"), null);
  f.addon.isActive = false;
  assert.equal(await f.invoke("absent"), null);
  f.addon.temporarilyInstalled = false;
  assert.equal(await f.invoke("absent"), null);
  f.state.absent = true;
  assert.equal((await f.invoke("absent")).state, "absent");
  assert.equal(f.state.disables, 0);
});
