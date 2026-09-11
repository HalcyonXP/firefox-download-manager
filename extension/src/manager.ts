import { renderHandoffs } from "./handoff-ui";
import type { HandoffView } from "./browser-handoff";
import { sessionPermission, SessionError } from "./session";
import { creationPayload, suggestedFilename } from "./creation";
import type { NativeState, NativeTask, NativeSettings } from "./native-connection";
import { Dashboard } from "./dashboard";

function element<T extends HTMLElement>(id: string): T {
  return document.getElementById(id) as T;
}
const form = element<HTMLFormElement>("create");
const url = element<HTMLInputElement>("url");
const filename = element<HTMLInputElement>("filename");
const destination = element<HTMLInputElement>("destination");
const workers = element<HTMLSelectElement>("workers");
const feedback = element("feedback");
const submit = element<HTMLButtonElement>("submit");
let port: browser.runtime.Port;
let effectiveSettings: NativeSettings | undefined;
let settingsKey = "";
let handoffView: HandoffView = { blocked: false, pending: [] };
let handoffTasks: readonly NativeTask[] = [];
const dashboard = new Dashboard(element("tasks"), (action, task) => {
  if (
    action === "remove" &&
    !confirm(
      "Remove this task history and any retained partial? Completed downloads will not be deleted.",
    )
  )
    return;
  if (
    action === "cancel" &&
    !confirm(
      effectiveSettings?.keep_partial_on_cancel === false
        ? "Cancel and delete this partial?"
        : "Cancel this download? Partial bytes will be retained until you remove the task.",
    )
  )
    return;
  dashboard.busy(true);
  port.postMessage({ action: "control", command: action, taskId: task.task_id });
});

function renderPendingHandoffs(): void {
  renderHandoffs(
    element("handoffs"),
    handoffView,
    (taskId, choice) => {
      if (choice === "recheck") {
        port.postMessage({ action: "handoff-recheck" });
        return;
      }
      if (
        !confirm(
          choice === "manager"
            ? "Continue only if Firefox has stopped this download. If you cannot identify this download, do not continue. If Firefox is still downloading, continuing can create competing output. Continue in Manager?"
            : "Check that Firefox is handling the download. Discard only the unused Manager reservation? An already committed task will not be discarded.",
        )
      )
        return;
      port.postMessage({ action: "handoff-resolve", taskId, choice });
    },
    handoffTasks,
  );
}

