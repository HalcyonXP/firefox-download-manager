import { beforeEach, expect, it, vi } from "vitest";
import { registerCapture } from "../src/capture-registration";

const mocks = vi.hoisted(() => ({
  preference: vi.fn(),
  connect: vi.fn(),
  command: vi.fn(),
  capture: vi.fn(),
  terminal: vi.fn(),
  recover: vi.fn(),
  supports: vi.fn(),
  state: vi.fn(),
  view: vi.fn(),
  listen: vi.fn(),
}));
vi.mock("../src/background", () => ({
  captureControl: {
    state: () => ({ available: true, ready: true, enabled: true, busy: false, failed: false }),
    ready: async () => {},
    activate: (register: (enabled: () => boolean) => void) => register(mocks.preference),
  },
  nativeConnection: {
    connect: mocks.connect,
    command: mocks.command,
    supports: mocks.supports,
    state: mocks.state,
  },
  browserHandoff: {
    capture: mocks.capture,
    terminal: mocks.terminal,
    recover: mocks.recover,
    view: mocks.view,
  },
}));
vi.mock("../src/capture-registration", () => ({ registerCapture: vi.fn() }));
const inspector = { id: "owned", url: "moz-extension://owned/inspect.html" };
let control: (message: unknown, sender: unknown) => unknown;
beforeEach(async () => {
  vi.resetModules();
  vi.clearAllMocks();
  mocks.state.mockReturnValue({
    connected: true,
    tasks: [],
    settings: { destination: "owned-output" },
  });
  mocks.view.mockReturnValue({ loaded: true, blocked: false, pending: [] });
  mocks.supports.mockReturnValue(true);
  mocks.preference.mockReturnValue(true);
  mocks.capture.mockResolvedValue({ cancel: true });
  vi.stubGlobal("browser", {
    runtime: {
      id: "owned",
      getURL: (name: string) => `moz-extension://owned/${name}`,
      onMessage: { addListener: mocks.listen },
    },
  });
  (await import("../src/background")).nativeConnection.command = mocks.command;
  await import("../diagnostic/handoff");
  control = mocks.listen.mock.calls[0]![0] as typeof control;
});

it("requires exact inspector and independently matched destination before arming", async () => {
  expect(await control({ action: "arm" }, inspector)).toBeNull();
  expect(
    control(
      { action: "ready", destination: "owned-output", origins: ["http://127.0.0.1"] },
      { ...inspector, url: "http://127.0.0.1/page" },
    ),
  ).toBeUndefined();
  expect(control({ action: "arm", extra: true }, inspector)).toBeUndefined();
  expect(control({ action: "commit_handoff" }, inspector)).toBeUndefined();
  expect(
    await control(
      { action: "ready", destination: "other", origins: ["http://127.0.0.1"] },
      inspector,
    ),
  ).toMatchObject({
    destinationVerified: false,
  });
  expect(await control({ action: "arm" }, inspector)).toBeNull();
  expect(
    await control(
      { action: "ready", destination: "owned-output", origins: ["http://127.0.0.1"] },
      inspector,
    ),
  ).toMatchObject({
    destinationVerified: true,
  });
  expect(await control({ action: "arm" }, inspector)).toMatchObject({ enabled: true });
  expect(await control({ action: "off" }, inspector)).toMatchObject({ enabled: false });
  expect(mocks.command.mock.calls.every(([name]) => name === "get_settings")).toBe(true);
});

it("bounds authority to loopback and reports only correlated decisions/terminal classes", async () => {
  const [handoff, enabled, urls] = vi.mocked(registerCapture).mock.calls[0]!;
  expect(enabled()).toBe(false);
  expect(urls).toEqual(["http://127.0.0.1/*"]);
  expect(
    await handoff.capture(
      "foreign",
      { url: "https://example.invalid/", suggested_filename: "unused" },
      () => true,
    ),
  ).toEqual({});
  expect(mocks.capture).not.toHaveBeenCalled();
  await control(
    { action: "ready", destination: "owned-output", origins: ["http://127.0.0.1"] },
    inspector,
  );
  await control({ action: "arm" }, inspector);
  expect(
    await handoff.capture(
      "other-port",
      { url: "http://127.0.0.1:9999/direct", suggested_filename: "unused" },
      () => true,
    ),
  ).toEqual({});
  expect(mocks.capture).not.toHaveBeenCalled();
  const eligible = () => true;
  await handoff.capture(
    "owned-request",
    { url: "http://127.0.0.1/direct", suggested_filename: "owned.bin" },
    eligible,
  );
  expect(mocks.capture).toHaveBeenCalledWith("owned-request", expect.anything(), eligible);
  handoff.terminal("other-request", "NS_ERROR_ABORT");
  handoff.terminal("owned-request", "NS_ERROR_ABORT");
  const result = await control({ action: "snapshot" }, inspector);
  expect(result).toMatchObject({
    qualification: false,
    records: [
      { request: 1, stage: "decision", cancelled: true },
      { request: 1, stage: "terminal", cancelled: true },
    ],
  });
  expect(JSON.stringify(result)).not.toMatch(/owned-request|127\.0\.0\.1|owned\.bin|owned-output/u);
});

