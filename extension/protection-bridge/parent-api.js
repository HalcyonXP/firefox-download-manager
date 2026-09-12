// One trusted API owner per extension. No SDK dependency, native frame or
// admission ID is accepted from extension callers. Capture remains unavailable.
import { ParentConnection } from "./parent-connection.js";

function refused() {
  throw new Error("Download Manager parent API refused");
}

export class ParentApi {
  #extension;
  #platform;
  #Connection;
  #onRetired;
  #slot = null;
  #closed = false;
  #checking = false;
  #next = 1;

  constructor(extension, platform, Connection = ParentConnection, onRetired = () => {}) {
    this.#extension = extension;
    this.#platform = platform;
    this.#Connection = Connection; // Internal test injection, never an SDK parameter.
    this.#onRetired = onRetired;
  }

  #guard(slot, context, id) {
    if (this.#checking) refused();
    this.#checking = true;
    try {
      if (
        slot === null ||
        this.#closed ||
        this.#slot !== slot ||
        !slot.active ||
        slot.context !== context ||
        !Number.isSafeInteger(id) ||
        id !== slot.id
      )
        refused();
      slot.connection.assertCaller(context);
      if (this.#closed || this.#slot !== slot || !slot.active) refused();
    } catch {
      if (slot !== null && this.#slot === slot && slot.context === context && slot.id === id)
        void this.#retire(slot);
      refused();
    } finally {
      this.#checking = false;
    }
  }

  open(context) {
    if (this.#closed || this.#checking || this.#slot !== null || !Number.isSafeInteger(this.#next))
      refused();
    {
      const slot = {
        context,
        id: this.#next++, // API-local routing only, never native admission or authority.
        connection: null,
        active: true,
        starting: null,
        retirement: null,
        queue: [],
        pending: null,
        reading: false,
      };
      this.#checking = true;
      try {
        slot.connection = new this.#Connection(this.#extension, this.#platform, {
          onMessage: (message) => this.#message(slot, message),
          onDisconnect: () => {
            // Never return retirement to the callback which it waits for.
            void this.#retire(slot);
          },
        });
        // Bad popup/private/inactive callers cannot occupy the extension's slot.
        slot.connection.assertCaller(context);
        if (this.#closed) refused();
        this.#slot = slot; // Retain before any possible SDK/process effect.
        slot.starting = slot.connection.connect(context);
      } catch {
        if (this.#slot === slot) void this.#retire(slot);
        refused();
      } finally {
        this.#checking = false;
      }
    }
    return this.#slot.id;
  }

  ready(context, id) {
    const slot = this.#slot;
    this.#guard(slot, context, id);
    // A memoized successful start alone cannot authorize a later observation.
    return Promise.resolve(slot.starting).then(
      (started) => {
        if (started !== true) {
          void this.#retire(slot);
          return false;
        }
        this.#guard(slot, context, id);
        return slot.connection.transportReady(context) === true;
      },
      () => {
        void this.#retire(slot);
        return false;
      },
    );
  }

  postMessage(context, id, message) {
    const slot = this.#slot;
    this.#guard(slot, context, id);
    slot.connection.postMessage(context, message);
  }

  async read(context, id) {
    const slot = this.#slot;
    this.#guard(slot, context, id);
    if (slot.reading) refused();
    slot.reading = true;
    try {
      const value = slot.queue.length
        ? slot.queue.shift()
        : await new Promise((resolve) => {
            slot.pending = resolve;
          });
      // A close can occur between resolution and this continuation. Never
      // release an old message to a revoked or replacement caller.
      if (!slot.active || this.#closed) return null;
      this.#guard(slot, context, id);
      return value;
    } finally {
      slot.reading = false;
    }
  }

  #message(slot, message) {
    this.#guard(slot, slot.context, slot.id);
    // The broker has already copied, bounded and excluded private frames.
    if (slot.pending !== null) {
      const resolve = slot.pending;
      slot.pending = null;
      resolve(message);
    } else {
      if (slot.queue.length >= 32) {
        void this.#retire(slot);
        refused();
      }
      slot.queue.push(message);
    }
  }

  close(context, id) {
    const slot = this.#slot;
    if (slot === null || slot.context !== context || !Number.isSafeInteger(id) || slot.id !== id)
      refused();
    // A pending close can be observed again, never sent again. A foreign caller
    // cannot close this owner even when its originating context has unloaded.
    if (slot.retirement !== null) return slot.retirement;
    this.#guard(slot, context, id);
    return this.#retire(slot);
  }

  #retire(slot) {
    if (slot.retirement !== null) return slot.retirement;
    slot.active = false;
    slot.queue.length = 0;
    slot.pending?.(null);
    slot.pending = null;
    slot.retirement = Promise.resolve().then(async () => {
      const receipt = await slot.connection.close();
      if (slot.starting !== null) await slot.starting;
      // One-way local observation after the actual retained close. It never
      // supplies authority, completes SDK I/O, or substitutes for those joins.
      this.#onRetired(Object.freeze({ version: 1, connection: slot.id, receipt }));
      // Failed/indeterminate startup or retirement never permits replacement.
      if (receipt.successful !== true) return false;
      if (this.#slot === slot) this.#slot = null;
      return true;
    });
    void slot.retirement.catch(() => {});
    return slot.retirement;
  }

  shutdown() {
    this.#closed = true;
    return this.#slot === null ? Promise.resolve(true) : this.#retire(this.#slot);
  }

  toJSON() {
    throw new Error("Download Manager parent API is not serializable");
  }
}
