import { describe, expect, it } from "vitest";

import { NativeConnection, type NativePort } from "../src/native-connection";

class MessageListeners {
  readonly listeners: Array<(message: unknown) => void> = [];

  addListener(listener: (message: unknown) => void): void {
    this.listeners.push(listener);
  }

  emit(message: unknown): void {
    for (const listener of this.listeners) {
      listener(message);
    }
  }
}

class DisconnectListeners {
  readonly listeners: Array<() => void> = [];

  addListener(listener: () => void): void {
    this.listeners.push(listener);
  }

  emit(): void {
    for (const listener of this.listeners) {
      listener();
    }
  }
}

class FakePort implements NativePort {
  readonly onMessage = new MessageListeners();
  readonly onDisconnect = new DisconnectListeners();
  readonly sent: unknown[] = [];
  disconnected = false;

  postMessage(message: unknown): void {
    this.sent.push(message);
  }

  disconnect(): void {
    if (!this.disconnected) {
      this.disconnected = true;
      this.onDisconnect.emit();
    }
  }
}

function helloCorrelation(port: FakePort): string {
  const message = port.sent[0];
  if (
    typeof message !== "object" ||
    message === null ||
    !("correlation_id" in message) ||
    typeof message.correlation_id !== "string"
  ) {
    throw new Error("hello correlation was not sent");
  }
  return message.correlation_id;
}

function acceptHello(port: FakePort): void {
  port.onMessage.emit({
    protocol_version: 2,
    correlation_id: helloCorrelation(port),
    kind: "response",
    command: "hello",
    ok: true,
    result: {
      selected_version: 2,
      helper_version: "0.1.0",
      capabilities: ["snapshots", "coalesced_progress"],
      max_message_bytes: 1_048_576,
    },
  });
}

function task(taskId: string, state = "queued"): Record<string, unknown> {
  return {
    task_id: taskId,
    display_name: "file.bin",
    destination: "C:\\Users\\Example User\\Downloads",
    source_origin: "https://example.invalid",
    state,
    transfer_mode: "pending",
    expected_size: null,
    bytes_completed: 0,
    workers: 4,
    speed_bytes_per_second: null,
    eta_seconds: null,
    created_at: "2026-09-05T00:00:00.000Z",
    updated_at: "2026-09-05T00:00:00.000Z",
    error: null,
  };
}

function snapshot(
  port: FakePort,
  sequence: number,
  tasks: readonly Record<string, unknown>[],
): void {
  snapshotPage(port, sequence, `snapshot-${sequence}`, 0, tasks, true);
}

function snapshotPage(
  port: FakePort,
  sequence: number,
  snapshotId: string,
  pageIndex: number,
  tasks: readonly Record<string, unknown>[],
  complete: boolean,
): void {
  port.onMessage.emit({
    protocol_version: 2,
    correlation_id: `event-${sequence}`,
    kind: "event",
    event: "snapshot",
    sequence,
    emitted_at: "2026-09-05T00:00:00.000Z",
    data: {
      snapshot_id: snapshotId,
      page_index: pageIndex,
      tasks,
      next_cursor: complete ? null : `${snapshotId}-${pageIndex + 1}`,
      complete,
    },
  });
}