it("withholds only the selected terminal observation, never manufacture a cancellation", async () => {
  await control(
    { action: "ready", destination: "owned-output", origins: ["http://127.0.0.1"] },
    inspector,
  );
  await control({ action: "arm-missing-terminal" }, inspector);
  const [handoff] = vi.mocked(registerCapture).mock.calls[0]!;
  await handoff.capture(
    "request",
    { url: "http://127.0.0.1/direct", suggested_filename: "owned.bin" },
    () => true,
  );
  handoff.terminal("request", "NS_ERROR_ABORT");
  expect(mocks.terminal).not.toHaveBeenCalled();
  expect(await control({ action: "snapshot" }, inspector)).toMatchObject({
    terminalSuppressed: true,
  });
  handoff.terminal("other");
  expect(mocks.terminal).toHaveBeenCalledWith("other", undefined);
});

it("bounds independent fixture origins and revokes arming during reconfiguration", async () => {
  const [handoff, enabled, , options] = vi.mocked(registerCapture).mock.calls[0]!;
  const origins = ["http://127.0.0.1:39001", "http://127.0.0.1:39002"];
  expect(
    await control({ action: "ready", destination: "owned-output", origins }, inspector),
  ).toMatchObject({ destinationVerified: true });
  expect(options?.crossOriginRedirects).toBe(true);
  expect(origins.every((origin) => options?.originAllowed?.(origin))).toBe(true);
  expect(options?.originAllowed?.("http://127.0.0.1:39003")).toBe(false);
  await control({ action: "arm" }, inspector);
  await handoff.capture(
    "cdn",
    { url: origins[1] + "/attachment", suggested_filename: "owned.bin" },
    () => true,
  );
  expect(mocks.capture).toHaveBeenCalledTimes(1);
  for (const refused of [
    [],
    [...origins, "http://127.0.0.1:39003"],
    [origins[0], origins[0]],
    ["https://example.invalid"],
    ["http://127.0.0.1/path"],
    [origins[0], 1],
  ]) {
    expect(
      await control({ action: "ready", destination: "owned-output", origins: refused }, inspector),
    ).toMatchObject({ destinationVerified: false });
    expect(enabled()).toBe(false);
    expect(origins.some((origin) => options?.originAllowed?.(origin))).toBe(false);
    expect(await control({ action: "arm" }, inspector)).toBeNull();
  }
});

it("only substitutes a real abort command in the explicitly armed terminal fault", async () => {
  await control(
    { action: "ready", destination: "owned-output", origins: ["http://127.0.0.1"] },
    inspector,
  );
  await control({ action: "arm-aborted-terminal" }, inspector);
  const { nativeConnection } = await import("../src/background");
  const payload = { task_id: "b4ac080c-862f-4ea8-b60c-06a9718b2306" };
  await nativeConnection.command("commit_handoff", payload);
  expect(mocks.command).toHaveBeenLastCalledWith("abort_handoff", payload);
  expect(await control({ action: "snapshot" }, inspector)).toMatchObject({ commitReplaced: true });
  await nativeConnection.command("get_handoff", payload);
  expect(mocks.command).toHaveBeenLastCalledWith("get_handoff", payload);
});
it("seeds only one unlinked reservation at the verified exact fixture destination/origin", async () => {
  expect(await control({ action: "seed-unlinked" }, inspector)).toBeNull();
  await control(
    { action: "ready", destination: "owned-output", origins: ["http://127.0.0.1:39001"] },
    inspector,
  );
  await control({ action: "seed-unlinked" }, inspector);
  expect(mocks.command).toHaveBeenLastCalledWith("prepare_handoff", {
    task_id: expect.any(String),
    download: { url: "http://127.0.0.1:39001/direct", suggested_filename: "owned-capture.bin" },
  });
  mocks.state.mockReturnValue({ tasks: [{}], settings: { destination: "owned-output" } });
  expect(await control({ action: "seed-unlinked" }, inspector)).toBeNull();
});

it("does not replace an in-flight or uncertain seed with another ID", async () => {
  await control(
    { action: "ready", destination: "owned-output", origins: ["http://127.0.0.1"] },
    inspector,
  );
  let release!: () => void;
  mocks.command.mockImplementationOnce(
    () =>
      new Promise<void>((done) => {
        release = done;
      }),
  );
  const first = control({ action: "seed-unlinked" }, inspector);
  expect(await control({ action: "seed-unlinked" }, inspector)).toBeNull();
  release();
  await first;
  expect(await control({ action: "seed-unlinked" }, inspector)).toBeNull();
  expect(
    mocks.command.mock.calls.filter(([command]) => command === "prepare_handoff"),
  ).toHaveLength(1);
});

it("applies the shared saved preference even when the diagnostic gate is armed", async () => {
  await control(
    { action: "ready", destination: "owned-output", origins: ["http://127.0.0.1"] },
    inspector,
  );
  await control({ action: "arm" }, inspector);
  const enabled = vi.mocked(registerCapture).mock.calls[0]![1];
  expect(enabled()).toBe(true);
  mocks.preference.mockReturnValue(false);
  expect(enabled()).toBe(false);
});
