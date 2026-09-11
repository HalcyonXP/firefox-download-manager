// Unselected loopback diagnostic entry. Never a production build entry or permission grant.
import { browserHandoff, nativeConnection } from "../src/background";
import { registerCapture } from "../src/capture-registration";

let enabled = false;
let destinationVerified = false;
let suppressTerminal = false;
let terminalSuppressed = false;
let fixtureOrigin: string | undefined;
let overflow = false;
const keys = new Map<string, number>();
const records: { request: number; stage: "decision" | "terminal"; cancelled: boolean }[] = [];
function record(request: number, stage: "decision" | "terminal", cancelled: boolean): void {
  if (records.length >= 64) overflow = true;
  else records.push({ request, stage, cancelled });
}
registerCapture(
  {
    capture: async (key, input, eligible) => {
      // Defense in depth: diagnostic authority never reaches a non-loopback origin.
      const url = new URL(input.url);
      if (
        !enabled ||
        !destinationVerified ||
        url.protocol !== "http:" ||
        url.hostname !== "127.0.0.1" ||
        url.origin !== fixtureOrigin ||
        keys.size >= 32
      )
        return {};
      const request = keys.size + 1;
      keys.set(key, request);
      const decision = await browserHandoff.capture(key, input, eligible);
      record(request, "decision", decision.cancel === true);
      return decision;
    },
    terminal: (key, error) => {
      const request = keys.get(key);
      if (request !== undefined) record(request, "terminal", error === "NS_ERROR_ABORT");
      if (suppressTerminal && request !== undefined) {
        terminalSuppressed = true;
        return; // Deliberate diagnostic loss after the actual browser event.
      }
      browserHandoff.terminal(key, error);
    },
  },
  () => enabled && !overflow,
  ["http://127.0.0.1/*"],
);
function snapshot() {
  const state = nativeConnection.state();
  const handoff = browserHandoff.view();
  return {
    qualification: false,
    connected: state.connected,
    enabled,
    overflow,
    terminalSuppressed,
    records: records.slice(),
    blocked: handoff.blocked,
    pending: handoff.pending.map((entry) => entry.stage),
    tasks: state.tasks
      .slice(0, 32)
      .map((task) => ({ state: task.state, bytes: task.bytes_completed })),
    taskCount: state.tasks.length,
  };
}
browser.runtime.onMessage.addListener((message: unknown, sender) => {
  if (
    sender.id !== browser.runtime.id ||
    sender.url !== browser.runtime.getURL("inspect.html") ||
    typeof message !== "object" ||
    message === null ||
    !("action" in message)
  )
    return undefined;
  const keys = Object.keys(message).sort().join(",");
  if (keys !== (message.action === "ready" ? "action,destination,origin" : "action"))
    return undefined;
  if (message.action === "snapshot") return Promise.resolve(snapshot());
  if (message.action === "arm" || message.action === "arm-missing-terminal") {
    if (
      !destinationVerified ||
      !nativeConnection.supports("prepared_handoff") ||
      browserHandoff.view().blocked ||
      browserHandoff.view().pending.length
    )
      return Promise.resolve(null);
    suppressTerminal = message.action === "arm-missing-terminal";
    enabled = true;
    return Promise.resolve(snapshot());
  }
  if (message.action === "off") {
    enabled = false;
    return Promise.resolve(snapshot());
  }
  if (message.action === "ready")
    return (async () => {
      await nativeConnection.connect();
      await nativeConnection.command("get_settings", {});
      await browserHandoff.recover();
      fixtureOrigin = undefined;
      if ("origin" in message && typeof message.origin === "string") {
        try {
          const url = new URL(message.origin);
          if (
            url.protocol === "http:" &&
            url.hostname === "127.0.0.1" &&
            url.origin === message.origin
          )
            fixtureOrigin = url.origin;
        } catch {
          /* No fixture authority. */
        }
      }
      destinationVerified =
        fixtureOrigin !== undefined &&
        "destination" in message &&
        typeof message.destination === "string" &&
        nativeConnection.state().settings?.destination === message.destination;
      return { ...snapshot(), destinationVerified };
    })();
  return undefined;
});
