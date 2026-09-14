import { MAX_MESSAGE_BYTES } from "./protocol";
import type { NativeConnector, NativePort } from "./native-connection";

/** Fixed privileged namespace. Tokens route one API connection, not native authority. */
export interface ParentTransportApi {
  open(): Promise<number>;
  ready(connection: number): Promise<boolean>;
  read(connection: number): Promise<unknown | null>;
  postMessage(connection: number, message: unknown): Promise<void>;
  close(connection: number): Promise<boolean>;
}

function refused(): never {
  throw new Error("Download Manager parent connector refused");
}

class ParentPort implements NativePort {
  readonly #api: ParentTransportApi;
  readonly #messages = new Set<(message: unknown) => void>();
  readonly #disconnects = new Set<() => void>();
  readonly #opened: Promise<number>;
  readonly #opening: Promise<void>;
  #id: number | undefined;
  #ready = false;
  #closed = false;
  #hello: unknown | undefined;
  #helloAccepted = false;
  #serializing = false;
  #queued = 0;
  #writes: Promise<void> = Promise.resolve();
  #reading: Promise<void> = Promise.resolve();
  #retirement: Promise<void> | undefined;
  #settled = false;

  constructor(api: ParentTransportApi) {
    this.#api = api;
    // Retain opening even when the synchronous NativePort caller closes before
    // the SDK returns its API-local token. No replacement or blind open retry.
    this.#opened = Promise.resolve().then(async () => {
      const id = await api.open();
      if (!Number.isSafeInteger(id) || id < 1) refused();
      this.#id = id;
      return id;
    });
    this.#opening = this.#opened.then(async (id) => {
      if (this.#closed) return;
      const ready = await api.ready(id);
      if (this.#closed) return;
      if (ready !== true) refused();
      this.#ready = true;
      this.#reading = this.#pump(id);
      if (this.#hello !== undefined) {
        const hello = this.#hello;
        this.#hello = undefined;
        this.#send(hello);
      }
    });
    void this.#opening.catch(() => this.disconnect());
  }

  get settled(): boolean {
    return this.#settled;
  }

  readonly onMessage = {
    addListener: (listener: (message: unknown) => void): void => {
      if (this.#closed || this.#messages.size >= 32) refused();
      this.#messages.add(listener);
    },
  };

  readonly onDisconnect = {
    addListener: (listener: () => void): void => {
      if (this.#closed || this.#disconnects.size >= 32) refused();
      this.#disconnects.add(listener);
    },
  };

  postMessage(value: unknown): void {
    if (this.#closed || this.#serializing) refused();
    this.#serializing = true;
    let message: unknown;
    try {
      const text = JSON.stringify(value);
      if (typeof text !== "string" || new TextEncoder().encode(text).length > MAX_MESSAGE_BYTES)
        refused();
      message = JSON.parse(text) as unknown;
    } finally {
      this.#serializing = false;
    }
    if (this.#closed || typeof message !== "object" || message === null || Array.isArray(message))
      refused();
    if ((message as { command?: unknown }).command === "hello") {
      if (this.#helloAccepted) refused();
      this.#helloAccepted = true;
      if (!this.#ready) {
        this.#hello = message;
        return;
      }
    } else if (!this.#ready || !this.#helloAccepted) refused();
    this.#send(message); // Privileged broker/native decoder remain authoritative.
  }

  #send(message: unknown): void {
    if (this.#closed || this.#queued >= 32) {
      this.disconnect();
      refused();
    }
    this.#queued += 1;
    this.#writes = this.#writes
      .then(async () => {
        if (this.#closed || this.#id === undefined) refused();
        await this.#api.postMessage(this.#id, message);
      })
      .catch(() => this.disconnect())
      .finally(() => {
        this.#queued -= 1;
      });
  }

  async #pump(id: number): Promise<void> {
    try {
      while (!this.#closed) {
        const message = await this.#api.read(id);
        if (this.#closed) break;
        if (message === null) break;
        for (const listener of this.#messages) {
          if (this.#closed) break;
          listener(message);
        }
      }
    } catch {
      // No raw SDK error or uncertain replay. Retirement is never awaited from
      // this read loop: retirement itself joins the loop.
    } finally {
      this.disconnect();
    }
  }

  disconnect(): void {
    if (this.#closed) return;
    this.#closed = true;
    this.#ready = false;
    this.#hello = undefined;
    this.#retirement = Promise.resolve().then(async () => {
      // Opening can reject after an OS effect with no returned token. Preserve
      // that failure; never infer absence or authorize a replacement port.
      const id = await this.#opened;
      // Close before waiting for readiness: the owned close releases a pending
      // native admission. Waiting for ready first would create a startup cycle.
      const closed = Promise.resolve().then(() => this.#api.close(id));
      const results = await Promise.allSettled([
        closed,
        this.#opening,
        this.#writes,
        this.#reading,
      ]);
      if (results.some((result) => result.status !== "fulfilled")) refused();
      if (results[0]?.status !== "fulfilled" || results[0].value !== true) refused();
      this.#settled = true;
    });
    void this.#retirement.catch(() => {});
    this.#messages.clear();
    for (const listener of this.#disconnects) {
      try {
        listener();
      } catch {
        // A throwing consumer cannot discard or bypass retained retirement.
      }
    }
    this.#disconnects.clear();
  }
}

/** One port at a time; clean joined API/read/write retirement before replacement. */
export function parentConnector(api: ParentTransportApi): NativeConnector {
  let owner: ParentPort | undefined;
  return () => {
    if (owner !== undefined && !owner.settled) refused();
    owner = new ParentPort(api);
    return owner;
  };
}
