import { MAX_MESSAGE_BYTES, PROTOCOL_VERSION, isValidCorrelationId } from "./protocol";

export const NATIVE_HOST_NAME = "com.halcyonxp.firefox_download_manager" as const;

const HANDSHAKE_TIMEOUT_MS = 5_000;
const MAX_SNAPSHOT_TASKS = 10_000;
const MAX_SNAPSHOT_PAGES = 10_000;
const TASK_STATES = new Set([
  "queued",
  "probing",
  "downloading",
  "paused",
  "validating",
  "promoting",
  "completed",
  "failed",
  "cancelled",
]);
const TRANSFER_MODES = new Set(["pending", "single", "segmented"]);
const EVENT_NAMES = new Set([
  "snapshot",
  "state_changed",
  "progress",
  "warning",
  "completed",
  "failed",
]);
const ERROR_CODES = new Set([
  "PROTOCOL_UNSUPPORTED_VERSION",
  "PROTOCOL_UNKNOWN_COMMAND",
  "PROTOCOL_INVALID_MESSAGE",
  "PROTOCOL_MESSAGE_TOO_LARGE",
  "INVALID_URL",
  "UNSUPPORTED_SCHEME",
  "INVALID_DESTINATION",
  "INVALID_FILENAME",
  "INVALID_SETTINGS",
  "TASK_NOT_FOUND",
  "INVALID_TASK_STATE",
  "AUTH_REQUIRED",
  "AUTH_EXPIRED",
  "REDIRECT_REJECTED",
  "PROBE_FAILED",
  "RANGE_UNSUPPORTED",
  "RANGE_RESPONSE_INVALID",
  "RESOURCE_CHANGED",
  "HTTP_STATUS",
  "RETRY_EXHAUSTED",
  "STORAGE_ERROR",
  "DISK_FULL",
  "ACCESS_DENIED",
  "FILE_LOCKED",
  "FILE_EXISTS",
  "STATE_CORRUPT",
  "CHECKSUM_MISMATCH",
  "CANCELLED",
  "INTERNAL_ERROR",
]);

interface ListenerEvent<Listener extends (...arguments_: never[]) => void> {
  addListener(listener: Listener): void;
}

export interface NativePort {
  postMessage(message: unknown): void;
  disconnect(): void;
  readonly onMessage: ListenerEvent<(message: unknown) => void>;
  readonly onDisconnect: ListenerEvent<() => void>;
}

export type NativeConnector = () => NativePort;

export type NativeConnectionFailure =
  "disconnected" | "helper_error" | "protocol_error" | "timeout" | "unavailable";

export class NativeConnectionError extends Error {
  readonly failure: NativeConnectionFailure;
  readonly helperCode: string | undefined;

  constructor(failure: NativeConnectionFailure, helperCode?: string) {
    super(`Native Messaging connection failed: ${failure}`);
    this.name = "NativeConnectionError";
    this.failure = failure;
    this.helperCode = helperCode;
  }
}

export interface NativeTaskError {
  readonly code: string;
  readonly display_message: string;
  readonly context?: Readonly<Record<string, unknown>>;
}

export interface NativeTask {
  readonly task_id: string;
  readonly display_name: string;
  readonly destination: string;
  readonly source_origin: string;
  readonly state: string;
  readonly transfer_mode: string;
  readonly expected_size: number | null;
  readonly bytes_completed: number;
  readonly workers: 1 | 2 | 4 | 8;
  readonly speed_bytes_per_second: number | null;
  readonly eta_seconds: number | null;
  readonly created_at: string;
  readonly updated_at: string;
  readonly error: NativeTaskError | null;
}

export interface NativeSettings {
  readonly destination: string;
  readonly default_workers: 1 | 2 | 4 | 8;
  readonly global_concurrency: number;
  readonly per_host_concurrency: number;
  readonly retry_limit: number;
  readonly keep_partial_on_cancel: boolean;
  readonly keep_partial_on_failure: boolean;
  readonly verbose_logging: boolean;
}

export interface NativeState {
  readonly connected: boolean;
  readonly tasks: readonly NativeTask[];
  readonly settings?: NativeSettings;
}

type StateListener = (state: NativeState) => void;

