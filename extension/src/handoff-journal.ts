// Contains only opaque task IDs and decision stages, never URLs or HTTP context.
export type HandoffStage = "preparing" | "intent" | "cancelled" | "confirmed" | "fallback";
export interface PendingHandoff {
  readonly id: string;
  readonly stage: HandoffStage;
  readonly createdAt: number;
}
export interface HandoffStorage {
  read(): Promise<unknown>;
  write(value: unknown): Promise<void>;
}
export const HANDOFF_STORAGE_KEY = "pending-handoffs-v1";
export const MAX_PENDING_HANDOFFS = 32;
const stages = new Set<string>(["preparing", "intent", "cancelled", "confirmed", "fallback"]);
export const handoffId = (value: unknown): value is string =>
  typeof value === "string" &&
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u.test(value);

export class HandoffJournalError extends Error {
  constructor() {
    super("Download handoff history is unavailable; automatic capture is paused.");
  }
}
function decode(value: unknown): PendingHandoff[] {
  if (value === undefined) return [];
  if (
    !value ||
    typeof value !== "object" ||
    Array.isArray(value) ||
    Object.keys(value).sort().join(",") !== "pending,version" ||
    !("version" in value) ||
    value.version !== 1 ||
    !("pending" in value) ||
    !Array.isArray(value.pending) ||
    value.pending.length > MAX_PENDING_HANDOFFS
  )
    throw new HandoffJournalError();
  const seen = new Set<string>();
  return value.pending.map((raw: unknown) => {
    if (
      !raw ||
      typeof raw !== "object" ||
      Array.isArray(raw) ||
      Object.keys(raw).sort().join(",") !== "createdAt,id,stage" ||
      !("id" in raw) ||
      !handoffId(raw.id) ||
      seen.has(raw.id) ||
      !("stage" in raw) ||
      typeof raw.stage !== "string" ||
      !stages.has(raw.stage) ||
      !("createdAt" in raw) ||
      typeof raw.createdAt !== "number" ||
      !Number.isSafeInteger(raw.createdAt) ||
      raw.createdAt < 0
    )
      throw new HandoffJournalError();
    seen.add(raw.id);
    return Object.freeze({
      id: raw.id,
      stage: raw.stage as HandoffStage,
      createdAt: raw.createdAt,
    });
  });
}

/** Single background owner; ordered write+readback precedes any destructive decision. */
export class HandoffJournal {
  readonly #storage: HandoffStorage;
  #loaded: Promise<void> | undefined;
  #tail: Promise<void> = Promise.resolve();
  #records: PendingHandoff[] = [];
  #blocked = false;
  constructor(storage: HandoffStorage) {
    this.#storage = storage;
  }
  get blocked(): boolean {
    return this.#blocked;
  }
  snapshot(): readonly PendingHandoff[] {
    return this.#records.slice();
  }
  ready(): Promise<void> {
    return (this.#loaded ??= (async () => {
      try {
        this.#records = decode(await this.#storage.read());
      } catch {
        this.#blocked = true;
        throw new HandoffJournalError();
      }
    })());
  }
  insert(id: string, createdAt: number): Promise<void> {
    return this.#change((records) => {
      if (records.length >= MAX_PENDING_HANDOFFS || records.some((entry) => entry.id === id))
        throw new HandoffJournalError();
      return [...records, { id, stage: "preparing", createdAt }];
    });
  }
  advance(id: string, stage: HandoffStage): Promise<void> {
    return this.#change((records) => {
      const entry = records.find((item) => item.id === id);
      const allowed: Record<HandoffStage, readonly HandoffStage[]> = {
        preparing: ["intent", "fallback"],
        intent: ["cancelled", "confirmed", "fallback"],
        cancelled: [],
        confirmed: [],
        fallback: [],
      };
      if (!entry || !allowed[entry.stage].includes(stage)) throw new HandoffJournalError();
      return records.map((item) => (item.id === id ? { ...item, stage } : item));
    });
  }
  /** Coordinator only: terminal receipt or proof that native preparation was never attempted. */
  settle(id: string): Promise<void> {
    return this.#change((records) => records.filter((entry) => entry.id !== id));
  }
  #change(transform: (records: PendingHandoff[]) => PendingHandoff[]): Promise<void> {
    const operation = this.#tail.then(async () => {
      await this.ready();
      if (this.#blocked) throw new HandoffJournalError();
      const next = decode({ version: 1, pending: transform(this.#records) });
      const expected = { version: 1, pending: next };
      try {
        await this.#storage.write(expected);
        const raw = await this.#storage.read();
        if (raw === undefined) throw new HandoffJournalError();
        const observed = decode(raw);
        if (JSON.stringify(observed) !== JSON.stringify(next)) throw new HandoffJournalError();
        this.#records = next;
      } catch {
        this.#blocked = true;
        throw new HandoffJournalError();
      }
    });
    this.#tail = operation.catch(() => {
      /* A refused operation does not detach later callers. */
    });
    return operation;
  }
}
