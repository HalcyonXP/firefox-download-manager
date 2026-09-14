import { expect, it, vi } from "vitest";
import { CaptureControl, type CaptureStorage } from "../src/capture-control";
import { renderCaptureControl } from "../src/capture-ui";
class Storage implements CaptureStorage {
  value: unknown;
  pause: Promise<void> | undefined;
  readback: unknown = "normal";
  writes: unknown[] = [];
  async read(): Promise<unknown> {
    return this.readback === "normal" ? structuredClone(this.value) : this.readback;
  }
  async write(value: unknown): Promise<void> {
    if (this.pause) await this.pause;
    this.value = structuredClone(value);
    this.writes.push(this.value);
  }
}
function setup() {
  const storage = new Storage();
  const control = new CaptureControl(storage);
  let allowed = () => false;
  control.activate((enabled) => {
    allowed = enabled;
  });
  return { storage, control, allowed: () => allowed() };
}
it("defaults on only after verified preference and explicit listener activation", async () => {
  const storage = new Storage();
  const control = new CaptureControl(storage);
  expect(control.effective()).toBe(false);
  await control.ready();
  expect(control.state().enabled).toBe(true);
  expect(control.effective()).toBe(false);
  let check = () => true;
  control.activate((enabled) => {
    check = enabled;
    expect(enabled()).toBe(false);
  });
  expect(check()).toBe(true);
  expect(() => control.activate(() => {})).toThrow();
  expect(storage.writes).toEqual([]);
});
it("revokes immediately, persists Off and independently restores it after restart", async () => {
  const { control, storage, allowed } = setup();
  await control.ready();
  expect(allowed()).toBe(true);
  const off = control.setEnabled(false);
  expect(allowed()).toBe(false);
  expect(control.state().busy).toBe(true);
  await off;
  expect(storage.value).toEqual({ version: 1, enabled: false });
  const restarted = new CaptureControl(storage);
  restarted.activate(() => {});
  await restarted.ready();
  expect(restarted.effective()).toBe(false);
  await restarted.setEnabled(true);
  expect(restarted.effective()).toBe(true);
});
it("serializes rapid On/Off without stale authorization while writes are pending", async () => {
  const { control, storage, allowed } = setup();
  await control.ready();
  let release!: () => void;
  storage.pause = new Promise<void>((done) => {
    release = done;
  });
  const on = control.setEnabled(true);
  expect(allowed()).toBe(false);
  const off = control.setEnabled(false);
  expect(allowed()).toBe(false);
  release();
  await on;
  expect(allowed()).toBe(false);
  await off;
  expect(storage.writes).toEqual([
    { version: 1, enabled: true },
    { version: 1, enabled: false },
  ]);
  expect(allowed()).toBe(false);
});
it("does not let the initial asynchronous read undo a requested Off", async () => {
  let release!: (value: unknown) => void;
  const storage = new Storage();
  vi.spyOn(storage, "read").mockImplementationOnce(
    () =>
      new Promise((done) => {
        release = done;
      }),
  );
  const control = new CaptureControl(storage);
  control.activate(() => {});
  const ready = control.ready();
  const off = control.setEnabled(false);
  release(undefined);
  await ready;
  expect(control.effective()).toBe(false);
  await off;
  expect(control.effective()).toBe(false);
});
it.each([
  null,
  true,
  {},
  { version: 2, enabled: true },
  { version: 1, enabled: 1 },
  { version: 1, enabled: true, extra: true },
])("refuses corrupt/unknown saved preferences %#", async (value) => {
  const { storage, control, allowed } = setup();
  storage.value = value;
  await expect(control.ready()).rejects.toThrow();
  expect(allowed()).toBe(false);
  await expect(control.setEnabled(true)).rejects.toThrow();
  expect(storage.writes).toEqual([]);
});
it.each([undefined, { version: 1, enabled: false }])(
  "requires independent readback before On %#",
  async (value) => {
    const { storage, control, allowed } = setup();
    await control.ready();
    storage.readback = value;
    await expect(control.setEnabled(true)).rejects.toThrow();
    expect(allowed()).toBe(false);
    expect(control.state().failed).toBe(true);
  },
);
it("failed partial registration remains unavailable and cannot be blindly repeated", async () => {
  const control = new CaptureControl(new Storage());
  await control.ready();
  let retained = () => true;
  expect(() =>
    control.activate((enabled) => {
      retained = enabled;
      throw new Error("synthetic registration failure");
    }),
  ).toThrow();
  expect(retained()).toBe(false);
  expect(() => control.activate(() => {})).toThrow();
  await expect(control.setEnabled(true)).rejects.toThrow();
});
it("distinguishes unavailable, verifying, failed and effective Off in the actual renderer", async () => {
  const { control } = setup();
  const input = { checked: true, disabled: false, indeterminate: true } as HTMLInputElement;
  const status = { textContent: "" } as HTMLElement;
  renderCaptureControl(input, status, { ...control.state(), available: false });
  expect(input.checked).toBe(false);
  expect(input.disabled).toBe(true);
  expect(status.textContent).toContain("unavailable");
  renderCaptureControl(input, status, control.state());
  expect(status.textContent).toContain("Verifying");
  await control.ready();
  await control.setEnabled(false);
  renderCaptureControl(input, status, control.state());
  expect(input.checked).toBe(false);
  expect(input.disabled).toBe(false);
  expect(status.textContent).toContain("Existing Manager transfers are unchanged");
  renderCaptureControl(input, status, { ...control.state(), failed: true });
  expect(input.disabled).toBe(true);
  expect(status.textContent).toContain("paused");
});
