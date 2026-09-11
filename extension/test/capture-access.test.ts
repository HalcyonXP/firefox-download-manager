import { expect, it, vi } from "vitest";
import { CaptureAccess, capturePermissions } from "../src/capture-access";
import { wireCaptureAccess } from "../src/capture-access-ui";

const flush = async (): Promise<void> => {
  await Promise.resolve();
  await Promise.resolve();
};
function setup() {
  let added = (): void => {};
  let removed = (): void => {};
  const contains = vi.fn<() => Promise<boolean>>().mockResolvedValue(true);
  const access = new CaptureAccess({
    contains,
    onAdded: (f) => {
      added = f;
    },
    onRemoved: (f) => {
      removed = f;
    },
  });
  return { access, contains, added: () => added(), removed: () => removed() };
}
it("requires explicit selection and verified host/API permission; never requests permission", async () => {
  const { access, contains } = setup();
  expect(access.allowed()).toBe(false);
  expect(contains).not.toHaveBeenCalled();
  access.start();
  expect(access.allowed()).toBe(false);
  await flush();
  expect(access.allowed()).toBe(true);
  expect(() => access.start()).toThrow();
  expect(capturePermissions()).toEqual({
    permissions: ["webRequest", "webRequestBlocking"],
    origins: ["http://*/*", "https://*/*"],
  });
});
it("immediately revokes on permission changes and rejects stale asynchronous grants", async () => {
  const { access, contains, removed, added } = setup();
  access.start();
  await flush();
  let old!: (value: boolean) => void;
  contains.mockReturnValueOnce(
    new Promise((resolve) => {
      old = resolve;
    }),
  );
  added();
  expect(access.allowed()).toBe(false);
  contains.mockResolvedValueOnce(false);
  removed();
  await flush();
  old(true);
  await flush();
  expect(access.allowed()).toBe(false);
  expect(access.state().granted).toBe(false);
  contains.mockResolvedValue(true);
  await access.recheck();
  expect(access.allowed()).toBe(true);
});
it("partial listener registration cannot recover into authority without revocation observation", async () => {
  let added = (): void => {};
  const contains = vi.fn().mockResolvedValue(true);
  const access = new CaptureAccess({
    contains,
    onAdded: (f) => {
      added = f;
    },
    onRemoved: () => {
      throw Error("fixture");
    },
  });
  access.start();
  added();
  await access.recheck();
  await flush();
  expect(access.state().failed).toBe(true);
  expect(access.allowed()).toBe(false);
  expect(contains).not.toHaveBeenCalled();
});
it("unknown or failed readback refuses capture without changing the saved preference", async () => {
  const { access, contains } = setup();
  contains.mockResolvedValueOnce(1 as unknown as boolean);
  access.start();
  await flush();
  expect(access.state().failed).toBe(true);
  expect(access.allowed()).toBe(false);
  contains.mockRejectedValueOnce(Error("fixture"));
  await access.recheck();
  expect(access.allowed()).toBe(false);
});
it("permission UI only prompts on its button, never infers authority or replays an uncertain request", async () => {
  let click = (): void => {};
  const button = {
    hidden: false,
    disabled: false,
    addEventListener: (_: string, f: () => void) => {
      click = f;
    },
  } as unknown as HTMLButtonElement;
  const status = { hidden: false, textContent: "" } as HTMLElement;
  let approve!: (value: boolean) => void;
  const request = vi.fn(
    () =>
      new Promise<boolean>((resolve) => {
        approve = resolve;
      }),
  );
  const check = vi.fn();
  const ui = wireCaptureAccess(button, status, request, check);
  click();
  expect(request).not.toHaveBeenCalled();
  ui.update({ selected: true, checking: false, granted: false, failed: false });
  expect(status.textContent).toContain("even if");
  click();
  click();
  expect(request).toHaveBeenCalledTimes(1);
  approve(true);
  await flush();
  expect(check).toHaveBeenCalledTimes(1);
  expect(status.textContent).toContain("Checking");
  ui.update({ selected: true, checking: false, granted: true, failed: false });
  expect(button.disabled).toBe(true);
  ui.disconnect();
  expect(status.textContent).toContain("connection lost");
  click();
  expect(request).toHaveBeenCalledTimes(1);
});
