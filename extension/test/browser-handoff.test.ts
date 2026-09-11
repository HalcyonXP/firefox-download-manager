import { afterEach, describe, expect, it, vi } from "vitest";
import { BrowserHandoff, type HandoffPeer } from "../src/browser-handoff";
import { HandoffJournal, type HandoffStage, type HandoffStorage } from "../src/handoff-journal";
import type { HandoffCommand, NativeHandoff, NativeTask } from "../src/native-connection";

const id = "a4ac080c-862f-4ea8-b60c-06a9718b2306";
const download = {
  url: "https://example.invalid/file?opaque=fixture",
  suggested_filename: "file.bin",
};
class Storage implements HandoffStorage {
  value: unknown;
  fail = false;
  readonly writes: unknown[] = [];
  async read(): Promise<unknown> {
    return structuredClone(this.value);
  }
  async write(value: unknown): Promise<void> {
    if (this.fail) throw new Error("synthetic storage failure");
    this.value = structuredClone(value);
    this.writes.push(structuredClone(value));
  }
}
function receipt(phase: NativeHandoff["phase"], taskId = id): NativeHandoff {
  const task: NativeTask = {
    handoff_phase: phase,
    task_id: taskId,
    display_name: "file.bin",
    destination: "C:\\Downloads",
    source_origin: "https://example.invalid",
    state: phase === "prepared" ? "queued" : phase === "aborted" ? "cancelled" : "probing",
    transfer_mode: "pending",
    expected_size: null,
    bytes_completed: 0,
    workers: 4,
    speed_bytes_per_second: null,
    eta_seconds: null,
    created_at: "2026-09-11T00:00:00.000Z",
    updated_at: "2026-09-11T00:00:00.000Z",
    error: null,
  };
  return { phase, task };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
class Peer implements HandoffPeer {
  async ready(): Promise<void> {}
  phase: NativeHandoff["phase"] = "prepared";
  readonly calls: HandoffCommand[] = [];
  transfers = 0;
  delay: Promise<NativeHandoff> | undefined;
  fail: HandoffCommand | undefined;
  observe: ((command: HandoffCommand) => void) | undefined;
  async command(command: HandoffCommand, payload: unknown): Promise<NativeHandoff> {
    expect(payload).toMatchObject({ task_id: id });
    this.calls.push(command);
    this.observe?.(command);
    if (command === "prepare_handoff" && this.delay) return this.delay;
    if (command === "commit_handoff" && this.phase === "prepared") {
      this.phase = "committed";
      this.transfers++;
    }
    if (command === this.fail) throw new Error("synthetic uncertain reply");
    if (command === "abort_handoff") {
      if (this.phase === "committed") throw new Error("already committed");
      this.phase = "aborted";
    }
    return receipt(this.phase);
  }
}
function setup(stage?: HandoffStage) {
  const storage = new Storage();
  if (stage) storage.value = { version: 1, pending: [{ id, stage, createdAt: 1 }] };
  const journal = new HandoffJournal(storage);
  const peer = new Peer();
  const controller = new BrowserHandoff(journal, peer, {
    uuid: () => id,
    deadlineMs: 100,
    terminalMs: 100,
  });
  return { storage, journal, peer, controller };
}
afterEach(() => vi.useRealTimers());

describe("browser cancellation/commit boundary", () => {
  it("persists intent before cancellation and observed cancellation before one commit", async () => {
    const { storage, peer, controller } = setup();
    peer.observe = (command) => {
      if (command === "prepare_handoff")
        expect(storage.value).toMatchObject({ pending: [{ stage: "preparing" }] });
      if (command === "commit_handoff")
        expect(storage.value).toMatchObject({ pending: [{ stage: "cancelled" }] });
    };
    await expect(controller.capture("request", download, () => true)).resolves.toEqual({
      cancel: true,
    });
    expect(storage.value).toMatchObject({ pending: [{ stage: "intent" }] });
    expect(peer.transfers).toBe(0);
    controller.terminal("unrelated", "NS_ERROR_ABORT");
    expect(peer.transfers).toBe(0);
    controller.terminal("request", "NS_ERROR_ABORT");
    controller.terminal("request", "NS_ERROR_ABORT");
    await controller.drain();
    expect(peer.transfers).toBe(1);
    expect(controller.view().pending).toEqual([]);
    expect(JSON.stringify(storage.writes)).not.toContain("opaque");
    expect(JSON.stringify(storage.writes)).not.toContain("https:");
  });
  it.each([undefined, "NS_BINDING_ABORTED", "NS_ERROR_BLOCKED_BY_POLICY"])(
    "does not infer cancellation from %s",
    async (error) => {
      const { peer, controller } = setup();
      await controller.capture("request", download, () => true);
      controller.terminal("request", error);
      await controller.drain();
      expect(peer.calls).toEqual(["prepare_handoff"]);
      expect(controller.view().pending[0]?.stage).toBe("intent");
      await controller.recover();
      expect(peer.calls).toEqual(["prepare_handoff", "get_handoff"]);
    },
  );
  it("leaves a missing terminal event uncertain rather than committing after its deadline", async () => {
    vi.useFakeTimers();
    const { controller, peer } = setup();
    await controller.capture("request", download, () => true);
    await vi.advanceTimersByTimeAsync(100);
    controller.terminal("request", "NS_ERROR_ABORT");
    await controller.drain();
    expect(peer.transfers).toBe(0);
    expect(controller.view().pending[0]?.stage).toBe("intent");
  });
  it("late preparation cannot cancel Firefox after the bounded decision", async () => {
    vi.useFakeTimers();
    const { peer, controller } = setup();
    const delayed = deferred<NativeHandoff>();
    peer.delay = delayed.promise;
    const decision = controller.capture("request", download, () => true);
    await vi.advanceTimersByTimeAsync(100);
    await expect(decision).resolves.toEqual({});
    delayed.resolve(receipt("prepared"));
    await controller.drain();
    expect(peer.calls).toEqual(["prepare_handoff", "abort_handoff"]);
    expect(peer.transfers).toBe(0);
    expect(controller.view().pending).toEqual([]);
  });
  it("rechecks eligibility after the native wait", async () => {
    const { peer, controller } = setup();
    let eligible = true;
    peer.observe = (command) => {
      if (command === "prepare_handoff") eligible = false;
    };
    await expect(controller.capture("request", download, () => eligible)).resolves.toEqual({});
    await controller.drain();
    expect(peer.calls).toEqual(["prepare_handoff", "abort_handoff"]);
  });
  it("refuses a reused ID without abandoning the older uncertain request", async () => {
    const { peer, controller } = setup("intent");
    await expect(controller.capture("request", download, () => true)).resolves.toEqual({});
    await controller.drain();
    expect(peer.calls).toEqual([]);
    expect(controller.view().pending[0]?.stage).toBe("intent");
  });
  it("failed intent persistence leaves Firefox untouched", async () => {
    const { storage, peer, controller } = setup();
    peer.observe = (command) => {
      if (command === "prepare_handoff") storage.fail = true;
    };
    await expect(controller.capture("request", download, () => true)).resolves.toEqual({});
    await controller.drain();
    expect(peer.transfers).toBe(0);
    expect(controller.view().blocked).toBe(true);
  });
  it("failed cancellation persistence cannot authorize commit", async () => {
    const { storage, peer, controller } = setup();
    await controller.capture("request", download, () => true);
    storage.fail = true;
    controller.terminal("request", "NS_ERROR_ABORT");
    await controller.drain();
    expect(peer.transfers).toBe(0);
    expect(controller.view().blocked).toBe(true);
    expect(controller.view().pending[0]?.stage).toBe("intent");
  });
  it("a lost commit receipt is recovered by status, never another prepare/Add", async () => {
    const { storage, controller, peer } = setup();
    peer.fail = "commit_handoff";
    await controller.capture("request", download, () => true);
    controller.terminal("request", "NS_ERROR_ABORT");
    await controller.drain();
    expect(controller.view().pending[0]?.stage).toBe("cancelled");
    const recovered = new BrowserHandoff(new HandoffJournal(storage), peer);
    await recovered.recover();
    expect(peer.calls).toEqual(["prepare_handoff", "commit_handoff", "get_handoff"]);
    expect(peer.transfers).toBe(1);
    expect(recovered.view().pending).toEqual([]);
  });
  it.each(["cancelled", "confirmed"] as const)(
    "recovers %s before dispatch with the same ID",
    async (stage) => {
      const { controller, peer } = setup(stage);
      await controller.recover();
      expect(peer.calls).toEqual(["get_handoff", "commit_handoff"]);
      expect(peer.transfers).toBe(1);
    },
  );
  it.each(["preparing", "fallback"] as const)(
    "aborts %s without replaying preparation",
    async (stage) => {
      const { controller, peer } = setup(stage);
      await controller.recover();
      expect(peer.calls).toEqual(["abort_handoff"]);
      expect(peer.transfers).toBe(0);
    },
  );
  it("retains unknown outcomes and failed status replies without generating IDs", async () => {
    const { controller, peer } = setup("cancelled");
    peer.fail = "get_handoff";
    await controller.recover();
    expect(controller.view().pending).toHaveLength(1);
    expect(peer.transfers).toBe(0);
  });
  it("requires explicit intent resolution and records confirmation separately from observation", async () => {
    const { controller, storage, peer } = setup("intent");
    await controller.recover();
    expect(peer.calls).toEqual(["get_handoff"]);
    peer.observe = (command) => {
      if (command === "commit_handoff")
        expect(storage.value).toMatchObject({ pending: [{ stage: "confirmed" }] });
    };
    await controller.resolveIntent(id, "manager");
    expect(peer.transfers).toBe(1);
    expect(controller.view().pending).toEqual([]);
  });
});

describe("deadline and preflight races", () => {
  it("an unavailable peer leaves no journal or uncertain reservation", async () => {
    const { controller, peer, storage } = setup();
    peer.ready = async () => {
      throw new Error("unavailable");
    };
    await expect(controller.capture("request", download, () => true)).resolves.toEqual({});
    await controller.drain();
    expect(peer.calls).toEqual([]);
    expect(storage.writes).toEqual([]);
  });
  it("checks elapsed monotonic time even if the timeout callback has not run", async () => {
    const storage = new Storage();
    const journal = new HandoffJournal(storage);
    const peer = new Peer();
    let now = 0;
    const controller = new BrowserHandoff(journal, peer, {
      uuid: () => id,
      now: () => now,
      deadlineMs: 100,
    });
    peer.observe = (command) => {
      if (command === "prepare_handoff") now = 101;
    };
    await expect(controller.capture("request", download, () => true)).resolves.toEqual({});
    await controller.drain();
    expect(peer.calls).toEqual(["prepare_handoff", "abort_handoff"]);
  });
  it("a late intent write cannot change a Firefox fallback decision", async () => {
    vi.useFakeTimers();
    const { storage, controller, peer } = setup();
    const waiting = deferred<void>();
    const original = storage.write.bind(storage);
    storage.write = async (value) => {
      if ((value as { pending: { stage: string }[] }).pending[0]?.stage === "intent")
        await waiting.promise;
      await original(value);
    };
    const decision = controller.capture("request", download, () => true);
    await vi.advanceTimersByTimeAsync(100);
    await expect(decision).resolves.toEqual({});
    waiting.resolve();
    await controller.drain();
    expect(peer.calls).toEqual(["prepare_handoff", "abort_handoff"]);
    expect(peer.transfers).toBe(0);
  });
  it("a browser terminal event before preparation completes invalidates cancellation", async () => {
    const { controller, peer } = setup();
    const waiting = deferred<NativeHandoff>();
    peer.delay = waiting.promise;
    const decision = controller.capture("request", download, () => true);
    controller.terminal("request");
    waiting.resolve(receipt("prepared"));
    await expect(decision).resolves.toEqual({});
    await controller.drain();
    expect(peer.transfers).toBe(0);
  });
  it("does not dismiss a confirmed cancellation whose native task was aborted", async () => {
    const { controller, peer } = setup("cancelled");
    peer.phase = "aborted";
    await controller.recover();
    expect(controller.view().pending).toHaveLength(1);
    expect(peer.calls).toEqual(["get_handoff"]);
  });
});

describe("closed bounded pending journal", () => {
  it.each([
    null,
    {},
    { version: 2, pending: [] },
    {
      version: 1,
      pending: [{ id, stage: "intent", createdAt: 1, url: "https://example.invalid" }],
    },
    { version: 1, pending: [{ id, stage: "invented", createdAt: 1 }] },
    { version: 1, pending: Array(33).fill({ id, stage: "intent", createdAt: 1 }) },
    { version: 1, pending: Array(2).fill({ id, stage: "intent", createdAt: 1 }) },
  ])("preserves and blocks malformed persisted state %#", async (value) => {
    const storage = new Storage();
    storage.value = value;
    const journal = new HandoffJournal(storage);
    await expect(journal.ready()).rejects.toThrow("history is unavailable");
    expect(storage.writes).toEqual([]);
    expect(journal.blocked).toBe(true);
  });
  it("requires independent write readback and preserves failed write uncertainty", async () => {
    const storage = new Storage();
    storage.write = async () => {
      /* Simulated acknowledged but unapplied write. */
    };
    const journal = new HandoffJournal(storage);
    await expect(journal.insert(id, 1)).rejects.toThrow();
    expect(journal.blocked).toBe(true);
    await expect(journal.insert(id, 1)).rejects.toThrow();
  });
  it("refuses a syntactically valid readback that differs from the requested checkpoint", async () => {
    const storage = new Storage();
    const write = storage.write.bind(storage);
    storage.write = async (value) => {
      const changed = JSON.parse(JSON.stringify(value)) as { pending: { createdAt: number }[] };
      changed.pending[0]!.createdAt += 1;
      await write(changed);
    };
    const journal = new HandoffJournal(storage);
    await expect(journal.insert(id, 1)).rejects.toThrow();
    expect(journal.blocked).toBe(true);
  });
  it("serializes simultaneous changes instead of losing another ID", async () => {
    const storage = new Storage();
    const journal = new HandoffJournal(storage);
    const other = "b4ac080c-862f-4ea8-b60c-06a9718b2306";
    await Promise.all([journal.insert(id, 1), journal.insert(other, 2)]);
    await Promise.all([journal.advance(id, "intent"), journal.advance(other, "fallback")]);
    expect(journal.snapshot()).toEqual([
      { id, createdAt: 1, stage: "intent" },
      { id: other, createdAt: 2, stage: "fallback" },
    ]);
  });
});

describe("phase-aware uncertain intent recovery", () => {
  it("rechecks an already committed intent without another commit or Add", async () => {
    const { controller, peer } = setup("intent");
    peer.phase = "committed";
    await controller.recover();
    expect(peer.calls).toEqual(["get_handoff"]);
    expect(controller.view().pending).toEqual([]);
    expect(peer.transfers).toBe(0);
  });
  it("does not strand an aborted intent as a confirmed continuation", async () => {
    const { controller, peer } = setup("intent");
    peer.phase = "aborted";
    await expect(controller.resolveIntent(id, "manager")).rejects.toThrow();
    expect(controller.view().pending[0]?.stage).toBe("intent");
    expect(peer.calls).toEqual(["get_handoff"]);
    await controller.resolveIntent(id, "firefox");
    expect(controller.view().pending).toEqual([]);
    expect(peer.transfers).toBe(0);
  });
});