interface PendingConnection {
  readonly promise: Promise<NativeState>;
  readonly resolve: (state: NativeState) => void;
  readonly reject: (error: NativeConnectionError) => void;
}

interface SnapshotAssembly {
  readonly id: string;
  nextPage: number;
  readonly tasks: Map<string, NativeTask>;
}

/**
 * Owns one on-demand native port. A fresh connection is not considered ready
 * until hello succeeds and the helper's complete authoritative snapshot has
 * atomically replaced any state retained from the previous port.
 */
export class NativeConnection {
  readonly #connector: NativeConnector;
  readonly #clientVersion: string;
  readonly #tasks = new Map<string, NativeTask>();
  readonly #listeners = new Set<StateListener>();
  readonly #commands = new Map<
    string,
    {
      command: string;
      resolve: (result: unknown) => void;
      taskId: string | undefined;
      reject: (error: NativeConnectionError) => void;
      timeout: ReturnType<typeof setTimeout>;
    }
  >();
  #settings: NativeSettings | undefined;
  #port: NativePort | undefined;
  #pending: PendingConnection | undefined;
  #helloCorrelation: string | undefined;
  #helloAccepted = false;
  #capabilities: string[] = [];
  #snapshot: SnapshotAssembly | undefined;
  #nextSequence = 0;
  #correlationCounter = 0;
  #timeout: ReturnType<typeof setTimeout> | undefined;

  constructor(
    connector: NativeConnector = () =>
      browser.runtime.connectNative(NATIVE_HOST_NAME) as NativePort,
    clientVersion: string = browser.runtime.getManifest().version,
  ) {
    this.#connector = connector;
    this.#clientVersion = clientVersion;
  }

  supports(capability: string): boolean {
    return this.state().connected && this.#capabilities.includes(capability);
  }

  /** Opens/reopens the helper and resolves only after its initial snapshot. */
  connect(): Promise<NativeState> {
    if (this.#pending !== undefined) {
      return this.#pending.promise;
    }
    if (this.#port !== undefined && this.#helloAccepted && this.#snapshot === undefined) {
      return Promise.resolve(this.state());
    }

    const pending = promiseWithResolvers<NativeState>();
    this.#pending = pending;
    this.#helloAccepted = false;
    this.#snapshot = undefined;
    this.#nextSequence = 0;

    let port: NativePort;
    try {
      port = this.#connector();
    } catch {
      this.#reject(new NativeConnectionError("unavailable"));
      return pending.promise;
    }
    this.#port = port;
    port.onMessage.addListener((message) => {
      if (this.#port === port) {
        this.#receive(message);
      }
    });
    port.onDisconnect.addListener(() => {
      if (this.#port === port) {
        this.#reject(new NativeConnectionError("disconnected"));
      }
    });

    this.#helloCorrelation = this.#nextCorrelation("hello");
    try {
      port.postMessage({
        protocol_version: PROTOCOL_VERSION,
        correlation_id: this.#helloCorrelation,
        kind: "command",
        command: "hello",
        payload: {
          supported_versions: [PROTOCOL_VERSION],
          client_name: "firefox-download-manager",
          client_version: this.#clientVersion,
        },
      });
    } catch {
      this.#reject(new NativeConnectionError("unavailable"));
      return pending.promise;
    }
    this.#armTimeout();
    return pending.promise;
  }

