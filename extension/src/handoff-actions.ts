export type HandoffAction = "manager" | "firefox" | "recheck" | "acknowledge" | "discard";

/** Fixed UI warnings; acknowledgement never starts a new Firefox download. */
export function handoffPrompt(choice: Exclude<HandoffAction, "recheck">): string {
  switch (choice) {
    case "manager":
      return "Continue only if Firefox has stopped this download. If you cannot identify this download, do not continue. If Firefox is still downloading, continuing can create competing output. Continue in Manager?";
    case "firefox":
      return "Check that Firefox is handling the download. Discard only the unused Manager reservation? An already committed task will not be discarded.";
    case "acknowledge":
      return "Manager discarded this reservation and cannot complete this download. Dismiss this notice only after checking the download. If needed, restart it from the original page in Firefox. This button will not start a download.";
    case "discard":
      return "Discard this unused Manager reservation? Its status will be checked again. An already committed task will not be discarded. This does not restart a download in Firefox or erase the retained task identity.";
  }
}

export function dispatchHandoffAction(
  taskId: string,
  choice: HandoffAction,
  confirmAction: (prompt: string) => boolean,
  send: (message: object) => void,
): void {
  if (choice === "recheck") {
    send({ action: "handoff-recheck" });
    return;
  }
  if (!confirmAction(handoffPrompt(choice))) return;
  send({
    action:
      choice === "acknowledge" || choice === "discard" ? "handoff-cleanup" : "handoff-resolve",
    taskId,
    choice,
  });
}
