import { beforeEach, expect, it, vi } from "vitest";
const mock = vi.hoisted(() => ({
  start: vi.fn(),
  allowed: vi.fn(),
  activate: vi.fn(),
  register: vi.fn(),
  state: vi.fn(),
  supports: vi.fn(),
}));
vi.mock("../src/background", () => ({
  browserHandoff: {},
  captureAccess: { start: mock.start, allowed: mock.allowed },
  captureControl: { activate: mock.activate },
  nativeConnection: { state: mock.state, supports: mock.supports },
}));
vi.mock("../src/capture-registration", () => ({ registerCapture: mock.register }));
beforeEach(() => {
  vi.resetModules();
  vi.clearAllMocks();
  mock.allowed.mockReturnValue(true);
  mock.state.mockReturnValue({ connected: true });
  mock.supports.mockReturnValue(true);
});
it("candidate activates without diagnostic arming, but requires every independent authorization gate", async () => {
  let preference = true;
  mock.activate.mockImplementation((register: (enabled: () => boolean) => void) =>
    register(() => preference),
  );
  await import("../src/automatic-background");
  expect(mock.start).toHaveBeenCalledTimes(1);
  expect(mock.register).toHaveBeenCalledTimes(1);
  const enabled = mock.register.mock.calls[0]![1] as () => boolean;
  expect(enabled()).toBe(true);
  preference = false;
  expect(enabled()).toBe(false);
  preference = true;
  mock.allowed.mockReturnValue(false);
  expect(enabled()).toBe(false);
  mock.allowed.mockReturnValue(true);
  mock.state.mockReturnValue({ connected: false });
  expect(enabled()).toBe(false);
  mock.state.mockReturnValue({ connected: true });
  for (const missing of ["prepared_handoff", "task_handoff_phase"]) {
    mock.supports.mockImplementation((name: string) => name !== missing);
    expect(enabled()).toBe(false);
  }
  expect(mock.register.mock.calls[0]![2]).toEqual(["http://*/*", "https://*/*"]);
  expect(mock.register.mock.calls[0]![3]).toEqual({ crossOriginRedirects: true });
});