function attach(): void {
  port = browser.runtime.connect({ name: "manager-ui" });
  port.onMessage.addListener((raw: object) => {
    const message = raw as {
      kind: string;
      state?: NativeState;
      url?: string;
      message?: string;
      task?: NativeTask;
      view?: HandoffView;
    };
    if (message.kind === "handoffs" && message.view) {
      handoffView = message.view;
      renderPendingHandoffs();
    }
    if (message.kind === "capture" && message.url) {
      url.value = message.url;
      filename.value = suggestedFilename(url.value);
    }
    if (message.kind === "state" && message.state) {
      element("connection").textContent = message.state.connected
        ? "Helper connected"
        : "Helper disconnected · showing last snapshot";
      element("queue-summary").textContent = `${message.state.tasks.length} download(s)`;
      dashboard.update(message.state);
      handoffTasks = message.state.tasks;
      renderPendingHandoffs();
      if (message.state.settings && JSON.stringify(message.state.settings) !== settingsKey) {
        effectiveSettings = message.state.settings;
        settingsKey = JSON.stringify(effectiveSettings);
        renderSettings(effectiveSettings);
      }
    }
    if (message.kind === "error" || message.kind === "notice")
      feedback.textContent = message.message ?? "Action failed.";
    if (message.kind === "added" && message.task) {
      feedback.textContent = `Added ${message.task.display_name}. The helper now owns this download.`;
      url.value = "";
      element<HTMLInputElement>("checksum").value = "";
    }
    if (message.kind === "idle") {
      submit.disabled = false;
      dashboard.busy(false);
      element<HTMLButtonElement>("save-settings").disabled = false;
    }
  });
  port.onDisconnect.addListener(() => {
    feedback.textContent =
      "The extension connection closed. Reconnect and check the queue before submitting again.";
    submit.disabled = false;
    dashboard.busy(false);
  });
  port.postMessage({ action: "capture", key: location.hash.slice(1) });
  history.replaceState(null, "", location.pathname);
  port.postMessage({ action: "connect" });
}
url.addEventListener("change", () => {
  filename.value = suggestedFilename(url.value);
});
form.addEventListener("submit", (event) => {
  void submitDownload(event);
});
async function submitDownload(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  const input = {
    url: url.value,
    destination: destination.value,
    filename: filename.value,
    workers: Number(workers.value),
    checksum: element<HTMLInputElement>("checksum").value,
  };
  try {
    creationPayload(input);
    submit.disabled = true;
    feedback.textContent = "Creating download…";
    const session = {
      enabled: element<HTMLInputElement>("session-enabled").checked,
      referrer: element<HTMLInputElement>("session-referrer").value,
      authorization: element<HTMLInputElement>("session-authorization").value,
    };
    if (
      session.enabled &&
      !(await browser.permissions.request({
        permissions: ["cookies"],
        origins: [sessionPermission(input.url)],
      }))
    )
      throw new SessionError();
    const sessionTabId = session.enabled ? (await browser.tabs.getCurrent())?.id : undefined;
    port.postMessage({ action: "add", input, session, sessionTabId });
    clearSession();
  } catch (error) {
    feedback.textContent =
      error instanceof SessionError
        ? error.message
        : "Download was not submitted. Check the form and optional session permission.";
    submit.disabled = false;
    clearSession();
  }
}
function clearSession(): void {
  element<HTMLInputElement>("session-enabled").checked = false;
  element<HTMLInputElement>("session-referrer").value = "";
  element<HTMLInputElement>("session-authorization").value = "";
}
element("revoke-session").addEventListener("click", () => {
  void browser.permissions
    .getAll()
    .then(async (permissions) => {
      const origins = (permissions.origins ?? []).filter((origin) => /^https?:/u.test(origin));
      await browser.permissions.remove({ permissions: ["cookies"], origins });
      clearSession();
      feedback.textContent =
        "Optional permissions revoked. Already-submitted tasks retain their in-memory context; cancel them separately.";
    })
    .catch(() => {
      feedback.textContent =
        "Could not revoke permissions. Use Firefox's extension permissions page.";
    });
});
element("reconnect").addEventListener("click", () => {
  port.disconnect();
  attach();
});
attach();

function renderSettings(settings: NativeSettings): void {
  element<HTMLInputElement>("setting-destination").value = settings.destination;
  element<HTMLSelectElement>("setting-workers").value = String(settings.default_workers);
  element<HTMLInputElement>("setting-global").value = String(settings.global_concurrency);
  element<HTMLInputElement>("setting-host").value = String(settings.per_host_concurrency);
  element<HTMLInputElement>("setting-retry").value = String(settings.retry_limit);
  element<HTMLInputElement>("setting-cancel").checked = settings.keep_partial_on_cancel;
  element<HTMLInputElement>("setting-failure").checked = settings.keep_partial_on_failure;
  element<HTMLInputElement>("setting-verbose").checked = settings.verbose_logging;
  if (!destination.value) destination.value = settings.destination;
  workers.value = String(settings.default_workers);
}
element<HTMLFormElement>("settings-form").addEventListener("submit", (event) => {
  event.preventDefault();
  element<HTMLButtonElement>("save-settings").disabled = true;
  port.postMessage({
    action: "settings",
    patch: {
      destination: element<HTMLInputElement>("setting-destination").value,
      default_workers: Number(element<HTMLSelectElement>("setting-workers").value),
      global_concurrency: Number(element<HTMLInputElement>("setting-global").value),
      per_host_concurrency: Number(element<HTMLInputElement>("setting-host").value),
      retry_limit: Number(element<HTMLInputElement>("setting-retry").value),
      keep_partial_on_cancel: element<HTMLInputElement>("setting-cancel").checked,
      keep_partial_on_failure: element<HTMLInputElement>("setting-failure").checked,
      verbose_logging: element<HTMLInputElement>("setting-verbose").checked,
    },
  });
});
