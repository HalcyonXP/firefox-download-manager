// Unselected trusted-parent broker. Transport admission is NOT capture readiness.
// No extension API exposes its launcher, platform, callbacks or admission ID.
import { FixedParentLauncher } from "./parent-launcher.js";

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u;
const CORRELATION = /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u;
const COMMANDS = new Set([
  "hello",
  "add",
  "pause",
  "resume",
  "cancel",
  "remove",
  "list",
  "get",
  "open_folder",
  "get_settings",
  "update_settings",
]);
const MAX_FRAME = 1024 * 1024;

function refused() {
  throw new Error("Download Manager parent connection refused");
}
function keys(value, expected) {
  return (
    value !== null &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    Object.keys(value).sort().join(",") === expected
  );
}
function copy(value) {
  const text = JSON.stringify(value);
  if (typeof text !== "string" || new TextEncoder().encode(text).length > MAX_FRAME) refused();
  return JSON.parse(text);
}
function command(value) {
  if (
    !keys(value, "command,correlation_id,kind,payload,protocol_version") ||
    value.protocol_version !== 2 ||
    value.kind !== "command" ||
    !COMMANDS.has(value.command) ||
    typeof value.correlation_id !== "string" ||
    !CORRELATION.test(value.correlation_id) ||
    value.payload === null ||
    typeof value.payload !== "object" ||
    Array.isArray(value.payload)
  )
    refused();
  // Payload decoding remains the existing closed native wire2 schema. Private
  // messages and captured handoffs cannot be selected through this discriminator.
}
function wire(value) {
  if (
    value.protocol_version !== 2 ||
    typeof value.correlation_id !== "string" ||
    !CORRELATION.test(value.correlation_id)
  )
    refused();
  if (value.kind === "response") {
    const fields =
      value.ok === true
        ? "command,correlation_id,kind,ok,protocol_version,result"
        : value.ok === false
          ? "command,correlation_id,error,kind,ok,protocol_version"
          : "";
    if (!keys(value, fields)) refused();
  } else if (value.kind === "event") {
    if (!keys(value, "correlation_id,data,emitted_at,event,kind,protocol_version,sequence"))
      refused();
  } else refused();
}

export class ParentConnection {
  #launcher;
  #onMessage;
  #context = null;
  #running = null;
  #retirement = null;
  #resolve = null;
  #stage = "new";
  #id = null;
  #hello = null;
  #helloAccepted = false;
  #failed = false;
  #handling = false;
  #serializing = false;
  #observed = false;

  constructor(extension, platform, { onMessage, onDisconnect }, Launcher = FixedParentLauncher) {
    this.#onMessage = onMessage;
    this.#launcher = new Launcher(extension, platform, {
      onMessage: (message) => this.#message(message),
      onDisconnect: () => {
        // The launcher's retirement observes this callback. Never return/await
        // our retirement here, which itself waits for that exact launcher.
        void this.close();
        return onDisconnect();
      },
    });
  }

  #guard(context) {
    if (context !== this.#context || context === null || this.#stage === "closed") refused();
    this.#launcher.assertCaller(context);
    if (this.#stage === "closed") refused();
  }

  connect(context) {
    if (this.#stage !== "new") refused();
    this.#context = context;
    this.#stage = "starting";
    const ready = new Promise((resolve) => {
      this.#resolve = resolve;
    });
    this.#running = Promise.resolve().then(async () => {
      try {
        this.#guard(context);
        if ((await this.#launcher.start(context)) !== true) refused();
        if ((await ready) !== true) return false;
        this.#guard(context);
        return this.#stage === "ready";
      } catch {
        this.#fail();
        return false;
      }
    });
    return this.#running;
  }

  // A live check, not a reusable promise result or policy-authority receipt.
  transportReady(context) {
    this.#guard(context);
    return this.#stage === "ready";
  }

  postMessage(context, value) {
    this.#guard(context);
    if (this.#serializing) refused();
    this.#serializing = true;
    let message;
    try {
      message = copy(value);
      command(message);
    } finally {
      this.#serializing = false;
    }
    this.#guard(context); // Serialization can invoke caller code and revoke us.
    if (message.command === "hello") {
      if (this.#helloAccepted) refused();
      this.#helloAccepted = true; // An uncertain queued write must never replay.
      if (this.#stage !== "ready") {
        this.#hello = message; // Exactly one bounded Hello; never an early Add.
        return;
      }
    } else if (this.#stage !== "ready" || !this.#helloAccepted) refused();
    try {
      this.#launcher.postMessage(context, message);
    } catch {
      // The queue may have accepted the command before reporting failure.
      // Revoke this connection, retain retirement and never replay that send.
      this.#fail();
      refused();
    }
  }

  #message(value) {
    try {
      this.#guard(this.#context);
      if (this.#handling) refused();
      this.#handling = true;
      const message = copy(value);
      this.#guard(this.#context);
      if (this.#stage === "ready") {
        if (!this.#helloAccepted) refused();
        wire(message); // Private offer/ready/decision frames never reach clients.
        return this.#onMessage(message);
      }
      if (
        !keys(message, "admission_id,capture_ready,kind,parent_transport") ||
        message.parent_transport !== 1 ||
        message.capture_ready !== false ||
        typeof message.admission_id !== "string" ||
        !UUID.test(message.admission_id)
      )
        refused();
      if (this.#stage === "starting" && message.kind === "offer") {
        this.#id = message.admission_id;
        this.#stage = "accepting"; // Before potentially uncertain queue delivery.
        this.#launcher.postMessage(this.#context, {
          parent_transport: 1,
          admission_id: this.#id,
          kind: "accept",
        });
        // A queued write is not acceptance. Only a later exact native ready is.
      } else if (
        this.#stage === "accepting" &&
        message.kind === "ready" &&
        message.admission_id === this.#id
      ) {
        this.#stage = "ready";
        this.#observed = true;
        const hello = this.#hello;
        this.#hello = null;
        if (hello !== null) this.#launcher.postMessage(this.#context, hello);
        this.#guard(this.#context);
        this.#resolve(true);
        this.#resolve = null;
      } else refused();
    } catch {
      this.#fail();
    } finally {
      this.#handling = false;
    }
  }

  #fail() {
    this.#failed = true;
    void this.close();
  }

  close() {
    if (this.#retirement !== null) return this.#retirement;
    this.#stage = "closed";
    this.#id = null;
    this.#hello = null;
    this.#resolve?.(false);
    this.#resolve = null;
    this.#retirement = Promise.resolve().then(async () => {
      const launcher = await this.#launcher.close();
      if (this.#running !== null) await this.#running;
      return Object.freeze({
        transport_admission_observed: this.#observed,
        capture_ready: false,
        launcher,
        successful: !this.#failed && launcher.successful === true,
      });
    });
    // Preserve the original failed retirement for its owner, but observe rejection
    // immediately even when a synchronous SDK hook cannot await this promise.
    void this.#retirement.catch(() => {});
    return this.#retirement;
  }

  toJSON() {
    throw new Error("Download Manager parent connection is not serializable");
  }
}
