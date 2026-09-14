/** Site/API authority is separate from the saved capture preference. */
export interface CaptureAccessState {
  readonly selected: boolean;
  readonly checking: boolean;
  readonly granted: boolean;
  readonly failed: boolean;
}
export function capturePermissions(): browser.permissions.Permissions {
  return {
    permissions: ["webRequest", "webRequestBlocking"],
    origins: ["http://*/*", "https://*/*"],
  };
}
/** Required API permissions are checked, never requested as optional permissions. */
export function captureSitePermissions(): browser.permissions.Permissions {
  return { origins: ["http://*/*", "https://*/*"] };
}
interface AccessApi {
  contains(): Promise<boolean>;
  onAdded(listener: () => void): void;
  onRemoved(listener: () => void): void;
}
export class CaptureAccess {
  readonly #api: AccessApi;
  readonly #listeners = new Set<(state: CaptureAccessState) => void>();
  #state: CaptureAccessState = { selected: false, checking: false, granted: false, failed: false };
  #revision = 0;
  #started = false;
  #listening = false;
  constructor(api: AccessApi) {
    this.#api = api;
  }
  state(): CaptureAccessState {
    return { ...this.#state };
  }
  allowed(): boolean {
    return (
      this.#state.selected && this.#state.granted && !this.#state.checking && !this.#state.failed
    );
  }
  subscribe(listener: (state: CaptureAccessState) => void): () => void {
    this.#listeners.add(listener);
    listener(this.state());
    return () => this.#listeners.delete(listener);
  }
  #publish(state: CaptureAccessState): void {
    this.#state = state;
    for (const listener of this.#listeners) {
      try {
        listener(this.state());
      } catch {
        /* Presentation is not authority. */
      }
    }
  }
  start(): void {
    if (this.#started) throw new Error("Capture access already started");
    this.#started = true;
    try {
      const changed = (): void => {
        if (this.#listening) void this.#refresh();
      };
      this.#api.onAdded(changed);
      this.#api.onRemoved(changed);
      this.#listening = true;
      changed();
    } catch {
      ++this.#revision;
      this.#publish({ selected: true, checking: false, granted: false, failed: true });
    }
  }
  recheck(): Promise<void> {
    return this.#listening ? this.#refresh() : Promise.resolve();
  }
  async #refresh(): Promise<void> {
    const revision = ++this.#revision;
    // Revoke synchronously, before awaiting any new permission readback.
    this.#publish({ selected: true, checking: true, granted: false, failed: false });
    try {
      const granted = await this.#api.contains();
      if (typeof granted !== "boolean") throw new Error("Invalid permission receipt");
      if (revision === this.#revision)
        this.#publish({ selected: true, checking: false, granted, failed: false });
    } catch {
      if (revision === this.#revision)
        this.#publish({ selected: true, checking: false, granted: false, failed: true });
    }
  }
}
