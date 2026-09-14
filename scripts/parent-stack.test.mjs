import assert from "node:assert/strict";
import test from "node:test";
import { buildSync } from "esbuild";
import { ParentApi } from "../extension/protection-bridge/parent-api.js";
import { ParentConnection } from "../extension/protection-bridge/parent-connection.js";

const flush = () => new Promise((resolve) => setImmediate(resolve));
test("actual client/connector/API/broker stack separates tokens and native frames across clean reconnect", async () => {
  const bundle = buildSync({
    stdin: {
      contents:
        'export {NativeConnection} from "./extension/src/native-connection"; export {parentConnector} from "./extension/src/parent-connector";',
      resolveDir: process.cwd(),
    },
    bundle: true,
    write: false,
    format: "esm",
    platform: "node",
  });
  const { NativeConnection, parentConnector } = await import(
    `data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].text).toString("base64")}`
  );
  const context = { active: true };
  const extension = {};
  const frames = [];
  const launchers = [];
  class Launcher {
    constructor(ext, platform, callbacks) {
      assert.equal(ext, extension);
      this.callbacks = callbacks;
      this.closed = false;
      launchers.push(this);
      this.id = `11111111-1111-4111-8111-${String(launchers.length).padStart(12, "0")}`;
    }
    assertCaller(ctx) {
      if (ctx !== context || !context.active || this.closed) throw new Error("guard");
    }
    start(ctx) {
      this.assertCaller(ctx);
      queueMicrotask(() =>
        this.callbacks.onMessage({
          parent_transport: 1,
          admission_id: this.id,
          kind: "offer",
          capture_ready: false,
        }),
      );
      return Promise.resolve(true);
    }
    postMessage(ctx, message) {
      this.assertCaller(ctx);
      frames.push(message);
      if (message.kind === "accept") {
        assert.equal(message.admission_id, this.id);
        queueMicrotask(() =>
          this.callbacks.onMessage({
            parent_transport: 1,
            admission_id: this.id,
            kind: "ready",
            capture_ready: false,
          }),
        );
      } else {
        assert.equal(message.command, "hello");
        queueMicrotask(() => {
          this.callbacks.onMessage({
            protocol_version: 2,
            correlation_id: message.correlation_id,
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
          this.callbacks.onMessage({
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
        });
      }
    }
    close() {
      if (!this.closed) {
        this.closed = true;
        this.callbacks.onDisconnect();
      }
      return Promise.resolve({
        spawn_called: true,
        hooks_removed: true,
        successful: true,
        transport: {
          startup: "started",
          process_waited: true,
          exit_code: 0,
          pipes_closed: true,
          io_settled: true,
          forced: false,
          successful: true,
        },
      });
    }
  }
  class Broker extends ParentConnection {
    constructor(ext, platform, callbacks) {
      super(ext, platform, callbacks, Launcher);
    }
  }
  const retirements = [];
  const owner = new ParentApi(extension, {}, Broker, (value) => {
    assert.ok(Object.isFrozen(value));
    retirements.push(JSON.parse(JSON.stringify(value)));
  });
  const tokens = [];
  const delivered = [];
  const api = {
    open: async () => {
      const id = owner.open(context);
      tokens.push(id);
      return id;
    },
    ready: async (id) => owner.ready(context, id),
    read: async (id) => {
      const message = await owner.read(context, id);
      if (message !== null) delivered.push(message);
      return message;
    },
    postMessage: async (id, message) => owner.postMessage(context, id, message),
    close: async (id) => owner.close(context, id),
  };
  const client = new NativeConnection(parentConnector(api), "0.3.0");
  try {
    for (let i = 0; i < 2; i++) {
      assert.equal((await client.connect()).connected, true);
      assert.equal(client.supports("prepared_handoff"), false);
      assert.equal(client.supports("task_handoff_phase"), true);
      client.disconnect();
      await flush();
    }
    assert.deepEqual(tokens, [1, 2]);
    assert.notEqual(launchers[0].id, launchers[1].id);
    assert.equal(frames.filter((f) => f.kind === "accept").length, 2);
    assert.equal(frames.filter((f) => f.command === "hello").length, 2);
    assert.equal(delivered.length, 4);
    assert.ok(
      delivered.every((m) => m.protocol_version === 2 && !Object.hasOwn(m, "parent_transport")),
    );
  } finally {
    client.disconnect();
    await owner.shutdown();
    await flush();
  }
  assert.ok(launchers.every((l) => l.closed));
  assert.deepEqual(
    retirements.map((r) => r.connection),
    [1, 2],
  );
  for (const value of retirements) {
    assert.deepEqual(Object.keys(value).sort(), ["connection", "receipt", "version"]);
    assert.equal(value.version, 1);
    assert.equal(value.receipt.successful, true);
    assert.equal(value.receipt.capture_ready, false);
    assert.equal(value.receipt.launcher.transport.process_waited, true);
  }
});