  /** Commands are never replayed automatically after an uncertain disconnect. */
  command(
    command: "add" | "pause" | "resume" | "cancel" | "get",
    payload: unknown,
  ): Promise<NativeTask>;
  command(command: "remove" | "open_folder", payload: unknown): Promise<unknown>;
  command(command: "get_settings" | "update_settings", payload: unknown): Promise<NativeSettings>;
  async command(
    command:
      | "add"
      | "pause"
      | "resume"
      | "cancel"
      | "get"
      | "remove"
      | "open_folder"
      | "get_settings"
      | "update_settings",
    payload: unknown,
  ): Promise<unknown> {
    await this.connect();
    if (
      command === "add" &&
      isRecord(payload) &&
      payload.request_context !== undefined &&
      !this.supports("authenticated_requests")
    ) {
      throw new NativeConnectionError("protocol_error");
    }
    if (this.#commands.size >= 32) throw new NativeConnectionError("unavailable");
    const correlation = this.#nextCorrelation("command");
    const message = {
      protocol_version: PROTOCOL_VERSION,
      correlation_id: correlation,
      kind: "command",
      command,
      payload,
    };
    if (!isBoundedMessage(message)) throw new NativeConnectionError("protocol_error");
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => this.#reject(new NativeConnectionError("timeout")), 120_000);
      const taskId =
        isRecord(payload) && typeof payload.task_id === "string" ? payload.task_id : undefined;
      this.#commands.set(correlation, { command, resolve, reject, timeout, taskId });
      try {
        this.#port?.postMessage(message);
      } catch {
        this.#reject(new NativeConnectionError("disconnected"));
      }
    });
  }

  /** Closes the current port without discarding its last rendered snapshot. */
  disconnect(): void {
    const port = this.#port;
    this.#reject(new NativeConnectionError("disconnected"));
    try {
      port?.disconnect();
    } catch {
      // The browser may already have torn down the native port.
    }
  }

  /** Returns an immutable latest-value projection. */
  state(): NativeState {
    return Object.freeze({
      connected: this.#port !== undefined && this.#helloAccepted && this.#pending === undefined,
      tasks: Object.freeze([...this.#tasks.values()]),
      ...(this.#settings ? { settings: this.#settings } : {}),
    });
  }

  /** Receives complete state changes; returns an unsubscribe function. */
  subscribe(listener: StateListener): () => void {
    this.#listeners.add(listener);
    listener(this.state());
    return () => this.#listeners.delete(listener);
  }

  #receive(message: unknown): void {
    if (!isBoundedMessage(message)) {
      this.#reject(new NativeConnectionError("protocol_error"));
      return;
    }
    if (isResponse(message)) {
      this.#receiveResponse(message);
      return;
    }
    if (isEvent(message)) {
      this.#receiveEvent(message);
      return;
    }
    this.#reject(new NativeConnectionError("protocol_error"));
  }

  #receiveResponse(message: Record<string, unknown>): void {
    const pending = this.#commands.get(String(message.correlation_id));
    if (pending !== undefined) {
      if (!this.#helloAccepted || message.command !== pending.command) {
        this.#reject(new NativeConnectionError("protocol_error"));
        return;
      }
      clearTimeout(pending.timeout);
      this.#commands.delete(String(message.correlation_id));
      if (message.ok === false && isTaskError(message.error)) {
        pending.reject(new NativeConnectionError("helper_error", message.error.code));
        return;
      }
      if (pending.command === "get_settings" || pending.command === "update_settings") {
        const settings = nativeSettings(message.result);
        if (!settings) {
          pending.reject(new NativeConnectionError("protocol_error"));
          this.#reject(new NativeConnectionError("protocol_error"));
          return;
        }
        this.#settings = settings;
        pending.resolve(settings);
        this.#notify();
        return;
      }
      if (pending.command === "remove" || pending.command === "open_folder") {
        const key = pending.command === "remove" ? "removed_task_id" : "opened_task_id";
        const result = message.result;
        if (
          !isRecord(result) ||
          !hasExactKeys(result, [key]) ||
          result[key] !== pending.taskId ||
          !pending.taskId
        ) {
          pending.reject(new NativeConnectionError("protocol_error"));
          this.#reject(new NativeConnectionError("protocol_error"));
          return;
        }
        if (pending.command === "remove") this.#tasks.delete(pending.taskId);
        pending.resolve(result);
        this.#notify();
        return;
      }
      const task = nativeTask(message.result);
      if (task === undefined || (pending.taskId !== undefined && task.task_id !== pending.taskId)) {
        pending.reject(new NativeConnectionError("protocol_error"));
        this.#reject(new NativeConnectionError("protocol_error"));
        return;
      }
      this.#tasks.set(task.task_id, task);
      pending.resolve(task);
      this.#notify();
      return;
    }
    if (this.#helloAccepted || message.correlation_id !== this.#helloCorrelation) {
      this.#reject(new NativeConnectionError("protocol_error"));
      return;
    }
    if (message.ok === false) {
      if (
        (message.command !== "hello" && message.command !== "protocol") ||
        !isTaskError(message.error)
      ) {
        this.#reject(new NativeConnectionError("protocol_error"));
        return;
      }
      this.#reject(new NativeConnectionError("helper_error", message.error.code));
      return;
    }
    if (message.command !== "hello" || !isHelloResult(message.result)) {
      this.#reject(new NativeConnectionError("protocol_error"));
      return;
    }
    this.#capabilities = (message.result as { capabilities: string[] }).capabilities;
    this.#helloAccepted = true;
    this.#armTimeout();
  }

  #receiveEvent(message: Record<string, unknown>): void {
    if (!this.#helloAccepted || message.sequence !== this.#nextSequence) {
      this.#reject(new NativeConnectionError("protocol_error"));
      return;
    }
    this.#nextSequence += 1;
    if (message.event === "snapshot") {
      this.#receiveSnapshot(message.data);
      return;
    }
    if (this.#pending !== undefined || this.#snapshot !== undefined) {
      this.#reject(new NativeConnectionError("protocol_error"));
      return;
    }
    if (message.event === "state_changed") {
      const data = message.data;
      if (
        !isRecord(data) ||
        !hasExactKeys(data, ["task", "previous_state"]) ||
        !TASK_STATES.has(String(data.previous_state))
      ) {
        this.#reject(new NativeConnectionError("protocol_error"));
        return;
      }
      const task = nativeTask(data.task);
      if (task === undefined) {
        this.#reject(new NativeConnectionError("protocol_error"));
        return;
      }
      this.#tasks.set(task.task_id, task);
      this.#notify();
      return;
    }
    if (message.event === "completed") {
      const task = nativeTask(message.data);
      if (task === undefined) {
        this.#reject(new NativeConnectionError("protocol_error"));
        return;
      }
      this.#tasks.set(task.task_id, task);
      this.#notify();
      return;
    }
    if (message.event === "failed") {
      const data = message.data;
      if (!isRecord(data)) {
        this.#reject(new NativeConnectionError("protocol_error"));
        return;
      }
      const task = nativeTask(data.task);
      if (
        !hasExactKeys(data, ["task", "error"]) ||
        task === undefined ||
        !isTaskError(data.error)
      ) {
        this.#reject(new NativeConnectionError("protocol_error"));
        return;
      }
      this.#tasks.set(task.task_id, task);
      this.#notify();
      return;
    }
    if (message.event === "progress") {
      const progress = nativeProgress(message.data);
      if (progress === undefined) {
        this.#reject(new NativeConnectionError("protocol_error"));
        return;
      }
      const task = this.#tasks.get(progress.taskId);
      if (task !== undefined) {
        this.#tasks.set(
          progress.taskId,
          Object.freeze({
            ...task,
            bytes_completed: progress.bytesCompleted,
            expected_size: progress.expectedSize,
            speed_bytes_per_second: progress.speed,
            eta_seconds: progress.eta,
          }),
        );
        this.#notify();
      }
      return;
    }
    if (message.event === "warning" && isWarning(message.data)) {
      return;
    }
    this.#reject(new NativeConnectionError("protocol_error"));
  }

  #receiveSnapshot(value: unknown): void {
    const page = snapshotPage(value);
    if (page === undefined) {
      this.#reject(new NativeConnectionError("protocol_error"));
      return;
    }
    if (this.#pending !== undefined) {
      this.#armTimeout();
    }
    if (page.pageIndex === 0) {
      if (this.#snapshot !== undefined) {
        this.#reject(new NativeConnectionError("protocol_error"));
        return;
      }
      this.#snapshot = {
        id: page.snapshotId,
        nextPage: 0,
        tasks: new Map(),
      };
    }
    const assembly = this.#snapshot;
    if (
      assembly === undefined ||
      assembly.id !== page.snapshotId ||
      assembly.nextPage !== page.pageIndex ||
      page.pageIndex >= MAX_SNAPSHOT_PAGES ||
      assembly.tasks.size + page.tasks.length > MAX_SNAPSHOT_TASKS
    ) {
      this.#reject(new NativeConnectionError("protocol_error"));
      return;
    }
    for (const task of page.tasks) {
      if (assembly.tasks.has(task.task_id)) {
        this.#reject(new NativeConnectionError("protocol_error"));
        return;
      }
      assembly.tasks.set(task.task_id, task);
    }
    assembly.nextPage += 1;
    if (!page.complete) {
      return;
    }
    this.#tasks.clear();
    for (const [taskId, task] of assembly.tasks) {
      this.#tasks.set(taskId, task);
    }
    this.#snapshot = undefined;
    const pending = this.#pending;
    this.#pending = undefined;
    this.#clearTimeout();
    const state = this.state();
    pending?.resolve(state);
    this.#notify();
  }

  #reject(error: NativeConnectionError): void {
    const pending = this.#pending;
    this.#pending = undefined;
    this.#helloCorrelation = undefined;
    this.#helloAccepted = false;
    this.#snapshot = undefined;
    this.#clearTimeout();
    const port = this.#port;
    this.#port = undefined;
    pending?.reject(error);
    for (const command of this.#commands.values()) {
      clearTimeout(command.timeout);
      command.reject(error);
    }
    this.#commands.clear();
    try {
      port?.disconnect();
    } catch {
      // Disconnect is best effort after a protocol/transport failure.
    }
    this.#notify();
  }

  #armTimeout(): void {
    this.#clearTimeout();
    this.#timeout = setTimeout(() => {
      this.#reject(new NativeConnectionError("timeout"));
    }, HANDSHAKE_TIMEOUT_MS);
  }

  #clearTimeout(): void {
    if (this.#timeout !== undefined) {
      clearTimeout(this.#timeout);
      this.#timeout = undefined;
    }
  }

  #nextCorrelation(prefix: string): string {
    const correlation = `${prefix}-${Date.now().toString(36)}-${this.#correlationCounter.toString(36)}`;
    this.#correlationCounter += 1;
    return correlation;
  }

  #notify(): void {
    const state = this.state();
    for (const listener of this.#listeners) {
      try {
        listener(state);
      } catch {
        // A presentation listener cannot corrupt connection authority.
      }
    }
  }
}

