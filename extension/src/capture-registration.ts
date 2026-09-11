import type { BrowserHandoff } from "./browser-handoff";
import { CapturePolicy } from "./capture-policy";

/** Requires separately reviewed/granted webRequest, blocking and site authority.
 * Not called by the production background until browser/native integration passes.
 */
export function registerCapture(
  handoff: Pick<BrowserHandoff, "capture" | "terminal">,
  enabled: () => boolean,
  urls: string[] = ["http://*/*", "https://*/*"],
): void {
  const policy = new CapturePolicy(handoff, enabled);
  const filter: browser.webRequest.RequestFilter = {
    urls,
    types: ["main_frame"],
  };
  browser.runtime.onMessage.addListener((message: unknown, sender) => {
    if (
      !message ||
      typeof message !== "object" ||
      Object.keys(message).sort().join(",") !== "action,target,trusted" ||
      !("action" in message) ||
      message.action !== "ordinary-download-click" ||
      !("target" in message) ||
      typeof message.target !== "string" ||
      !("trusted" in message) ||
      message.trusted !== true ||
      sender.id !== browser.runtime.id ||
      sender.frameId !== 0 ||
      sender.tab?.incognito !== false ||
      sender.tab.id === undefined ||
      typeof sender.url !== "string"
    )
      return undefined;
    policy.click(message.target, sender.url, sender.tab.id, true);
    return undefined;
  });
  browser.webRequest.onBeforeRequest.addListener((details) => policy.before(details), filter);
  browser.webRequest.onBeforeSendHeaders.addListener(
    (details) => {
      policy.sent(details, details.requestHeaders);
    },
    filter,
    ["requestHeaders"],
  );
  browser.webRequest.onBeforeRedirect.addListener((details) => {
    policy.redirect(details, details.redirectUrl);
  }, filter);
  browser.webRequest.onHeadersReceived.addListener(
    (details) => {
      try {
        return policy
          .headers(details, details.statusCode, details.responseHeaders)
          .catch(() => ({}));
      } catch {
        return {};
      }
    },
    filter,
    ["blocking", "responseHeaders"],
  );
  browser.webRequest.onCompleted.addListener((details) => policy.terminal(details), filter);
  browser.webRequest.onErrorOccurred.addListener(
    (details) => policy.terminal(details, details.error),
    filter,
  );
}
