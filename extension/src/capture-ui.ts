import type { CaptureState } from "./capture-control";

export function renderCaptureControl(
  input: HTMLInputElement,
  status: HTMLElement,
  state: CaptureState,
): void {
  input.indeterminate = false;
  input.checked = state.available && state.ready && state.enabled && !state.failed;
  input.disabled = !state.available || !state.ready || state.failed || state.busy;
  status.textContent = !state.available
    ? "Automatic capture is unavailable. Firefox keeps control."
    : state.failed
      ? "Capture preference could not be verified. Automatic capture is paused; the stored setting may enable it again after reload or restart."
      : !state.ready || state.busy
        ? "Verifying capture preference; new downloads remain in Firefox."
        : state.enabled
          ? "On preference for supported anonymous links; capture also requires website access and an available Manager. Unsupported downloads remain in Firefox."
          : "Off: new downloads remain in Firefox. Existing Manager transfers are unchanged.";
}