function promiseWithResolvers<T>(): {
  readonly promise: Promise<T>;
  readonly resolve: (value: T) => void;
  readonly reject: (error: NativeConnectionError) => void;
} {
  let resolve!: (value: T) => void;
  let reject!: (error: NativeConnectionError) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function isBoundedMessage(value: unknown): value is Record<string, unknown> {
  if (!isRecord(value)) {
    return false;
  }
  try {
    return new TextEncoder().encode(JSON.stringify(value)).byteLength <= MAX_MESSAGE_BYTES;
  } catch {
    return false;
  }
}

function isResponse(value: Record<string, unknown>): boolean {
  const ok = value.ok;
  const branchIsValid =
    ok === true
      ? hasExactKeys(value, [
          "protocol_version",
          "correlation_id",
          "kind",
          "command",
          "ok",
          "result",
        ])
      : ok === false &&
        hasExactKeys(value, [
          "protocol_version",
          "correlation_id",
          "kind",
          "command",
          "ok",
          "error",
        ]) &&
        isTaskError(value.error);
  return (
    branchIsValid &&
    value.protocol_version === PROTOCOL_VERSION &&
    value.kind === "response" &&
    typeof value.correlation_id === "string" &&
    isValidCorrelationId(value.correlation_id) &&
    typeof value.command === "string"
  );
}

function isEvent(value: Record<string, unknown>): boolean {
  return (
    hasExactKeys(value, [
      "protocol_version",
      "correlation_id",
      "kind",
      "event",
      "sequence",
      "emitted_at",
      "data",
    ]) &&
    value.protocol_version === PROTOCOL_VERSION &&
    value.kind === "event" &&
    typeof value.correlation_id === "string" &&
    isValidCorrelationId(value.correlation_id) &&
    typeof value.event === "string" &&
    EVENT_NAMES.has(value.event) &&
    isSafeInteger(value.sequence) &&
    typeof value.emitted_at === "string" &&
    Number.isFinite(Date.parse(value.emitted_at)) &&
    isRecord(value.data)
  );
}

function isHelloResult(value: unknown): boolean {
  return (
    isRecord(value) &&
    hasExactKeys(value, [
      "selected_version",
      "helper_version",
      "capabilities",
      "max_message_bytes",
    ]) &&
    value.selected_version === PROTOCOL_VERSION &&
    typeof value.helper_version === "string" &&
    value.helper_version.length > 0 &&
    value.helper_version.length <= 128 &&
    value.max_message_bytes === MAX_MESSAGE_BYTES &&
    Array.isArray(value.capabilities) &&
    value.capabilities.includes("snapshots") &&
    new Set(value.capabilities).size === value.capabilities.length &&
    value.capabilities.every(
      (capability) =>
        capability === "snapshots" ||
        capability === "coalesced_progress" ||
        capability === "authenticated_requests" ||
        capability === "sha256",
    )
  );
}

interface SnapshotPage {
  readonly snapshotId: string;
  readonly pageIndex: number;
  readonly tasks: readonly NativeTask[];
  readonly complete: boolean;
}

function snapshotPage(value: unknown): SnapshotPage | undefined {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, ["snapshot_id", "page_index", "tasks", "next_cursor", "complete"]) ||
    typeof value.snapshot_id !== "string" ||
    !isValidCorrelationId(value.snapshot_id) ||
    !isSafeInteger(value.page_index) ||
    !Array.isArray(value.tasks) ||
    value.tasks.length > 200 ||
    typeof value.complete !== "boolean" ||
    (value.complete
      ? value.next_cursor !== null
      : typeof value.next_cursor !== "string" ||
        value.next_cursor.length === 0 ||
        value.next_cursor.length > 1024)
  ) {
    return undefined;
  }
  const tasks: NativeTask[] = [];
  for (const valueTask of value.tasks) {
    const task = nativeTask(valueTask);
    if (task === undefined) {
      return undefined;
    }
    tasks.push(task);
  }
  return {
    snapshotId: value.snapshot_id,
    pageIndex: value.page_index,
    tasks,
    complete: value.complete,
  };
}

