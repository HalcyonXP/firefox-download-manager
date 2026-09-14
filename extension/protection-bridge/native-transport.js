// Unselected trusted-parent transport. This is not an extension API or a policy
// decision route. The spawn/timer dependencies must belong to a fixed-host parent
// adapter, never to an API caller. No manifest currently loads this module.
const MAX_FRAME = 1024 * 1024;
const MAX_WRITES = 32;
// Intrinsic brand check also accepts genuine SDK buffers from another realm.
// instanceof would reject them; Symbol.toStringTag/byteLength are spoofable.
const bufferLength = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, "byteLength").get;
// Matches the SDK native-messaging graceful shutdown interval. Expiry requests
// retirement of the retained owner; it never substitutes for waiting/joining.
const RETIRE_AFTER_MS = 3000;

function refused() {
  throw new Error("Download protection transport refused");
}

async function observe(operation) {
  try {
    return { ok: true, value: await operation() };
  } catch {
    return { ok: false };
  }
}

function exitStatus(observation) {
  try {
    const code = observation.value?.exitCode;
    return observation.ok && Number.isInteger(code) ? code : null;
  } catch {
    return null;
  }
}

export class RetainedNativeTransport {
  #spawn;
  #timer;
  #onMessage;
  #onDisconnect;
  #startup = null;
  #attempted = false;
  #process = null;
  #exit = null;
  #read = Promise.resolve();
  #stderr = Promise.resolve();
  #write = Promise.resolve();
  #writing = false;
  #queue = [];
  #outstanding = 0;
  #closed = false;
  #failed = false;
  #retirement = null;
  #kill = null;
  #forced = false;
  #disconnect = Promise.resolve({ ok: true });

  constructor({ spawn, timer, onMessage, onDisconnect }) {
    this.#spawn = spawn;
    this.#timer = timer;
    this.#onMessage = onMessage;
    this.#onDisconnect = onDisconnect;
  }

