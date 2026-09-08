import { creationPayload, suggestedFilename } from "./creation";
import type { NativeState, NativeTask } from "./native-connection";

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

function attach(): void {
  port = browser.runtime.connect({ name: "manager-ui" });
  port.onMessage.addListener((raw: object) => {
    const message = raw as {
      kind: string;
      state?: NativeState;
      url?: string;
      message?: string;
      task?: NativeTask;
    };
    if (message.kind === "capture" && message.url) {
      url.value = message.url;
      filename.value = suggestedFilename(url.value);
    }
    if (message.kind === "state" && message.state) {
      element("connection").textContent = message.state.connected
        ? "Helper connected"
        : "Helper disconnected · showing last snapshot";
      element("queue-summary").textContent = `${message.state.tasks.length} download(s)`;
      const rows = message.state.tasks.map((task) => {
        const row = document.createElement("p");
        row.textContent = `${task.display_name} — ${task.state} — ${task.bytes_completed} bytes`;
        return row;
      });
      element("tasks").replaceChildren(...rows);
    }
    if (message.kind === "error") feedback.textContent = message.message ?? "Action failed.";
    if (message.kind === "added" && message.task) {
      feedback.textContent = `Added ${message.task.display_name}. The helper now owns this download.`;
      url.value = "";
    }
    if (message.kind === "idle") submit.disabled = false;
  });
  port.onDisconnect.addListener(() => {
    feedback.textContent =
      "The extension connection closed. Reconnect and check the queue before submitting again.";
    submit.disabled = false;
  });
  port.postMessage({ action: "capture", key: location.hash.slice(1) });
  history.replaceState(null, "", location.pathname);
  port.postMessage({ action: "connect" });
}
url.addEventListener("change", () => {
  filename.value = suggestedFilename(url.value);
});
form.addEventListener("submit", (event) => {
  event.preventDefault();
  const input = {
    url: url.value,
    destination: destination.value,
    filename: filename.value,
    workers: Number(workers.value),
  };
  try {
    creationPayload(input);
    submit.disabled = true;
    feedback.textContent = "Creating download…";
    port.postMessage({ action: "add", input });
  } catch (error) {
    feedback.textContent = error instanceof Error ? error.message : "Invalid download.";
    submit.disabled = false;
  }
});
element("reconnect").addEventListener("click", () => {
  port.disconnect();
  attach();
});
attach();
