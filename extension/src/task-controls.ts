import type { TaskHandoffPhase } from "./native-connection";

export type TaskAction = "pause" | "resume" | "cancel" | "remove" | "open_folder";
export function actionsFor(
  state: string,
  phase: TaskHandoffPhase = null,
): Array<{ action: TaskAction; label: string }> {
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
  return actions.filter(({ action }) => handoffAllows(phase, action));
}

/** Presentation and dispatch share phase restrictions; the engine remains authoritative. */
export function handoffAllows(phase: TaskHandoffPhase | undefined, action: string): boolean {
  if (action === "open_folder") return true;
  if (phase === "prepared" || phase === "aborted" || phase === "unknown") return false;
  return phase !== "committed" || action !== "remove";
}
