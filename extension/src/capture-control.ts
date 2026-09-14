export const CAPTURE_SETTING_KEY = "automatic-capture-v1";
export interface CaptureStorage {
  read(): Promise<unknown>;
  write(value: unknown): Promise<void>;
}
export interface CaptureState {
  readonly available: boolean;
  readonly ready: boolean;
  readonly enabled: boolean;
  readonly busy: boolean;
  readonly failed: boolean;
}
function decode(value: unknown): boolean {
  if (value === undefined) return true; // Ordinary approved installation defaults on.
  if (
    !value ||
    typeof value !== "object" ||
    Array.isArray(value) ||
    Object.keys(value).sort().join(",") !== "enabled,version" ||
    !("version" in value) ||
    value.version !== 1 ||
    !("enabled" in value) ||
    typeof value.enabled !== "boolean"
  )
    throw new Error("Capture preference unavailable");
  return value.enabled;
}

/** Listener availability, saved preference and effective authorization are distinct. */
export class CaptureControl {
  readonly #storage: CaptureStorage;
  readonly #listeners = new Set<(state: CaptureState) => void>();
  #loaded: Promise<void> | undefined;
  #tail: Promise<void> = Promise.resolve();
  #attempted = false;
  #available = false;
  #ready = false;
  #enabled = false;
  #failed = false;
  #pending = 0;
  #revision = 0;
  constructor(storage: CaptureStorage) {
    this.#storage = storage;
  }
  effective(): boolean {
    return this.#available && this.#ready && this.#enabled && !this.#failed && this.#pending === 0;
  }
  state(): CaptureState {
    return {
      available: this.#available,
      ready: this.#ready,
      enabled: this.#enabled,
      busy: this.#pending !== 0,
      failed: this.#failed,
    };
  }
  subscribe(listener: (state: CaptureState) => void): () => void {
    this.#listeners.add(listener);
    listener(this.state());
    return () => this.#listeners.delete(listener);
  }
  #notify(): void {
    for (const listener of this.#listeners) {
      try {
        listener(this.state());
      } catch {
        /* Presentation is not authority. */
      }
    }
  }
  ready(): Promise<void> {
    return (this.#loaded ??= (async () => {
      try {
        const value = decode(await this.#storage.read());
        if (this.#revision === 0) this.#enabled = value;
        this.#ready = true;
      } catch {
        this.#failed = true;
        throw new Error("Capture preference unavailable");
      } finally {
        this.#notify();
      }
    })());
  }
  /** Only reviewed entry points may register listeners, after their permission review. */
  activate(register: (enabled: () => boolean) => void): void {
    if (this.#attempted) throw new Error("Capture registration already attempted");
    this.#attempted = true;
    register(() => this.effective());
    this.#available = true;
    this.#notify();
  }
  setEnabled(enabled: boolean): Promise<void> {
    if (typeof enabled !== "boolean" || !this.#available)
      return Promise.reject(new Error("Capture unavailable"));
    const revision = ++this.#revision;
    this.#enabled = false; // Immediately revoke any not-yet-authorized cancellation.
    this.#pending++;
    this.#notify();
    const job = this.#tail
      .then(async () => {
        await this.ready();
        if (this.#failed) throw new Error("Capture preference unavailable");
        const value = { version: 1, enabled };
        try {
          await this.#storage.write(value);
          const observed = await this.#storage.read();
          if (observed === undefined || decode(observed) !== enabled)
            throw new Error("Capture preference readback refused");
          if (revision === this.#revision) this.#enabled = enabled;
        } catch {
          this.#failed = true;
          throw new Error("Capture preference unavailable");
        }
      })
      .finally(() => {
        this.#pending--;
        this.#notify();
      });
    this.#tail = job.catch(() => {});
    return job;
  }
}
