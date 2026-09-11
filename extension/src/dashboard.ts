import type { NativeState, NativeTask } from "./native-connection";

import { actionsFor, type TaskAction } from "./task-controls";
export { actionsFor } from "./task-controls";

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
  if (task.handoff_phase === "prepared")
    return "Waiting for browser handoff resolution; no Manager transfer has started.";
  if (task.handoff_phase === "aborted")
    return "Unused reservation discarded; identity retained to prevent duplicate transfers.";
  if (task.handoff_phase === "unknown")
    return "Handoff phase unavailable from this older helper; update the paired helper before using task controls.";
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
      row.status.textContent =
        task.handoff_phase === "prepared"
          ? "WAITING FOR HANDOFF"
          : task.handoff_phase === "aborted"
            ? "RESERVATION DISCARDED"
            : task.state.toUpperCase();
      row.details.textContent =
        progressText(task) +
        (task.handoff_phase === "committed"
          ? " · Handoff history retained for duplicate prevention."
          : "");
      row.path.textContent = `${task.destination} · ${task.source_origin}`;
      row.progress.max = task.expected_size || 1;
      if (task.expected_size === null) row.progress.removeAttribute("value");
      else
        row.progress.value =
          task.expected_size === 0 && task.state === "completed" ? 1 : task.bytes_completed;
      row.error.textContent = task.error
        ? `${task.error.code}: ${task.error.display_message}${task.error.code === "CHECKSUM_MISMATCH" ? " Check the digest and create a fresh task; no new final file was published." : ""}${["AUTH_REQUIRED", "AUTH_EXPIRED"].includes(task.error.code) ? " Sign in, paste the original direct URL above, and explicitly add a fresh task with session handoff. The previous task is not changed by a new Add." : ""}`
        : "";
      row.error.hidden = task.error === null;
      const key = `${task.state}:${task.handoff_phase}:${state.connected}`;
      if (row.actionKey !== key) {
        row.actionKey = key;
        row.actions.replaceChildren(
          ...actionsFor(task.state, task.handoff_phase).map(({ action, label }) => {
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