interface NativeProgress {
  readonly taskId: string;
  readonly bytesCompleted: number;
  readonly expectedSize: number | null;
  readonly speed: number | null;
  readonly eta: number | null;
}

function nativeProgress(value: unknown): NativeProgress | undefined {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, [
      "task_id",
      "bytes_completed",
      "expected_size",
      "speed_bytes_per_second",
      "eta_seconds",
      "active_workers",
      "sampled_at",
    ]) ||
    typeof value.task_id !== "string" ||
    !isUuid(value.task_id) ||
    !isSafeInteger(value.bytes_completed) ||
    !isNullableSafeInteger(value.expected_size) ||
    !isNullableSafeInteger(value.speed_bytes_per_second) ||
    !isNullableSafeInteger(value.eta_seconds) ||
    !isSafeInteger(value.active_workers) ||
    value.active_workers > 8 ||
    !isDateTime(value.sampled_at)
  ) {
    return undefined;
  }
  return {
    taskId: value.task_id,
    bytesCompleted: value.bytes_completed,
    expectedSize: value.expected_size,
    speed: value.speed_bytes_per_second,
    eta: value.eta_seconds,
  };
}

function isWarning(value: unknown): boolean {
  return (
    isRecord(value) &&
    hasExactKeys(value, ["task_id", "warning"]) &&
    (value.task_id === null || (typeof value.task_id === "string" && isUuid(value.task_id))) &&
    isTaskError(value.warning)
  );
}

