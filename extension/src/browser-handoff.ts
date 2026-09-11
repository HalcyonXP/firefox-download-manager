import type { HandoffCommand, NativeHandoff } from "./native-connection";
import { HandoffJournal, type PendingHandoff } from "./handoff-journal";

export interface HandoffPeer {
  ready(): Promise<void>;
  command(command: HandoffCommand, payload: unknown): Promise<NativeHandoff>;
}
export interface AnonymousDownload {
  readonly url: string;
  readonly suggested_filename: string;
}
export interface CaptureDecision {
  readonly cancel?: true;
}
interface Ticket {
  readonly id: string;
  readonly key: string;
  valid: boolean;
  decided: boolean;
  cancelIssued: boolean;
  inserted: boolean;
  nativeAttempted: boolean;
  readonly expiresAt: number;
  readonly answer: (decision: CaptureDecision) => void;
  deadline: ReturnType<typeof setTimeout> | undefined;
  terminal: ReturnType<typeof setTimeout> | undefined;
}
export interface HandoffView {
  readonly blocked: boolean;
  readonly pending: readonly PendingHandoff[];
}

/** No interception is registered here. The caller must establish request eligibility. */
export class BrowserHandoff {
  readonly #journal: HandoffJournal;
  readonly #peer: HandoffPeer;
  readonly #active = new Map<string, Ticket>();
  readonly #jobs = new Set<Promise<void>>();
  readonly #busy = new Map<string, Promise<void>>();
  readonly #listeners = new Set<(view: HandoffView) => void>();
  readonly #uuid: () => string;
  readonly #deadlineMs: number;
  readonly #terminalMs: number;
  readonly #now: () => number;
  constructor(
    journal: HandoffJournal,
    peer: HandoffPeer,
    options: {
      uuid?: () => string;
      now?: () => number;
      deadlineMs?: number;
      terminalMs?: number;
    } = {},
  ) {
    this.#journal = journal;
    this.#peer = peer;
    this.#uuid = options.uuid ?? (() => crypto.randomUUID());
    this.#now = options.now ?? (() => performance.now());
    this.#deadlineMs = options.deadlineMs ?? 2000;
    this.#terminalMs = options.terminalMs ?? 5000;
    if (
      !Number.isFinite(this.#deadlineMs) ||
      this.#deadlineMs <= 0 ||
      this.#deadlineMs > 2000 ||
      !Number.isFinite(this.#terminalMs) ||
      this.#terminalMs <= 0 ||
      this.#terminalMs > 5000
    )
      throw new Error("Invalid handoff deadline");
  }
  view(): HandoffView {
    return { blocked: this.#journal.blocked, pending: this.#journal.snapshot() };
  }
  subscribe(listener: (view: HandoffView) => void): () => void {
    this.#listeners.add(listener);
    listener(this.view());
    return () => this.#listeners.delete(listener);
  }
  #notify(): void {
    for (const listener of this.#listeners) {
      try {
        listener(this.view());
      } catch {
        /* Presentation is not authority. */
      }
    }
  }
  #track(job: Promise<void>): void {
    const guarded = job
      .catch(() => {
        /* Retained journal surfaces uncertain outcomes. */
      })
      .finally(() => {
        this.#jobs.delete(guarded);
        this.#notify();
      });
    this.#jobs.add(guarded);
  }
  /** Owned work can outlive the bounded headers decision, but remains joined here. */
  async drain(): Promise<void> {
    while (this.#jobs.size) await Promise.all([...this.#jobs]);
  }

  capture(
    key: string,
    download: AnonymousDownload,
    eligible: () => boolean,
  ): Promise<CaptureDecision> {
    const prior = this.#active.get(key);
    if (prior) {
      prior.valid = false;
      return Promise.resolve({});
    }
    if (this.#active.size >= 16 || this.#journal.blocked) return Promise.resolve({});
    // Freeze only the two anonymous inputs; no context is spread from the caller.
    const input = { url: download.url, suggested_filename: download.suggested_filename };
    return new Promise((answer) => {
      const ticket: Ticket = {
        id: this.#uuid(),
        key,
        valid: true,
        decided: false,
        cancelIssued: false,
        inserted: false,
        nativeAttempted: false,
        expiresAt: this.#now() + this.#deadlineMs,
        answer,
        deadline: undefined,
        terminal: undefined,
      };
      this.#active.set(key, ticket);
      ticket.deadline = setTimeout(() => this.#decide(ticket, false), this.#deadlineMs);
      this.#track(this.#prepare(ticket, input, eligible));
    });
  }
  #decide(ticket: Ticket, cancel: boolean): void {
    if (ticket.decided) return;
    ticket.decided = true;
    cancel = cancel && this.#now() < ticket.expiresAt;
    ticket.cancelIssued = cancel;
    clearTimeout(ticket.deadline);
    if (cancel) {
      // Missing terminal observation becomes visible uncertainty, never an automatic commit.
      ticket.terminal = setTimeout(() => {
        if (this.#active.get(ticket.key) === ticket) this.#active.delete(ticket.key);
        this.#notify();
      }, this.#terminalMs);
    }
    ticket.answer(cancel ? { cancel: true } : {});
  }
  async #prepare(
    ticket: Ticket,
    download: AnonymousDownload,
    eligible: () => boolean,
  ): Promise<void> {
    try {
      await this.#peer.ready();
      if (ticket.decided || !ticket.valid || this.#now() >= ticket.expiresAt || !eligible()) return;
      await this.#journal.insert(ticket.id, Date.now());
      ticket.inserted = true;
      if (ticket.decided || !ticket.valid || this.#now() >= ticket.expiresAt || !eligible()) return;
      ticket.nativeAttempted = true;
      const receipt = await this.#peer.command("prepare_handoff", { task_id: ticket.id, download });
      if (receipt.task.task_id !== ticket.id || receipt.phase !== "prepared") return;
      if (ticket.decided || !ticket.valid || this.#now() >= ticket.expiresAt || !eligible()) return;
      await this.#journal.advance(ticket.id, "intent");
      if (ticket.decided || !ticket.valid || this.#now() >= ticket.expiresAt || !eligible()) return;
      this.#decide(ticket, true);
    } finally {
      this.#decide(ticket, false);
      this.#notify();
      if (!ticket.cancelIssued) {
        if (this.#active.get(ticket.key) === ticket) this.#active.delete(ticket.key);
        const entry = this.#journal.snapshot().find((value) => value.id === ticket.id);
        if (ticket.inserted && entry && !this.#journal.blocked) {
          if (!ticket.nativeAttempted) await this.#journal.settle(ticket.id);
          else {
            await this.#journal.advance(ticket.id, "fallback");
            await this.#abort(ticket.id);
          }
        }
      }
    }
  }
  /** Only the matching live request's observed cancellation can authorize auto-commit. */
  terminal(key: string, error?: string): void {
    const ticket = this.#active.get(key);
    if (!ticket) return;
    ticket.valid = false;
    if (!ticket.cancelIssued) return;
    clearTimeout(ticket.terminal);
    this.#active.delete(key);
    if (error !== "NS_ERROR_ABORT") {
      this.#notify();
      return;
    }
    this.#track(
      this.#exclusive(ticket.id, async () => {
        await this.#journal.advance(ticket.id, "cancelled");
        await this.#commit(ticket.id);
      }),
    );
  }
  #exclusive(id: string, run: () => Promise<void>): Promise<void> {
    const existing = this.#busy.get(id);
    if (existing) return existing;
    const job = Promise.resolve()
      .then(run)
      .finally(() => this.#busy.delete(id));
    this.#busy.set(id, job);
    return job;
  }
  async #commit(id: string): Promise<void> {
    const receipt = await this.#peer.command("commit_handoff", { task_id: id });
    if (receipt.phase !== "committed" || receipt.task.task_id !== id)
      throw new Error("Handoff receipt refused");
    await this.#journal.settle(id);
  }
  async #abort(id: string): Promise<void> {
    const receipt = await this.#peer.command("abort_handoff", { task_id: id });
    if (receipt.phase !== "aborted" || receipt.task.task_id !== id)
      throw new Error("Handoff receipt refused");
    await this.#journal.settle(id);
  }
  /** Never replay prepare or Add. Intent without an observed cancellation stays pending. */
  async recover(): Promise<void> {
    try {
      await this.#journal.ready();
    } finally {
      this.#notify();
    }
    for (const entry of this.#journal.snapshot()) {
      if (
        [...this.#active.values()].some((ticket) => ticket.id === entry.id) ||
        this.#busy.has(entry.id)
      )
        continue;
      try {
        await this.#exclusive(entry.id, async () => {
          if (entry.stage === "preparing" || entry.stage === "fallback")
            await this.#abort(entry.id);
          else if (entry.stage === "cancelled" || entry.stage === "confirmed") {
            const receipt = await this.#peer.command("get_handoff", { task_id: entry.id });
            if (receipt.task.task_id !== entry.id) throw new Error("Handoff receipt refused");
            if (receipt.phase === "committed") await this.#journal.settle(entry.id);
            else if (receipt.phase === "prepared") await this.#commit(entry.id);
          }
        });
      } catch {
        /* Unknown/native-aborted/corrupt IDs stay visible, never recreated. */
      }
    }
    this.#notify();
  }
  /** Explicit UI confirmation, not inferred agreement or an automatic restart action. */
  async resolveIntent(id: string, choice: "manager" | "firefox"): Promise<void> {
    await this.#journal.ready();
    if (
      this.#busy.has(id) ||
      [...this.#active.values()].some((ticket) => ticket.id === id) ||
      !this.#journal.snapshot().some((entry) => entry.id === id && entry.stage === "intent")
    )
      throw new Error("Handoff is not awaiting confirmation");
    try {
      await this.#exclusive(id, async () => {
        if (choice === "manager") {
          await this.#journal.advance(id, "confirmed");
          await this.#commit(id);
        } else await this.#abort(id);
      });
    } finally {
      this.#notify();
    }
  }
}
