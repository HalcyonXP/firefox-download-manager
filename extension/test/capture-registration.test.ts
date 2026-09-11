import { afterEach, expect, it, vi } from "vitest";
import { registerCapture } from "../src/capture-registration";
import type { BrowserHandoff } from "../src/browser-handoff";
const url = "https://example.invalid/file.bin";
const page = "https://example.invalid/page";
const request = {
  requestId: "r",
  url,
  method: "GET",
  type: "main_frame",
  frameId: 0,
  tabId: 1,
  incognito: false,
  cookieStoreId: "firefox-default",
  originUrl: page,
};
const response = {
  ...request,
  statusCode: 200,
  responseHeaders: [{ name: "Content-Disposition", value: "attachment" }],
};
const message = { action: "ordinary-download-click", target: url, trusted: true };
const sender = { id: "owned-extension", frameId: 0, tab: { id: 1, incognito: false }, url: page };
afterEach(() => vi.unstubAllGlobals());
function setup() {
  const callbacks = new Map<string, (...args: unknown[]) => unknown>();
  const registrations = new Map<string, unknown[]>();
  const event = (name: string) => ({
    addListener: (callback: (...args: unknown[]) => unknown, ...args: unknown[]) => {
      callbacks.set(name, callback);
      registrations.set(name, args);
    },
  });
  const names = [
    "onBeforeRequest",
    "onBeforeSendHeaders",
    "onBeforeRedirect",
    "onHeadersReceived",
    "onCompleted",
    "onErrorOccurred",
  ];
  vi.stubGlobal("browser", {
    runtime: { id: "owned-extension", onMessage: event("message") },
    webRequest: Object.fromEntries(names.map((name) => [name, event(name)])),
  });
  const capture = vi.fn<BrowserHandoff["capture"]>(async (_key, _download, eligible) =>
    eligible() ? { cancel: true } : {},
  );
  const terminal = vi.fn<BrowserHandoff["terminal"]>();
  registerCapture({ capture, terminal }, () => true);
  const emit = (name: string, ...args: unknown[]) => callbacks.get(name)!(...args);
  return { emit, capture, terminal, registrations };
}
it("registers read-only sent-header observation and asynchronous response cancellation", async () => {
  const { emit, capture, terminal, registrations } = setup();
  expect(registrations.get("onBeforeSendHeaders")?.[1]).toEqual(["requestHeaders"]);
  expect(registrations.get("onHeadersReceived")?.[1]).toEqual(["blocking", "responseHeaders"]);
  emit("message", message, sender);
  emit("onBeforeRequest", request);
  emit("onBeforeSendHeaders", { ...request, requestHeaders: [] });
  await expect(emit("onHeadersReceived", response)).resolves.toEqual({ cancel: true });
  expect(capture).toHaveBeenCalledTimes(1);
  emit("onErrorOccurred", { ...request, error: "NS_ERROR_ABORT" });
  expect(terminal).toHaveBeenCalledWith("r", "NS_ERROR_ABORT");
});
it.each([
  { id: "other-extension" },
  { frameId: 1 },
  { tab: { id: 1, incognito: true } },
  { tab: { incognito: false } },
])("rejects unowned or unsupported message sender %#", async (patch) => {
  const { emit, capture } = setup();
  emit("message", message, { ...sender, ...patch });
  emit("onBeforeRequest", request);
  emit("onBeforeSendHeaders", { ...request, requestHeaders: [] });
  await expect(emit("onHeadersReceived", response)).resolves.toEqual({});
  expect(capture).not.toHaveBeenCalled();
});
it("ignores messages with unknown fields and leaves missing sent-header evidence untouched", async () => {
  const { emit, capture } = setup();
  emit("message", { ...message, extra: true }, sender);
  emit("onBeforeRequest", request);
  await expect(emit("onHeadersReceived", response)).resolves.toEqual({});
  expect(capture).not.toHaveBeenCalled();
});
