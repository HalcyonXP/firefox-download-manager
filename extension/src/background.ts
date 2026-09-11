import { CaptureControl, CAPTURE_SETTING_KEY } from "./capture-control";
import { collectSession, SessionError, type SessionInput } from "./session";
import { NativeConnection } from "./native-connection";
import { connectionMessage, creationPayload, directUrl, type CreationInput } from "./creation";

import { BrowserHandoff } from "./browser-handoff";
import { HandoffJournal, HANDOFF_STORAGE_KEY } from "./handoff-journal";

export const captureControl = new CaptureControl({
  read: async () =>
    (await browser.storage.local.get(CAPTURE_SETTING_KEY))[CAPTURE_SETTING_KEY] as unknown,
  write: async (value) => {
    await browser.storage.local.set({ [CAPTURE_SETTING_KEY]: value });
  },
});
void captureControl.ready().catch(() => {});
export const nativeConnection = new NativeConnection();
export const browserHandoff = new BrowserHandoff(
  new HandoffJournal({
    read: async () =>
      (await browser.storage.local.get(HANDOFF_STORAGE_KEY))[HANDOFF_STORAGE_KEY] as unknown,
    write: async (value) => {
      await browser.storage.local.set({ [HANDOFF_STORAGE_KEY]: value });
    },
  }),
  {
    ready: async () => {
      await nativeConnection.connect();
      if (!nativeConnection.supports("prepared_handoff"))
        throw new Error("Handoff capability unavailable");
    },
    command: (command, payload) => nativeConnection.command(command, payload),
  },
);
browserHandoff.subscribe((view) => {
  void browser.action
    .setBadgeText({ text: view.blocked || view.pending.length ? "!" : "" })
    .catch(() => {});
});
// Resume only already recorded intent. No click interceptor is selected here yet.
void browserHandoff.recover().catch(() => {});

const captures = new Map<string, { url: string; expires: number; tabId: number | undefined }>();
const menuId = "download-with-manager";

async function openManager(url?: string, tabId?: number): Promise<void> {
  const now = Date.now();
  for (const [key, value] of captures) if (value.expires < now) captures.delete(key);
  let fragment = "";
  if (url !== undefined) {
    directUrl(url);
    if (captures.size >= 32) captures.delete(captures.keys().next().value!);
    fragment = crypto.randomUUID();
    captures.set(fragment, { url, expires: now + 60_000, tabId });
  }
  await browser.tabs.create({ url: browser.runtime.getURL(`manager.html#${fragment}`) });
}

browser.runtime.onInstalled.addListener(() => {
  void browser.menus.removeAll().then(() =>
    browser.menus.create({
      id: menuId,
      title: "Download with Manager",
      contexts: ["link"],
      targetUrlPatterns: ["http://*/*", "https://*/*"],
    }),
  );
});
browser.action.onClicked.addListener(() => {
  void openManager();
});
browser.menus.onClicked.addListener((info, tab) => {
  // No page URL/referrer, content scraping, or built-in download cancellation.
  if (info.menuItemId === menuId && info.linkUrl)
    void openManager(info.linkUrl, tab?.id).catch(() => openManager());
});

