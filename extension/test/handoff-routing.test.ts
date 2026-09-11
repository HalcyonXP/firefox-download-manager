import { beforeEach, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  connectListener: vi.fn(),
  connect: vi.fn(async () => {}),
  command: vi.fn(),
  acknowledge: vi.fn(),
  discard: vi.fn(),
  recover: vi.fn(async () => {}),
  resolve: vi.fn(),
}));
vi.mock("../src/native-connection", () => ({
  NativeConnection: class {
    subscribe() {
      return () => {};
    }
    command = mocks.command;
    connect = mocks.connect;
  },
}));
vi.mock("../src/browser-handoff", () => ({
  BrowserHandoff: class {
    subscribe() {
      return () => {};
    }
    recover = mocks.recover;
    acknowledgeAborted = mocks.acknowledge;
    discardUnlinked = mocks.discard;
    resolveIntent = mocks.resolve;
  },
}));
const id = "b4ac080c-862f-4ea8-b60c-06a9718b2306";
beforeEach(async () => {
  vi.resetModules();
  vi.clearAllMocks();
  const event = () => ({ addListener: vi.fn() });
  vi.stubGlobal("browser", {
    storage: { local: { get: vi.fn(async () => ({})), set: vi.fn(async () => {}) } },
    runtime: {
      id: "owned",
      getURL: (name: string) => `moz-extension://owned/${name}`,
      onConnect: { addListener: mocks.connectListener },
      onInstalled: event(),
    },
    action: { onClicked: event() },
    menus: { onClicked: event() },
  });
  await import("../src/background");
});
function port(url = "moz-extension://owned/manager.html") {
  const value = {
    name: "manager-ui",
    sender: { id: "owned", url },
    disconnect: vi.fn(),
    postMessage: vi.fn(),
    onMessage: { addListener: vi.fn() },
    onDisconnect: { addListener: vi.fn() },
  };
  (mocks.connectListener.mock.calls[0]![0] as (value: unknown) => void)(value);
  return value;
}
it.each(["acknowledge", "discard"])(
  "routes %s only to the coordinator, never a generic transfer command",
  async (choice) => {
    const peer = port();
    (peer.onMessage.addListener.mock.calls[0]![0] as (message: unknown) => void)({
      action: "handoff-cleanup",
      taskId: id,
      choice,
    });
    await vi.waitFor(() => expect(peer.postMessage).toHaveBeenCalledWith({ kind: "idle" }));
    expect(choice === "acknowledge" ? mocks.acknowledge : mocks.discard).toHaveBeenCalledWith(id);
    expect(mocks.command).not.toHaveBeenCalled();
  },
);
it("refuses unowned senders and unknown cleanup members/choices", async () => {
  expect(port("https://example.invalid/").disconnect).toHaveBeenCalled();
  for (const patch of [{ extra: true }, { choice: "manager" }, { taskId: 1 }]) {
    const peer = port();
    (peer.onMessage.addListener.mock.calls[0]![0] as (message: unknown) => void)({
      action: "handoff-cleanup",
      taskId: id,
      choice: "discard",
      ...patch,
    });
    await vi.waitFor(() => expect(peer.postMessage).toHaveBeenCalledWith({ kind: "idle" }));
  }
  expect(mocks.discard).not.toHaveBeenCalled();
  expect(mocks.acknowledge).not.toHaveBeenCalled();
  expect(mocks.command).not.toHaveBeenCalled();
});

it("routes verified capture preferences separately from native commands and never writes handoff history", async () => {
  const { captureControl } = await import("../src/background");
  captureControl.activate(() => {});
  await captureControl.ready();
  let stored: unknown;
  vi.mocked(browser.storage.local.get).mockImplementation(async () => ({
    "automatic-capture-v1": stored,
  }));
  vi.mocked(browser.storage.local.set).mockImplementation(async (values) => {
    stored = values["automatic-capture-v1"] as unknown;
  });
  const peer = port();
  const send = peer.onMessage.addListener.mock.calls[0]![0] as (message: unknown) => void;
  let release!: () => void;
  mocks.connect.mockImplementationOnce(
    () =>
      new Promise<void>((done) => {
        release = done;
      }),
  );
  send({ action: "connect" }); // Deliberately held native action owns the ordinary port queue.
  send({ action: "capture-setting", enabled: false });
  expect(captureControl.effective()).toBe(false);
  await vi.waitFor(() => expect(captureControl.state().busy).toBe(false));
  expect(stored).toEqual({ version: 1, enabled: false });
  expect(browser.storage.local.set).toHaveBeenCalledExactlyOnceWith({
    "automatic-capture-v1": { version: 1, enabled: false },
  });
  expect(mocks.command).not.toHaveBeenCalled();
  release();
  await vi.waitFor(() => expect(peer.postMessage).toHaveBeenCalledWith({ kind: "idle" }));
  send({ action: "capture-setting", enabled: true, extra: true });
  await vi.waitFor(() => expect(peer.postMessage).toHaveBeenCalledWith({ kind: "idle" }));
  expect(browser.storage.local.set).toHaveBeenCalledTimes(1);
  expect(mocks.command).toHaveBeenCalledExactlyOnceWith("get_settings", {});
});
