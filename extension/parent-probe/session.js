// Unselected fileless SDK fixture orchestration, not a Manager controller API.
import { FixedParentLauncher } from "../protection-bridge/parent-launcher.js";

const FIXTURE = "owned-parent-stdio-v1";
const REFUSED = "Owned parent stdio fixture refused";
const ping = Object.freeze({ fixture: FIXTURE, op: "ping", value: "π" });

function refused() {
  throw new Error(REFUSED);
}

function pid(value) {
  if (!Number.isSafeInteger(value) || value <= 0 || value > 0xffffffff) refused();
  return value;
}

export class ParentFixtureSession {
  #launcher;
  #notify;
  #pid = null;
  #failed = false;
  #closed = false;
  #echoed = false;
  #attempted = false;
  #running = null;
  #retirement = null;
  #pending = null;

  constructor(extension, platform, notify, Launcher = FixedParentLauncher) {
    this.#notify = notify;
    this.#launcher = new Launcher(
      extension,
      {
        ...platform,
        spawn: async (options) => {
          const process = await platform.spawn(options);
          // Never lose a returned SDK owner due to failed diagnostic metadata.
          // Return it to the retained transport even if the PID read refuses.
          try {
            this.#pid = pid(process.pid);
          } catch {
            this.#failed = true;
          }
          return process;
        },
      },
      {
        onMessage: (message) => this.#message(message),
        onDisconnect: () => {
          // The launcher observes its callback. Do not await our own retirement
          // here, since it in turn waits for the launcher and running operation.
          void this.close();
        },
      },
    );
  }

  #wait(op) {
    if (this.#closed || this.#pending !== null) refused();
    let resolve;
    const promise = new Promise((done) => {
      resolve = done;
    });
    this.#pending = { op, resolve };
    return promise;
  }

  #message(message) {
    try {
      if (
        message === null ||
        typeof message !== "object" ||
        Array.isArray(message) ||
        message.fixture !== FIXTURE ||
        this.#pending === null ||
        message.op !== this.#pending.op
      )
        refused();
      const keys = Object.keys(message).sort().join(",");
      if (message.op === "ready") {
        if (keys !== "fixture,op,pid" || pid(message.pid) !== this.#pid) refused();
      } else if (keys !== "fixture,op,value" || message.op !== "pong" || message.value !== "π") {
        refused();
      }
      if (this.#failed || this.#closed) refused();
      const pending = this.#pending;
      this.#pending = null;
      pending.resolve(true);
    } catch {
      this.#failed = true;
      refused();
    }
  }

  run(context) {
    if (this.#attempted || this.#closed) return Promise.reject(new Error(REFUSED));
    this.#attempted = true;
    // Schedule after retaining the operation promise, including synchronous
    // caller refusal and reentrant notification/retirement.
    this.#running = Promise.resolve().then(() => this.#run(context));
    return this.#running;
  }

  async #run(context) {
    try {
      const ready = this.#wait("ready");
      if (!(await this.#launcher.start(context)) || !(await ready)) refused();
      if (this.#failed || this.#closed) refused();
      const echoed = this.#wait("pong");
      this.#launcher.postMessage(context, ping);
      if (!(await echoed) || this.#failed || this.#closed) refused();
      this.#echoed = true;
      const value = Object.freeze({
        version: 1,
        qualification: false,
        scope: FIXTURE,
        stage: "echoed",
        pid: this.#pid,
      });
      // The trusted SDK observer is synchronous. Notification is not proof of
      // controller receipt; the separate controller must observe its own nonce.
      this.#notify("ready", value);
      return value;
    } catch {
      this.#failed = true;
      void this.close();
      refused();
    }
  }

  close() {
    if (this.#retirement !== null) return this.#retirement;
    this.#closed = true;
    this.#pending?.resolve(false);
    this.#pending = null;
    this.#retirement = Promise.resolve().then(() => this.#retire());
    // Context/transport hooks cannot await this promise. Observe rejection even
    // before API shutdown attaches a consumer, without replacing the original
    // failed retirement with a successful promise or a delivered receipt.
    void this.#retirement.catch(() => {
      this.#failed = true;
    });
    void this.#launcher.close();
    return this.#retirement;
  }

  async #retire() {
    const launcher = await this.#launcher.close();
    // run() never awaits close(), so retaining both cannot make a self-cycle.
    let running = false;
    if (this.#running !== null) {
      running = await this.#running.then(
        () => true,
        () => false,
      );
    }
    const value = Object.freeze({
      version: 1,
      qualification: false,
      scope: FIXTURE,
      stage: "retired",
      pid: this.#pid,
      attempted: this.#attempted,
      echoed: this.#echoed,
      launcher,
      successful: !this.#failed && running && this.#echoed && launcher.successful === true,
    });
    this.#notify("retired", value);
    return value;
  }

  toJSON() {
    throw new Error("Owned parent stdio fixture is not serializable");
  }
}
