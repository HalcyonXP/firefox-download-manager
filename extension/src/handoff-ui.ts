import type { HandoffAction } from "./handoff-actions";
import type { HandoffView } from "./browser-handoff";
import type { HandoffStage, PendingHandoff } from "./handoff-journal";
import type { NativeTask } from "./native-connection";

type TaskLabel = Pick<NativeTask, "task_id" | "display_name"> &
  Partial<Pick<NativeTask, "handoff_phase">>;
export function handoffLabel(entry: PendingHandoff, tasks: readonly TaskLabel[]): string {
  const task = tasks.find((value) => value.task_id === entry.id);
  return `${task?.display_name ?? "Task details unavailable"} (${entry.id}): ${handoffStageText[entry.stage]}`;
}

export function canContinueInManager(id: string, tasks: readonly TaskLabel[]): boolean {
  return tasks.some((task) => task.task_id === id && task.handoff_phase === "prepared");
}

export function canAcknowledgeAborted(entry: PendingHandoff, tasks: readonly TaskLabel[]): boolean {
  return (
    (entry.stage === "cancelled" || entry.stage === "confirmed") &&
    tasks.some((task) => task.task_id === entry.id && task.handoff_phase === "aborted")
  );
}

/** Candidates only; coordinator rechecks loaded history, activity and native status. */
export function unlinkedReservations(
  view: HandoffView,
  tasks: readonly TaskLabel[],
): readonly TaskLabel[] {
  if (!view.loaded || view.blocked) return [];
  const recorded = new Set(view.pending.map((entry) => entry.id));
  return tasks
    .filter((task) => task.handoff_phase === "prepared" && !recorded.has(task.task_id))
    .slice(0, 32);
}

export const handoffStageText: Record<HandoffStage, string> = {
  preparing: "Preparation interrupted or pending; no automatic cancellation was authorized.",
  fallback: "Firefox retained control; Manager reservation cleanup is pending.",
  intent:
    "Browser cancellation is uncertain. Check Firefox downloads before choosing where to continue.",
  cancelled:
    "Firefox cancellation was observed. Manager acceptance still needs recovery; do not submit a new Add.",
  confirmed:
    "Continuation was explicitly confirmed. Manager acceptance still needs recovery; do not submit a new Add.",
};
export function renderHandoffs(
  container: HTMLElement,
  view: HandoffView,
  act: (id: string, choice: HandoffAction) => void,
  tasks: readonly TaskLabel[] = [],
): void {
  const unlinked = unlinkedReservations(view, tasks);
  container.hidden = !view.blocked && view.pending.length === 0 && unlinked.length === 0;
  const heading = document.createElement("h2");
  heading.textContent = "Interrupted download handoffs";
  const nodes: HTMLElement[] = [heading];
  if (view.blocked) {
    const warning = document.createElement("p");
    warning.textContent =
      "Handoff history could not be verified. Automatic capture is paused; existing records have not been cleared.";
    nodes.push(warning);
  }
  for (const entry of view.pending) {
    const row = document.createElement("div");
    row.className = "panel";
    const text = document.createElement("p");
    text.textContent =
      handoffLabel(entry, tasks) +
      (canAcknowledgeAborted(entry, tasks)
        ? " Manager has discarded this reservation and cannot complete it. Check the download before dismissing this notice."
        : "");
    row.append(text);
    const choices =
      entry.stage === "intent"
        ? (["manager", "firefox", "recheck"] as const)
        : canAcknowledgeAborted(entry, tasks)
          ? (["acknowledge", "recheck"] as const)
          : (["recheck"] as const);
    for (const choice of choices) {
      const button = document.createElement("button");
      button.type = "button";
      button.disabled =
        !view.loaded ||
        view.blocked ||
        (choice === "manager" && !canContinueInManager(entry.id, tasks));
      button.textContent =
        choice === "manager"
          ? "I checked Firefox stopped — continue in Manager"
          : choice === "firefox"
            ? "Keep in Firefox; discard unused reservation"
            : choice === "acknowledge"
              ? "I checked the discarded download — dismiss notice"
              : "Recheck Manager";
      button.addEventListener("click", () => act(entry.id, choice));
      row.append(button);
    }
    nodes.push(row);
  }
  for (const task of unlinked) {
    const row = document.createElement("div");
    row.className = "panel";
    const text = document.createElement("p");
    text.textContent = `${task.display_name} (${task.task_id}): Unused Manager reservation without a loaded journal record. Discarding does not restart Firefox. Up to 32 reservations are shown at a time.`;
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = "Discard unused reservation";
    button.addEventListener("click", () => act(task.task_id, "discard"));
    row.append(text, button);
    nodes.push(row);
  }
  container.replaceChildren(...nodes);
}