function nativeTask(value: unknown): NativeTask | undefined {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, [
      "task_id",
      "display_name",
      "destination",
      "source_origin",
      "state",
      "transfer_mode",
      "expected_size",
      "bytes_completed",
      "workers",
      "speed_bytes_per_second",
      "eta_seconds",
      "created_at",
      "updated_at",
      "error",
    ]) ||
    typeof value.task_id !== "string" ||
    !isUuid(value.task_id) ||
    typeof value.display_name !== "string" ||
    value.display_name.length === 0 ||
    value.display_name.length > 255 ||
    typeof value.destination !== "string" ||
    value.destination.length === 0 ||
    value.destination.length > 32_767 ||
    typeof value.source_origin !== "string" ||
    value.source_origin.length > 4096 ||
    !isSourceOrigin(value.source_origin) ||
    !TASK_STATES.has(String(value.state)) ||
    !TRANSFER_MODES.has(String(value.transfer_mode)) ||
    !isNullableSafeInteger(value.expected_size) ||
    !isSafeInteger(value.bytes_completed) ||
    !isWorkerCount(value.workers) ||
    !isNullableSafeInteger(value.speed_bytes_per_second) ||
    !isNullableSafeInteger(value.eta_seconds) ||
    !isDateTime(value.created_at) ||
    !isDateTime(value.updated_at) ||
    !(value.error === null || isTaskError(value.error)) ||
    (typeof value.expected_size === "number" &&
      typeof value.bytes_completed === "number" &&
      value.bytes_completed > value.expected_size)
  ) {
    return undefined;
  }
  return Object.freeze(value) as unknown as NativeTask;
}

