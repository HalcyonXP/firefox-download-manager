import type { CaptureAccessState } from "./capture-access";

/** Request permission only from the real button handler; its result is not authority. */
export function wireCaptureAccess(
  button: HTMLButtonElement,
  status: HTMLElement,
  request: () => Promise<boolean>,
  check: () => void,
): { update(state: CaptureAccessState): void; disconnect(): void } {
  let state: CaptureAccessState = {
    selected: false,
    checking: false,
    granted: false,
    failed: false,
  };
  let pending = false;
  let connected = true;
  const render = (): void => {
    button.hidden = status.hidden = !state.selected;
    button.disabled = !connected || pending || state.checking || state.granted;
    status.textContent = !connected
      ? "Website access connection lost. Reconnect to verify it."
      : pending || state.checking
        ? "Checking website access; new downloads remain in Firefox until verified."
        : state.failed
          ? "Website access could not be verified. Firefox keeps new downloads; check permissions or reload the extension."
          : state.granted
            ? "Website access verified. Automatic capture still follows its preference and Manager availability."
            : "Website access is missing. Firefox keeps new downloads even if the capture preference is On.";
    if (state.selected) status.textContent = "Development capture candidate. " + status.textContent;
  };
  button.addEventListener("click", () => {
    if (button.hidden || button.disabled || pending || !connected) return;
    pending = true;
    render();
    const settled = (): void => {
      pending = false;
      // Re-read regardless of denial, rejection or approval; do not infer a grant.
      state = { ...state, checking: true, granted: false };
      try {
        check();
      } catch {
        connected = false;
      }
      render();
    };
    try {
      void request().then(settled, settled);
    } catch {
      settled();
    }
  });
  render();
  return {
    update(value) {
      state = value;
      connected = true;
      render();
    },
    disconnect() {
      connected = false;
      render();
    },
  };
}
