import type { NativeState, NativeTask } from "./native-connection";

export type TaskAction = "pause" | "resume" | "cancel" | "remove" | "open_folder";
export function actionsFor(state: string): Array<{ action: TaskAction; label: string }> {
  const actions: Array<{ action: TaskAction; label: string }> = [];
  if (["queued", "probing", "downloading"].includes(state))
    actions.push({ action: "pause", label: "Pause" });
  if (["queued", "paused", "failed"].includes(state))
    actions.push({
      action: "resume",
      label: state === "failed" ? "Retry" : state === "queued" ? "Start" : "Resume",
    });
  if (["queued", "probing", "downloading", "paused", "validating"].includes(state))
    actions.push({ action: "cancel", label: "Cancel" });
  if (["completed", "failed", "cancelled"].includes(state))
    actions.push({
      action: "remove",
      label: state === "completed" ? "Remove history" : "Remove task & partial",
    });
  actions.push({ action: "open_folder", label: "Open folder" });
  return actions;
}
export function bytes(value: number | null): string {
  if (value === null) return "Unknown size";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let index = 0;
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024;
    index += 1;
  }
  return `${value.toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
}
export function progressText(task: NativeTask): string {
  if (["validating", "promoting"].includes(task.state))
    return `${bytes(task.bytes_completed)} / ${bytes(task.expected_size)} · ${task.state === "validating" ? "Validating file; final name withheld" : "Publishing validated file"} · Completion time unknown`;
  const eta = task.eta_seconds === null ? "ETA unknown" : `${task.eta_seconds}s remaining`;
  const speed =
    task.speed_bytes_per_second === null
      ? "Measuring speed"
      : `${bytes(task.speed_bytes_per_second)}/s`;
  return `${bytes(task.bytes_completed)} / ${bytes(task.expected_size)} · ${speed} · ${eta} · ${task.workers} configured connection(s) · ${task.transfer_mode}`;
}

interface Row {
  root: HTMLElement;
  title: HTMLElement;
  status: HTMLElement;
  details: HTMLElement;
  progress: HTMLProgressElement;
  path: HTMLElement;
  error: HTMLElement;
  actions: HTMLElement;
  actionKey: string;
}
export class Dashboard {
  readonly #rows = new Map<string, Row>();
  readonly #container: HTMLElement;
  readonly #act: (action: TaskAction, task: NativeTask) => void;
  #latest: NativeState | undefined;
  #scheduled = false;
  #busy = false;
  constructor(container: HTMLElement, act: (action: TaskAction, task: NativeTask) => void) {
    this.#container = container;
    this.#act = act;
  }
  update(state: NativeState): void {
    this.#latest = state;
    this.#schedule();
  }
  busy(value: boolean): void {
    this.#busy = value;
    this.#schedule();
  }
  #schedule(): void {
    if (this.#scheduled) return;
    this.#scheduled = true;
    requestAnimationFrame(() => {
      this.#scheduled = false;
      if (this.#latest) this.#render(this.#latest);
    });
  }
  #render(state: NativeState): void {
    const ids = new Set(state.tasks.map((task) => task.task_id));
    for (const [id, row] of this.#rows)
      if (!ids.has(id)) {
        if (row.root.contains(document.activeElement)) this.#container.focus();
        row.root.remove();
        this.#rows.delete(id);
      }
    for (const task of state.tasks) {
      let row = this.#rows.get(task.task_id);
      if (!row) {
        row = makeRow(task.task_id);
        this.#rows.set(task.task_id, row);
        this.#container.append(row.root);
      }
      row.root.dataset.state = task.state;
      row.title.textContent = task.display_name;
      row.status.textContent = task.state.toUpperCase();
      row.details.textContent = progressText(task);
      row.path.textContent = `${task.destination} · ${task.source_origin}`;
      row.progress.max = task.expected_size || 1;
      if (task.expected_size === null) row.progress.removeAttribute("value");
      else
        row.progress.value =
          task.expected_size === 0 && task.state === "completed" ? 1 : task.bytes_completed;
      row.error.textContent = task.error
        ? `${task.error.code}: ${task.error.display_message}${task.error.code === "CHECKSUM_MISMATCH" ? " Check the digest and create a fresh task; no new final file was published." : ""}${["AUTH_REQUIRED", "AUTH_EXPIRED"].includes(task.error.code) ? " Sign in, paste the original direct URL above, and explicitly add a fresh task with session handoff. Remove the old task separately to delete its partial." : ""}`
        : "";
      row.error.hidden = task.error === null;
      const key = `${task.state}:${state.connected}`;
      if (row.actionKey !== key) {
        row.actionKey = key;
        row.actions.replaceChildren(
          ...actionsFor(task.state).map(({ action, label }) => {
            const button = document.createElement("button");
            button.type = "button";
            button.textContent = label;
            button.disabled = !state.connected || this.#busy;
            button.addEventListener("click", () => this.#act(action, task));
            return button;
          }),
        );
      }
      for (const button of row.actions.querySelectorAll("button"))
        button.disabled = !state.connected || this.#busy;
    }
  }
}
function makeRow(id: string): Row {
  const root = document.createElement("article");
  root.className = "task";
  const title = document.createElement("h3");
  title.id = `task-${id}`;
  root.setAttribute("aria-labelledby", title.id);
  const status = document.createElement("p");
  status.className = "status";
  const details = document.createElement("p");
  const path = document.createElement("p");
  path.className = "hint";
  const progress = document.createElement("progress");
  progress.setAttribute("aria-labelledby", title.id);
  const error = document.createElement("p");
  const actions = document.createElement("div");
  actions.className = "actions";
  root.append(title, status, progress, details, path, error, actions);
  return { root, title, status, details, path, progress, error, actions, actionKey: "" };
}
