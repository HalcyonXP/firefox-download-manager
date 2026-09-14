/* Test-only loopback API probe, never a product interceptor or native handoff. */
const records = [];
const requests = new Map();
const clicks = [];
let sequence = 0;
let armed = false;
let overflow = false;
const limit = 64;
const categories = new Set([
  "page",
  "direct",
  "redirect",
  "attachment",
  "post",
  "frame",
  "navigation",
]);

function category(url) {
  try {
    const parsed = new URL(url);
    return parsed.protocol === "http:" &&
      parsed.hostname === "127.0.0.1" &&
      categories.has(parsed.pathname.slice(1))
      ? parsed.pathname.slice(1)
      : "other";
  } catch {
    return "other";
  }
}
function record(value) {
  if (records.length >= limit) overflow = true;
  else records.push(value);
}
function request(details) {
  let value = requests.get(details.requestId);
  if (!value && requests.size < limit) {
    value = { id: ++sequence, initial: details.url, correlated: false };
    requests.set(details.requestId, value);
  }
  if (!value) overflow = true;
  return value;
}
function summarize(details, stage, extra = {}) {
  const value = request(details);
  record({
    stage,
    request: value?.id ?? 0,
    route: category(details.url),
    method: details.method === "GET" || details.method === "POST" ? details.method : "other",
    type: details.type === "main_frame" || details.type === "sub_frame" ? details.type : "other",
    topFrame: details.frameId === 0,
    private: typeof details.incognito === "boolean" ? details.incognito : "missing",
    store:
      details.cookieStoreId === "firefox-default"
        ? "default"
        : typeof details.cookieStoreId === "string"
          ? "other"
          : "missing",
    document: category(details.documentUrl),
    origin: category(details.originUrl),
    ...extra,
  });
}

browser.runtime.onMessage.addListener((message, sender) => {
  if (sender.id !== browser.runtime.id) return undefined;
  if (sender.url === browser.runtime.getURL("inspect.html")) {
    if (message?.action === "reset") {
      records.length = 0;
      requests.clear();
      clicks.length = 0;
      sequence = 0;
      overflow = false;
      armed = message.cancel === true;
      return Promise.resolve({ ready: true });
    }
    if (message?.action === "snapshot")
      return Promise.resolve({ records: records.slice(), overflow });
    return undefined;
  }
  if (
    message?.action !== "click" ||
    !sender.tab ||
    sender.frameId !== 0 ||
    typeof message.target !== "string" ||
    message.target.length > 2048 ||
    typeof message.trusted !== "boolean" ||
    category(sender.url) !== "page"
  )
    return undefined;
  if (clicks.length >= 8) {
    overflow = true;
    return undefined;
  }
  clicks.push({
    tab: sender.tab.id,
    document: sender.url,
    target: message.target,
    trusted: message.trusted,
    time: Date.now(),
  });
  record({ stage: "click", route: category(message.target), trusted: message.trusted });
  return undefined;
});
const filter = { urls: ["http://127.0.0.1/*"] };
browser.webRequest.onBeforeRequest.addListener((details) => {
  request(details);
  summarize(details, "before");
}, filter);
browser.webRequest.onHeadersReceived.addListener(
  (details) => {
    const value = request(details);
    const clicked =
      value &&
      clicks.find(
        (click) =>
          click.tab === details.tabId &&
          click.document === details.originUrl &&
          click.target === value.initial &&
          click.trusted &&
          !click.used &&
          Date.now() - click.time <= 5000,
      );
    if (
      value &&
      clicked &&
      details.method === "GET" &&
      details.frameId === 0 &&
      details.type === "main_frame" &&
      details.incognito === false &&
      details.cookieStoreId === "firefox-default"
    ) {
      clicked.used = true; // One observed click cannot authorize another request ID.
      value.correlated = true;
    }
    const attachment = (details.responseHeaders ?? []).some(
      (header) =>
        header.name.toLowerCase() === "content-disposition" &&
        /^attachment(?:;|$)/i.test(header.value ?? ""),
    );
    const cancel =
      armed &&
      !overflow &&
      !!value?.correlated &&
      attachment &&
      details.statusCode === 200 &&
      details.method === "GET" &&
      details.type === "main_frame" &&
      details.frameId === 0 &&
      details.incognito === false &&
      details.cookieStoreId === "firefox-default";
    summarize(details, "headers", {
      status: details.statusCode,
      attachment,
      correlated: value?.correlated ?? false,
      cancel,
    });
    // Diagnostic cancellation only: no task is created and no content is replayed.
    if (cancel) return new Promise((resolve) => setTimeout(() => resolve({ cancel: true }), 50));
    return {};
  },
  filter,
  ["blocking", "responseHeaders"],
);
browser.webRequest.onBeforeRedirect.addListener((details) => {
  summarize(details, "redirect", { target: category(details.redirectUrl) });
}, filter);
browser.webRequest.onCompleted.addListener((details) => summarize(details, "completed"), filter);
browser.webRequest.onErrorOccurred.addListener(
  (details) =>
    summarize(details, "error", {
      errorKind: ["NS_BINDING_ABORTED", "NS_ERROR_ABORT", "NS_ERROR_BLOCKED_BY_POLICY"].includes(
        details.error,
      )
        ? details.error
        : "other",
    }),
  filter,
);
