// Readiness only. No clicks, consent, preference changes, pixels or input values.
const done = arguments[arguments.length - 1];
const expected = arguments[0];
(async () => {
  const result = (state) => ({ version: 1, state });
  const w = window;
  const { NavigableManager } = ChromeUtils.importESModule(
    "chrome://remote/content/shared/NavigableManager.sys.mjs",
  );
  const browser = w.gBrowser?.selectedBrowser;
  const valid = () =>
    typeof expected === "string" &&
    /^[\x21-\x7e]{1,128}$/u.test(expected) &&
    browser &&
    w.gBrowser.selectedBrowser === browser &&
    w.gBrowser.selectedTab?.linkedBrowser === browser &&
    browser.currentURI?.spec === "about:blank" &&
    NavigableManager.getIdForBrowser(browser) === expected;
  if (!valid()) return result("unavailable");
  if (!w.document.documentElement.hasAttribute("window-modal-open")) return result("clear");
  const dialog = w.gDialogBox?.dialog;
  const frame = dialog?._frame;
  const uri = "chrome://browser/content/spotlight.html";
  const retained = () =>
    valid() &&
    w.gDialogBox.dialog === dialog &&
    dialog?._frame === frame &&
    frame?.ownerDocument === w.document;
  if (!retained()) return result("unavailable");
  if (dialog._openedURL != null && dialog._openedURL !== uri) return result("unsupported");
  const ready = dialog._dialogReady;
  if (!ready || typeof ready.then !== "function") return result("unavailable");
  await ready;
  if (!retained() || dialog._dialogReady !== ready) return result("unavailable");
  if (!w.document.documentElement.hasAttribute("window-modal-open")) return result("clear");
  const doc = frame.contentDocument;
  if (dialog._openedURL !== uri || doc?.documentURI !== uri) return result("unsupported");
  const config = frame.contentWindow?.arguments?.[0];
  const id = config && Object.getOwnPropertyDescriptor(config, "id");
  if (
    !retained() ||
    frame.contentDocument !== doc ||
    doc.documentURI !== uri ||
    dialog._openedURL !== uri
  )
    return result("unavailable");
  return result(
    id && Object.hasOwn(id, "value") && id.value === "NEW_USER_TOU_ONBOARDING"
      ? "terms"
      : "unsupported",
  );
})().then(done, () => done({ version: 1, state: "unavailable" }));
