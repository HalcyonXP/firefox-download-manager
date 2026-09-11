// Explicit capture-candidate entry, not the default/manual package entry.
import { browserHandoff, captureAccess, captureControl, nativeConnection } from "./background";
import { candidateOriginAllowed } from "./capture-protection";
import { registerCapture } from "./capture-registration";

captureAccess.start();
captureControl.activate((preferenceEnabled) =>
  registerCapture(
    browserHandoff,
    () =>
      preferenceEnabled() &&
      captureAccess.allowed() &&
      nativeConnection.state().connected &&
      nativeConnection.supports("prepared_handoff") &&
      nativeConnection.supports("task_handoff_phase"),
    ["http://*/*", "https://*/*"],
    { crossOriginRedirects: true, originAllowed: candidateOriginAllowed },
  ),
);
