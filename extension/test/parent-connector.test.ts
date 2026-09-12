import { describe, expect, it } from "vitest";
import { parentConnector, type ParentTransportApi } from "../src/parent-connector";
import { NativeConnection } from "../src/native-connection";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
const flush = async () => {
  for (let i = 0; i < 30; i++) await Promise.resolve();
};
class Api implements ParentTransportApi {
  opens = 0;
  closes: number[] = [];
  sent: Array<{ id: number; message: unknown }> = [];
  opening = deferred<number>();
  readiness = deferred<boolean>();
  reading = deferred<unknown | null>();
  retiring = deferred<boolean>();
  write = Promise.resolve();
  closeRead = true;
  open() {
    this.opens++;
    return this.opening.promise;
  }
  ready() {
    return this.readiness.promise;
  }
  read() {
    return this.reading.promise;
  }
  postMessage(id: number, message: unknown) {
    this.sent.push({ id, message });
    return this.write;
  }
  close(id: number) {
    this.closes.push(id);
    this.readiness.resolve(false);
    if (this.closeRead) this.reading.resolve(null);
    return this.retiring.promise;
  }
  emit(message: unknown) {
    const read = this.reading;
    this.reading = deferred<unknown | null>();
    read.resolve(message);
  }
}
const hello = { command: "hello", payload: {} };

describe("parent connector ownership", () => {
  it("retains one Hello until native readiness; close waits opening without spawning twice", async () => {
    const api = new Api();
    const connect = parentConnector(api);
    const port = connect();
    port.postMessage(hello);
    expect(() => port.postMessage(hello)).toThrow();
    expect(() => port.postMessage({ command: "add" })).toThrow();
    expect(() => connect()).toThrow();
    port.disconnect();
    await flush();
    expect(api.opens).toBe(1);
    expect(api.sent).toEqual([]);
    expect(api.closes).toEqual([]);
    api.opening.resolve(1);
    await flush();
    expect(api.closes).toEqual([1]);
    expect(() => connect()).toThrow();
    api.retiring.resolve(true);
    await flush();
    const next = connect();
    next.disconnect();
    await flush();
    expect(api.opens).toBe(2);
  });

  it("closes a pending ready before awaiting it, without a startup/retirement cycle", async () => {
    const api = new Api();
    const connect = parentConnector(api);
    const port = connect();
    api.opening.resolve(1);
    await flush();
    port.disconnect();
    await flush();
    expect(api.closes).toEqual([1]);
    api.retiring.resolve(true);
    await flush();
    const next = connect();
    next.disconnect();
    await flush();
    expect(api.opens).toBe(2);
  });

  it("read and write joins remain separate from a successful native close", async () => {
    for (const readFirst of [false, true]) {
      const api = new Api();
      api.closeRead = false;
      const writing = deferred<void>();
      api.write = writing.promise;
      const connect = parentConnector(api);
      const port = connect();
      port.postMessage(hello);
      api.opening.resolve(1);
      api.readiness.resolve(true);
      await flush();
      expect(api.sent).toHaveLength(1);
      port.disconnect();
      api.retiring.resolve(true);
      await flush();
      expect(() => connect()).toThrow();
      if (readFirst) api.reading.resolve(null);
      else writing.resolve();
      await flush();
      expect(() => connect()).toThrow();
      if (readFirst) writing.resolve();
      else api.reading.resolve(null);
      await flush();
      const next = connect();
      next.disconnect();
      await flush();
    }
  });

  it("late read delivery and caller serialization cannot escape revocation", async () => {
    const api = new Api();
    const port = parentConnector(api)();
    const received: unknown[] = [];
    port.onMessage.addListener((value) => received.push(value));
    port.postMessage(hello);
    api.opening.resolve(1);
    api.readiness.resolve(true);
    await flush();
    api.emit({ kind: "late" });
    expect(() =>
      port.postMessage({
        toJSON() {
          port.disconnect();
          return { command: "add" };
        },
      }),
    ).toThrow();
    api.retiring.resolve(true);
    await flush();
    expect(received).toEqual([]);
    expect(api.sent).toHaveLength(1);
    expect(api.closes).toEqual([1]);
  });

  it("rejects oversize/recursive input and bounds writes including an in-flight send", async () => {
    const api = new Api();
    const port = parentConnector(api)();
    expect(() => port.postMessage({ command: "hello", huge: "x".repeat(1048576) })).toThrow();
    expect(() =>
      port.postMessage({
        toJSON() {
          port.postMessage(hello);
          return hello;
        },
      }),
    ).toThrow();
    port.postMessage(hello);
    api.opening.resolve(1);
    api.readiness.resolve(true);
    await flush();
    const writing = deferred<void>();
    api.write = writing.promise;
    for (let i = 0; i < 32; i++) port.postMessage({ command: "get" });
    expect(() => port.postMessage({ command: "get" })).toThrow();
    writing.resolve();
    api.retiring.resolve(true);
    await flush();
    expect(api.closes).toEqual([1]);
  });

  it("failed open or native retirement never authorizes a replacement", async () => {
    for (const failedOpen of [true, false]) {
      const api = new Api();
      const connect = parentConnector(api);
      const port = connect();
      if (failedOpen) api.opening.reject(new Error("synthetic unknown open"));
      else {
        api.opening.resolve(1);
        api.readiness.resolve(true);
      }
      await flush();
      port.disconnect();
      api.retiring.resolve(false);
      await flush();
      expect(() => connect()).toThrow();
      expect(api.opens).toBe(1);
      expect(api.closes).toEqual(failedOpen ? [] : [1]);
    }
  });

  it("delivers real NativeConnection Hello/snapshot, not capture readiness", async () => {
    const api = new Api();
    const client = new NativeConnection(parentConnector(api), "0.1.0");
    const connected = client.connect();
    api.opening.resolve(1);
    api.readiness.resolve(true);
    await flush();
    const sent = api.sent[0]?.message as { correlation_id: string };
    api.emit({
      protocol_version: 2,
      correlation_id: sent.correlation_id,
      kind: "response",
      command: "hello",
      ok: true,
      result: {
        selected_version: 2,
        helper_version: "0.1.0",
        max_message_bytes: 1048576,
        capabilities: [
          "snapshots",
          "coalesced_progress",
          "authenticated_requests",
          "sha256",
          "task_handoff_phase",
        ],
      },
    });
    await flush();
    api.emit({
      protocol_version: 2,
      correlation_id: "event-0",
      kind: "event",
      event: "snapshot",
      sequence: 0,
      emitted_at: "2026-09-05T00:00:00.000Z",
      data: {
        snapshot_id: "snapshot-0",
        page_index: 0,
        tasks: [],
        next_cursor: null,
        complete: true,
      },
    });
    expect((await connected).connected).toBe(true);
    expect(client.supports("prepared_handoff")).toBe(false);
    client.disconnect();
    api.retiring.resolve(true);
    await flush();
    expect(client.state().connected).toBe(false);
    expect(api.closes).toEqual([1]);
  });
});
