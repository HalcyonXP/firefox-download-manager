import { afterEach, expect, it, vi } from "vitest";
import { createNativeConnection } from "../src/native-provider";

afterEach(() => vi.unstubAllGlobals());
it("ordinary manifest keeps the ordinary connector", async () => {
  vi.stubGlobal("__DM_PARENT_TRANSPORT__", false);
  const ordinary = vi.fn(() => {
    throw new Error("synthetic absent helper");
  });
  const parent = vi.fn(async () => 1);
  vi.stubGlobal("browser", {
    runtime: { getManifest: () => ({ version: "0.1.0" }), connectNative: ordinary },
    managerParentTransport: { open: parent },
  });
  const client = createNativeConnection();
  await expect(client.connect()).rejects.toThrow();
  expect(ordinary).toHaveBeenCalledOnce();
  expect(parent).not.toHaveBeenCalled();
});
it("selected but absent or failed parent API never downgrades to ordinary Native Messaging", async () => {
  vi.stubGlobal("__DM_PARENT_TRANSPORT__", true);
  for (const missing of [true, false]) {
    const ordinary = vi.fn();
    const open = vi.fn(async () => {
      throw new Error("synthetic parent refusal");
    });
    vi.stubGlobal("browser", {
      runtime: {
        getManifest: () => ({ version: "0.3.0", experiment_apis: { managerParentTransport: {} } }),
        connectNative: ordinary,
      },
      ...(missing ? {} : { managerParentTransport: { open } }),
    });
    const client = createNativeConnection();
    await expect(client.connect()).rejects.toThrow();
    expect(ordinary).not.toHaveBeenCalled();
    expect(open).toHaveBeenCalledTimes(missing ? 0 : 1);
  }
});