describe("NativeConnection", () => {
  it("negotiates on demand and resolves only after a complete snapshot", async () => {
    const port = new FakePort();
    const connection = new NativeConnection(() => port, "0.1.0");
    const ready = connection.connect();

    expect(port.sent).toHaveLength(1);
    expect(port.sent[0]).toMatchObject({
      protocol_version: 2,
      kind: "command",
      command: "hello",
      payload: { supported_versions: [2] },
    });
    acceptHello(port);
    snapshot(port, 0, [task("a4ac080c-862f-4ea8-b60c-06a9718b2306")]);

    await expect(ready).resolves.toMatchObject({ connected: true });
    expect(connection.state().tasks).toHaveLength(1);
    expect(connection.state().tasks[0]?.state).toBe("queued");

    port.onMessage.emit({
      protocol_version: 2,
      correlation_id: "event-1",
      kind: "event",
      event: "progress",
      sequence: 1,
      emitted_at: "2026-09-05T00:00:01.000Z",
      data: {
        task_id: "a4ac080c-862f-4ea8-b60c-06a9718b2306",
        bytes_completed: 1024,
        expected_size: 4096,
        speed_bytes_per_second: 512,
        eta_seconds: 6,
        active_workers: 1,
        sampled_at: "2026-09-05T00:00:01.000Z",
      },
    });
    expect(connection.state().tasks[0]?.bytes_completed).toBe(1024);
  });

  it("atomically replaces retained UI state after reconnect", async () => {
    const ports = [new FakePort(), new FakePort()];
    let nextPort = 0;
    const connection = new NativeConnection(() => {
      const port = ports[nextPort];
      nextPort += 1;
      if (port === undefined) {
        throw new Error("no fake port");
      }
      return port;
    }, "0.1.0");

    const first = connection.connect();
    acceptHello(ports[0]!);
    snapshot(ports[0]!, 0, [task("cfc734c8-56e8-4255-af83-0d917f90781f")]);
    await first;
    ports[0]!.onDisconnect.emit();
    expect(connection.state()).toMatchObject({ connected: false });
    expect(connection.state().tasks).toHaveLength(1);

    const second = connection.connect();
    acceptHello(ports[1]!);
    snapshotPage(ports[1]!, 0, "replacement", 0, [], false);
    expect(connection.state()).toMatchObject({ connected: false });
    expect(connection.state().tasks).toHaveLength(1);
    snapshotPage(ports[1]!, 1, "replacement", 1, [], true);
    await second;
    expect(connection.state()).toEqual({ connected: true, tasks: [] });
  });

  it("rejects helper errors and event sequence gaps without exposing payloads", async () => {
    const rejectedPort = new FakePort();
    const rejectedConnection = new NativeConnection(() => rejectedPort, "0.1.0");
    const rejected = rejectedConnection.connect();
    rejectedPort.onMessage.emit({
      protocol_version: 2,
      correlation_id: helloCorrelation(rejectedPort),
      kind: "response",
      command: "hello",
      ok: false,
      error: {
        code: "PROTOCOL_UNSUPPORTED_VERSION",
        display_message: "Version mismatch.",
      },
    });
    await expect(rejected).rejects.toMatchObject({
      failure: "helper_error",
      helperCode: "PROTOCOL_UNSUPPORTED_VERSION",
    });
    expect(rejectedPort.disconnected).toBe(true);

    const gapPort = new FakePort();
    const gapConnection = new NativeConnection(() => gapPort, "0.1.0");
    const ready = gapConnection.connect();
    acceptHello(gapPort);
    snapshot(gapPort, 0, []);
    await ready;
    gapPort.onMessage.emit({
      protocol_version: 2,
      correlation_id: "event-2",
      kind: "event",
      event: "warning",
      sequence: 2,
      emitted_at: "2026-09-05T00:00:00.000Z",
      data: {},
    });
    expect(gapConnection.state().connected).toBe(false);
    expect(gapPort.disconnected).toBe(true);
  });
});

describe("operational commands", () => {
  it("correlates add and publishes the authoritative result", async () => {
    const port = new FakePort();
    const connection = new NativeConnection(() => port, "0.1.0");
    const ready = connection.connect();
    acceptHello(port);
    snapshot(port, 0, []);
    await ready;
    const added = connection.command("add", { url: "https://example.invalid/file.bin" });
    await Promise.resolve();
    const sent = port.sent[1] as Record<string, unknown>;
    const value = task("a4ac080c-862f-4ea8-b60c-06a9718b2306");
    port.onMessage.emit({
      protocol_version: 2,
      correlation_id: sent.correlation_id,
      kind: "response",
      command: "add",
      ok: true,
      result: value,
    });
    await expect(added).resolves.toMatchObject(value);
    expect(connection.state().tasks).toHaveLength(1);
    connection.disconnect();
  });

  it("rejects a disconnected command without replaying it on reconnect", async () => {
    const port = new FakePort();
    const connection = new NativeConnection(() => port, "0.1.0");
    const ready = connection.connect();
    acceptHello(port);
    snapshot(port, 0, []);
    await ready;
    const added = connection.command("add", {});
    await Promise.resolve();
    connection.disconnect();
    await expect(added).rejects.toMatchObject({ failure: "disconnected" });
    expect(port.sent).toHaveLength(2);
  });
});

describe("session capability gate", () => {
  it("never sends reserved session fields to a helper that did not advertise support", async () => {
    const port = new FakePort();
    const connection = new NativeConnection(() => port, "0.1.0");
    const ready = connection.connect();
    acceptHello(port);
    snapshot(port, 0, []);
    await ready;
    await expect(
      connection.command("add", { request_context: { credentials: { cookies: [] } } }),
    ).rejects.toMatchObject({ failure: "protocol_error" });
    expect(port.sent).toHaveLength(1);
    connection.disconnect();
  });
});
