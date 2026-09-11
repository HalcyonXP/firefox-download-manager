import { beforeEach, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  connectListener: vi.fn(),
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
    connect = vi.fn(async () => {});
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