  // Transport startup only, not a native Hello, companion readiness, context
  // binding, cancellation permission, or download-protection authorization.
  start() {
    if (this.#closed) return Promise.resolve(false);
    if (this.#startup === null) {
      this.#startup = Promise.resolve().then(() => this.#start());
    }
    return this.#startup;
  }

  async #start() {
    if (this.#closed) return false;
    this.#attempted = true;
    try {
      const process = await this.#spawn();
      // Retain before inspecting/using the returned SDK object. A rejected spawn
      // may have failed AFTER OS process creation: never label it "no child".
      this.#process = process;
      if (process === null || typeof process !== "object") refused();
      this.#exit = observe(() => process.wait());
      void this.#exit.then(() => {
        if (!this.#closed) this.#fail();
      });
      if (this.#closed) return false;
      this.#read = this.#readFrames(process.stdout);
      this.#stderr = this.#drainErrors(process.stderr);
      this.#pumpWrites();
      return !this.#closed;
    } catch {
      this.#fail();
      return false;
    }
  }

  // Synchronous serialization freezes the bytes before queueing. The return
  // means queued only, never delivered. No queued/in-flight write is replayed.
  postMessage(message) {
    if (this.#closed || this.#startup === null || this.#outstanding >= MAX_WRITES) refused();
    let bytes;
    try {
      const json = JSON.stringify(message);
      if (typeof json !== "string") refused();
      bytes = new TextEncoder().encode(json);
      if (bytes.length === 0 || bytes.length > MAX_FRAME) refused();
    } catch {
      refused();
    }
    // Serialization may execute a getter/toJSON that closes or reenters us.
    if (this.#closed || this.#outstanding >= MAX_WRITES) refused();
    const frame = new Uint8Array(bytes.length + 4);
    new DataView(frame.buffer).setUint32(0, bytes.length, true);
    frame.set(bytes, 4);
    this.#queue.push(frame);
    this.#outstanding++;
    this.#pumpWrites();
  }

  #pumpWrites() {
    if (this.#writing || this.#process === null || this.#closed || this.#queue.length === 0) return;
    this.#writing = true;
    this.#write = this.#writeFrames();
  }

  async #writeFrames() {
    try {
      while (!this.#closed && this.#queue.length !== 0) {
        const frame = this.#queue.shift();
        try {
          const expected = frame.byteLength;
          // The SDK transfers (and can detach) the buffer. Read its length first.
          const result = await this.#process.stdin.write(frame);
          if (result.bytesWritten !== expected) refused();
        } finally {
          this.#outstanding--;
        }
      }
    } catch {
      if (!this.#closed) this.#fail();
    } finally {
      this.#writing = false;
    }
  }

  async #readFrames(pipe) {
    try {
      while (!this.#closed) {
        const header = await pipe.read(4);
        if (this.#closed) break;
        if (bufferLength.call(header) !== 4) refused();
        const length = new DataView(header).getUint32(0, true);
        if (length === 0 || length > MAX_FRAME) refused();
        const body = await pipe.read(length);
        if (this.#closed) break;
        if (bufferLength.call(body) !== length) refused();
        const message = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(body));
        // The parent adapter must consume/validate private native challenges
        // before forwarding ordinary protocol messages. Callback failure closes.
        const delivered = await observe(() => this.#onMessage(message));
        if (!delivered.ok) {
          this.#fail();
          break;
        }
      }
    } catch {
      if (!this.#closed) this.#fail();
    }
  }

  async #drainErrors(pipe) {
    try {
      while (!this.#closed) {
        // No raw stderr is retained, forwarded, decoded or logged. Fixed-sized
        // reads bound memory even for a hostile/broken helper. Short EOF rejects.
        const bytes = await pipe.read(4096);
        if (bufferLength.call(bytes) !== 4096) refused();
      }
    } catch {
      // This fixed-helper transport conservatively retires even on an early
      // stderr EOF. It never leaves an unread error pipe behind a live port.
      if (!this.#closed) this.#fail();
    }
  }

  #fail() {
    this.#failed = true;
    void this.close();
  }

  // Immediate revocation is separate from the returned retirement observation.
  // Keep this owner (including failures) retained; do not replace it on timeout.
  close() {
    if (this.#retirement !== null) return this.#retirement;
    this.#closed = true;
    this.#outstanding -= this.#queue.length;
    this.#queue.length = 0;
    // Install before notifying external code, which may reenter close().
    this.#retirement = Promise.resolve().then(() => this.#retire());
    this.#disconnect = observe(() => this.#onDisconnect());
    return this.#retirement;
  }

  async #retire() {
    if (this.#startup !== null) await this.#startup;
    if (this.#process === null || typeof this.#process !== "object") {
      const disconnected = await this.#disconnect;
      if (!disconnected.ok) this.#failed = true;
      return Object.freeze({
        startup: this.#attempted ? "indeterminate" : "not-started",
        process_waited: false,
        exit_code: null,
        pipes_closed: false,
        io_settled: true,
        forced: false,
        successful: !this.#attempted && !this.#failed,
      });
    }

    const process = this.#process;
    // A forced pipe close still returns the SDK's actual closedPromise; it is
    // not just the earlier rejected pending read/write. Keep every observation.
    const pipes = ["stdin", "stdout", "stderr"].map((name) =>
      observe(() => process[name].close(true)),
    );
    let cancelTimer = null;
    let timerLive = true;
    try {
      cancelTimer = this.#timer(RETIRE_AFTER_MS, () => {
        if (!timerLive || this.#forced) return;
        this.#forced = true;
        // Windows SDK kill targets its job, not just a PID. A future fixed-host
        // adapter must review ownership of that helper's descendant job members.
        this.#kill = observe(() => process.kill(0));
      });
      if (typeof cancelTimer !== "function") refused();
    } catch {
      this.#failed = true;
    }
    const exit = this.#exit === null ? { ok: false } : await this.#exit;
    const exitCode = exitStatus(exit);
    if (exitCode === null && this.#kill === null) {
      this.#forced = true;
      this.#kill = observe(() => process.kill(0));
    }
    timerLive = false;
    try {
      if (cancelTimer !== null) cancelTimer();
    } catch {
      this.#failed = true;
    }
    const closed = await Promise.all(pipes);
    await Promise.all([this.#read, this.#stderr, this.#write]);
    const killed = this.#kill === null ? { ok: true } : await this.#kill;
    const disconnected = await this.#disconnect;
    if (!disconnected.ok) this.#failed = true;
    return Object.freeze({
      startup: "started",
      process_waited: exit.ok && exitCode !== null,
      exit_code: exitCode,
      pipes_closed: closed.every((result) => result.ok),
      io_settled: true,
      forced: this.#forced,
      successful:
        !this.#failed &&
        !this.#forced &&
        killed.ok &&
        exitCode === 0 &&
        closed.every((result) => result.ok),
    });
  }

  toJSON() {
    throw new Error("Download protection transport is not serializable");
  }
}