browser.runtime.onConnect.addListener((port) => {
  if (
    port.name !== "manager-ui" ||
    port.sender?.id !== browser.runtime.id ||
    port.sender.url?.split("#")[0] !== browser.runtime.getURL("manager.html")
  ) {
    port.disconnect();
    return;
  }
  const send = (value: unknown): void => {
    try {
      port.postMessage(value);
    } catch {
      /* UI closed; helper remains authoritative. */
    }
  };
  const unsubscribe = nativeConnection.subscribe((state) => send({ kind: "state", state }));
  const unsubscribeHandoffs = browserHandoff.subscribe((view) => send({ kind: "handoffs", view }));
  const unsubscribeCapture = captureControl.subscribe((state) =>
    send({ kind: "capture-state", state }),
  );
  port.onDisconnect.addListener(() => {
    unsubscribeCapture();
    unsubscribe();
    unsubscribeHandoffs();
  });
  let busy = false;
  let sessionTabId = port.sender.tab?.id;
  port.onMessage.addListener((message: unknown) => {
    if (typeof message !== "object" || message === null || !("action" in message)) return;
    if (message.action === "capture" && "key" in message && typeof message.key === "string") {
      const captured = captures.get(message.key);
      captures.delete(message.key);
      if (captured && captured.expires >= Date.now()) sessionTabId = captured.tabId;
      send({
        kind: "capture",
        url: captured && captured.expires >= Date.now() ? captured.url : "",
      });
      return;
    }
    if (
      message.action === "capture-setting" &&
      Object.keys(message).sort().join(",") === "action,enabled" &&
      "enabled" in message &&
      typeof message.enabled === "boolean"
    ) {
      // Independent preference queue: a pending native action must not delay Off.
      void captureControl.setEnabled(message.enabled).catch(() =>
        send({
          kind: "error",
          message:
            "Capture preference was not verified. Capture is paused now; recheck its stored setting after reload or restart.",
        }),
      );
      return;
    }
    if (busy) {
      send({
        kind: "error",
        message:
          "Another action is still pending. This new action was not submitted; wait for completion before retrying.",
      });
      return;
    }
    busy = true;
    void (async () => {
      try {
        if (message.action === "connect") {
          await nativeConnection.connect();
          await nativeConnection.command("get_settings", {});
          await browserHandoff.recover();
        } else if (message.action === "handoff-recheck") {
          await browserHandoff.recover();
        } else if (
          message.action === "handoff-resolve" &&
          "taskId" in message &&
          typeof message.taskId === "string" &&
          "choice" in message &&
          (message.choice === "manager" || message.choice === "firefox")
        ) {
          await browserHandoff.resolveIntent(message.taskId, message.choice);
          send({
            kind: "notice",
            message:
              "The recorded handoff was resolved. Check the queue before starting another download.",
          });
        } else if (
          message.action === "handoff-cleanup" &&
          Object.keys(message).sort().join(",") === "action,choice,taskId" &&
          "taskId" in message &&
          typeof message.taskId === "string" &&
          "choice" in message &&
          (message.choice === "acknowledge" || message.choice === "discard")
        ) {
          if (message.choice === "acknowledge")
            await browserHandoff.acknowledgeAborted(message.taskId);
          else await browserHandoff.discardUnlinked(message.taskId);
          send({
            kind: "notice",
            message:
              "Reservation cleanup confirmed. No download was started. Use the original page in Firefox if you need to download again.",
          });
        } else if (message.action === "settings" && "patch" in message) {
          await nativeConnection.command("update_settings", { settings: message.patch });
          send({
            kind: "notice",
            message: "Settings saved and applied. Existing tasks keep their worker selection.",
          });
        } else if (
          message.action === "control" &&
          "command" in message &&
          "taskId" in message &&
          typeof message.taskId === "string"
        ) {
          const payload = { task_id: message.taskId };
          switch (message.command) {
            case "pause":
            case "resume":
              await nativeConnection.command(message.command, payload);
              break;
            case "cancel":
              await nativeConnection.command("cancel", {
                ...payload,
                partial_policy:
                  nativeConnection.state().settings?.keep_partial_on_cancel === false
                    ? "delete"
                    : "keep",
              });
              break;
            case "remove":
              await nativeConnection.command("remove", { ...payload, delete_partial: true });
              break;
            case "open_folder":
              await nativeConnection.command("open_folder", payload);
              break;
          }
        } else if (message.action === "add" && "input" in message) {
          const input = message.input as CreationInput;
          if (
            typeof input?.url !== "string" ||
            typeof input.destination !== "string" ||
            typeof input.filename !== "string" ||
            typeof input.workers !== "number" ||
            (input.checksum !== undefined && typeof input.checksum !== "string")
          )
            throw new Error("invalid input");
          const session = "session" in message ? (message.session as SessionInput) : undefined;
          if (
            session &&
            (typeof session.enabled !== "boolean" ||
              typeof session.referrer !== "string" ||
              typeof session.authorization !== "string")
          )
            throw new SessionError();
          const currentTabId =
            "sessionTabId" in message &&
            typeof message.sessionTabId === "number" &&
            Number.isSafeInteger(message.sessionTabId)
              ? message.sessionTabId
              : undefined;
          const requestContext = session
            ? await collectSession(input.url, session, sessionTabId ?? currentTabId)
            : undefined;
          const task = await nativeConnection.command("add", {
            ...creationPayload(input),
            ...(requestContext ? { request_context: requestContext } : {}),
          });
          send({ kind: "added", task });
        }
      } catch (error) {
        send({
          kind: "error",
          message: error instanceof SessionError ? error.message : connectionMessage(error),
        });
      } finally {
        busy = false;
        send({ kind: "idle" });
      }
    })();
  });
});
