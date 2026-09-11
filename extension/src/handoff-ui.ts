import type { HandoffView } from "./browser-handoff";
import type { HandoffStage, PendingHandoff } from "./handoff-journal";
import type { NativeTask } from "./native-connection";

type TaskLabel = Pick<NativeTask, "task_id" | "display_name">;
export function handoffLabel(entry: PendingHandoff, tasks: readonly TaskLabel[]): string {
  const task = tasks.find((value) => value.task_id === entry.id);
  return `${task?.display_name ?? "Task details unavailable"} (${entry.id}): ${handoffStageText[entry.stage]}`;
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
  act: (id: string, choice: "manager" | "firefox" | "recheck") => void,
  tasks: readonly TaskLabel[] = [],
): void {
  container.hidden = !view.blocked && view.pending.length === 0;
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
    text.textContent = handoffLabel(entry, tasks);
    row.append(text);
    const choices =
      entry.stage === "intent"
        ? (["manager", "firefox", "recheck"] as const)
        : (["recheck"] as const);
    for (const choice of choices) {
      const button = document.createElement("button");
      button.type = "button";
      button.disabled =
        view.blocked || (choice === "manager" && !tasks.some((task) => task.task_id === entry.id));
      button.textContent =
        choice === "manager"
          ? "I checked Firefox stopped — continue in Manager"
          : choice === "firefox"
            ? "Keep in Firefox; discard unused reservation"
            : "Recheck Manager";
      button.addEventListener("click", () => act(entry.id, choice));
      row.append(button);
    }
    nodes.push(row);
  }
  container.replaceChildren(...nodes);
}
