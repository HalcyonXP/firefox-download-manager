// Internal trusted-parent association, not a public API or publication decision.
// No manifest selects this component. Dependencies must never come from API input.
import { RegisteredRequestContexts } from "./request-context.js";

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u;
const CAPACITY = 32;
const HISTORY = 10000;
function refused() {
  throw new Error("Download protection handoff context refused");
}
function notSerializable() {
  throw new Error("Download protection handoff context is not serializable");
}

export class HandoffContexts {
  #reader;
  #connection;
  #owner = null;
  #closed = false;
  #busy = false;
  #epoch = 0;
  #records = new Map();
  #handoffs = new Set();
  #requests = new Set();

  constructor(extension, platform, connection) {
    this.#reader = new RegisteredRequestContexts(extension, platform);
    this.#connection = connection;
  }

  #guard(context) {
    if (this.#closed || (this.#owner !== null && this.#owner !== context)) refused();
    try {
      // Admission is necessary for this association, never sufficient for capture.
      if (this.#connection.transportReady(context) !== true) refused();
      if (context.active !== true || context.unloaded !== false || this.#closed) refused();
    } catch {
      this.close(); // The original connection/context cannot regain these bindings.
      refused();
    }
  }

  #operation(body) {
    if (this.#busy || this.#closed) refused();
    this.#busy = true;
    try {
      return body();
    } catch {
      // No browser exceptions, source URLs, IDs or native principals in errors.
      refused();
    } finally {
      this.#busy = false;
    }
  }

  // Must run at the registered response phase, BEFORE native prepare/cancellation.
  // An ID is consumed even when metadata capture fails. No retry/rebinding by ID.
  bind(context, handoffId, requestId, tabId, sourceUrl) {
    return this.#operation(() => {
      this.#guard(context);
      if (
        typeof handoffId !== "string" ||
        !UUID.test(handoffId) ||
        typeof requestId !== "string" ||
        !/^[1-9][0-9]{0,15}$/u.test(requestId) ||
        !Number.isSafeInteger(Number(requestId)) ||
        !Number.isSafeInteger(tabId) ||
        tabId < 0 ||
        typeof sourceUrl !== "string" ||
        sourceUrl.length === 0 ||
        sourceUrl.length > 16 * 1024 ||
        this.#handoffs.has(handoffId) ||
        this.#requests.has(requestId) ||
        this.#records.size >= CAPACITY ||
        this.#handoffs.size >= HISTORY
      )
        refused();
      this.#handoffs.add(handoffId);
      this.#requests.add(requestId);
      if (this.#owner === null) {
        this.#owner = context; // Retain before uncertain hook registration.
        try {
          context.callOnClose({ close: () => this.close() });
        } catch {
          this.close();
          refused();
        }
      }
      this.#guard(context);
      const metadata = this.#reader.capture(context, requestId, tabId);
      let retained = false;
      try {
        const data = metadata.read();
        if (
          data.requestId !== requestId ||
          data.tabId !== tabId ||
          data.sourceURI.spec !== sourceUrl
        )
          refused();
        this.#guard(context);
        const binding = Object.freeze({ toJSON: notSerializable });
        const record = Object.freeze({
          handoffId,
          requestId,
          tabId,
          sourceUrl,
          metadata,
          epoch: ++this.#epoch,
        });
        this.#records.set(binding, record);
        retained = true;
        return binding;
      } finally {
        if (!retained) metadata.release();
      }
    });
  }

  // The dispatcher supplies the native challenge's task/source, not caller JSON.
  // This returns metadata only. It does not attest cancellation, bytes or policy.
  snapshot(context, binding, handoffId, sourceUrl) {
    return this.#operation(() => {
      this.#guard(context);
      const record = this.#records.get(binding);
      if (!record || record.handoffId !== handoffId || record.sourceUrl !== sourceUrl) refused();
      const metadata = record.metadata.read();
      if (
        metadata.requestId !== record.requestId ||
        metadata.tabId !== record.tabId ||
        metadata.sourceURI.spec !== sourceUrl
      )
        refused();
      this.#guard(context);
      if (this.#records.get(binding) !== record) refused();
      return Object.freeze({ epoch: record.epoch, metadata, toJSON: notSerializable });
    });
  }

  // Trusted abort/task retirement only. Released identities remain tombstoned;
  // native history is untouched and no new capture is authorized by release.
  release(binding) {
    const record = this.#records.get(binding);
    if (!record) return;
    this.#records.delete(binding);
    record.metadata.release();
  }

  close() {
    if (this.#closed) return;
    this.#closed = true;
    this.#reader.close();
    this.#records.clear();
    this.#owner = null;
    this.#connection = null;
    // Keep the small identity tombstones; this consumed registry never reopens.
  }

  toJSON() {
    return notSerializable();
  }
}
