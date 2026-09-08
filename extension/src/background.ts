import { collectSession, SessionError, type SessionInput } from "./session";
import { NativeConnection } from "./native-connection";
import { connectionMessage, creationPayload, directUrl, type CreationInput } from "./creation";

export const nativeConnection = new NativeConnection();
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
  port.onDisconnect.addListener(unsubscribe);
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