function isTaskError(value: unknown): value is NativeTaskError {
  return (
    isRecord(value) &&
    hasOnlyKeys(value, ["code", "display_message", "context"]) &&
    Object.hasOwn(value, "code") &&
    Object.hasOwn(value, "display_message") &&
    typeof value.code === "string" &&
    ERROR_CODES.has(value.code) &&
    typeof value.display_message === "string" &&
    value.display_message.length > 0 &&
    value.display_message.length <= 4096 &&
    (value.context === undefined || isErrorContext(value.context))
  );
}

function isErrorContext(value: unknown): boolean {
  if (!isRecord(value) || !hasOnlyKeys(value, ["task_id", "status_code", "retry_after_seconds"])) {
    return false;
  }
  return (
    (value.task_id === undefined ||
      (Object.hasOwn(value, "task_id") &&
        typeof value.task_id === "string" &&
        isUuid(value.task_id))) &&
    (value.status_code === undefined ||
      (Object.hasOwn(value, "status_code") &&
        isSafeInteger(value.status_code) &&
        value.status_code >= 100 &&
        value.status_code <= 599)) &&
    (value.retry_after_seconds === undefined ||
      (Object.hasOwn(value, "retry_after_seconds") && isSafeInteger(value.retry_after_seconds)))
  );
}

function hasExactKeys(value: Record<string, unknown>, keys: readonly string[]): boolean {
  return hasOnlyKeys(value, keys) && keys.every((key) => Object.hasOwn(value, key));
}

function hasOnlyKeys(value: Record<string, unknown>, keys: readonly string[]): boolean {
  const allowed = new Set(keys);
  return Object.keys(value).every((key) => allowed.has(key));
}

function isUuid(value: string): boolean {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u.test(value);
}

function isSourceOrigin(value: string): boolean {
  try {
    const parsed = new URL(value);
    return (parsed.protocol === "http:" || parsed.protocol === "https:") && parsed.origin === value;
  } catch {
    return false;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return false;
  }
  const prototype: unknown = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

function isSafeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isNullableSafeInteger(value: unknown): value is number | null {
  return value === null || isSafeInteger(value);
}

function isWorkerCount(value: unknown): value is 1 | 2 | 4 | 8 {
  return value === 1 || value === 2 || value === 4 || value === 8;
}

function isDateTime(value: unknown): value is string {
  return (
    typeof value === "string" &&
    /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/u.test(value) &&
    Number.isFinite(Date.parse(value))
  );
}

export function nativeSettings(value: unknown): NativeSettings | undefined {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, [
      "destination",
      "default_workers",
      "global_concurrency",
      "per_host_concurrency",
      "retry_limit",
      "keep_partial_on_cancel",
      "keep_partial_on_failure",
      "verbose_logging",
    ]) ||
    typeof value.destination !== "string" ||
    !value.destination ||
    value.destination.length > 32767 ||
    !isWorkerCount(value.default_workers) ||
    !isSafeInteger(value.global_concurrency) ||
    value.global_concurrency < 1 ||
    value.global_concurrency > 32 ||
    !isSafeInteger(value.per_host_concurrency) ||
    value.per_host_concurrency < 1 ||
    value.per_host_concurrency > 8 ||
    !isSafeInteger(value.retry_limit) ||
    value.retry_limit > 20 ||
    typeof value.keep_partial_on_cancel !== "boolean" ||
    typeof value.keep_partial_on_failure !== "boolean" ||
    typeof value.verbose_logging !== "boolean"
  )
    return undefined;
  return Object.freeze(value) as unknown as NativeSettings;
}
