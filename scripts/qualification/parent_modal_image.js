// Private diagnostic pixels from one original, non-input common dialog only.
// Never capture the desktop, dismiss a prompt, read input values or change prefs.
const done = arguments[arguments.length - 1];
const expected = arguments[0];
(async () => {
  const unavailable = {
    version: 2,
    kind: "unavailable",
    prompt: "unknown",
    message: "unknown",
    png: null,
  };
  if (typeof expected !== "string" || !/^[\x21-\x7e]{1,128}$/u.test(expected)) return unavailable;
  const w = window;
  const { NavigableManager } = ChromeUtils.importESModule(
    "chrome://remote/content/shared/NavigableManager.sys.mjs",
  );
  const original = w.gBrowser?.selectedBrowser;
  const validWindow = () =>
    original &&
    w.gBrowser.selectedBrowser === original &&
    original.currentURI?.spec === "about:blank" &&
    NavigableManager.getIdForBrowser(original) === expected &&
    w.document.documentElement.hasAttribute("window-modal-open");
  if (!validWindow()) return unavailable;
  const dialog = w.gDialogBox?.dialog;
  if (!dialog) return { ...unavailable, kind: "none" };
  const uri = "chrome://global/content/commonDialog.xhtml";
  const spotlight = "chrome://browser/content/spotlight.html";
  const initial = dialog._openedURL;
  if (initial != null && initial !== uri && initial !== spotlight)
    return { ...unavailable, kind: "other" };
  const frame = dialog._frame;
  const retained = () =>
    validWindow() &&
    w.gDialogBox.dialog === dialog &&
    dialog._frame === frame &&
    frame?.ownerDocument === w.document;
  if (!retained()) return unavailable;
  // SubDialog sets its URL only AFTER awaiting frame creation. Await the exact
  // original readiness promise, within the existing Marionette script deadline.
  const ready = dialog._dialogReady;
  if (!ready || typeof ready.then !== "function") return unavailable;
  await ready;
  if (!retained()) return unavailable;
  const opened = dialog._openedURL;
  if (initial != null && opened !== initial) return unavailable;
  if (opened !== uri && opened !== spotlight) return { ...unavailable, kind: "other" };
  const doc = frame.contentDocument;
  if (!doc || doc.documentURI !== opened) return unavailable;
  if (opened === spotlight) {
    // These are fixed built-in configuration identities, not dialog text, form
    // values, telemetry, arbitrary message IDs or an action/consent interface.
    const config = frame.contentWindow?.arguments?.[0];
    const id = config && Object.getOwnPropertyDescriptor(config, "id");
    const messages = new Map([
      ["NEW_USER_TOU_ONBOARDING", "new-user-terms"],
      ["PRE_ONBOARDING_SPLASH", "startup-splash"],
      ["AI_WINDOW_TOU_EXISTING_USERS_MODAL", "ai-window-terms"],
      ["LOGIN_STATUS_ADVISORY", "login-advisory"],
      ["BROWSER_BACKUP_OPTIN_SPOTLIGHT", "backup-optin"],
      ["FX_MR_106_UPGRADE", "upgrade"],
    ]);
    const message =
      id && Object.hasOwn(id, "value") ? (messages.get(id.value) ?? "unknown") : "unknown";
    if (
      !retained() ||
      frame.contentDocument !== doc ||
      doc.documentURI !== spotlight ||
      dialog._openedURL !== spotlight
    )
      return unavailable;
    return { ...unavailable, kind: "spotlight", message };
  }
  const type = doc.getElementById("commonDialog")?.getAttribute("windowtype");
  const types = new Map([
    ["prompt:alert", "alert"],
    ["prompt:alertCheck", "alertCheck"],
    ["prompt:confirm", "confirm"],
    ["prompt:confirmCheck", "confirmCheck"],
    ["prompt:confirmEx", "confirmEx"],
    ["prompt:prompt", "prompt"],
    ["prompt:promptUserAndPass", "promptUserAndPass"],
    ["prompt:promptPassword", "promptPassword"],
  ]);
  const prompt = types.get(type) ?? "unknown";
  const result = { ...unavailable, kind: "common", prompt };
  // Authentication/input/unknown dialogs get a fixed class only, never pixels.
  if (!["alert", "alertCheck", "confirm", "confirmCheck", "confirmEx"].includes(prompt)) {
    return result;
  }
  const validDialog = () =>
    validWindow() &&
    w.gDialogBox.dialog === dialog &&
    dialog._frame === frame &&
    frame.ownerDocument === w.document &&
    frame.contentDocument === doc &&
    doc.documentURI === uri &&
    doc.getElementById("commonDialog")?.getAttribute("windowtype") === type &&
    doc.getElementById("loginContainer")?.hidden === true &&
    doc.getElementById("password1Container")?.hidden === true;
  if (!validDialog()) return result;
  const r = frame.getBoundingClientRect();
  const values = [r.x, r.y, r.width, r.height, w.innerWidth, w.innerHeight];
  if (
    !values.every(Number.isFinite) ||
    r.x < 0 ||
    r.y < 0 ||
    r.width < 1 ||
    r.height < 1 ||
    r.width > 1600 ||
    r.height > 1200 ||
    r.x + r.width > w.innerWidth ||
    r.y + r.height > w.innerHeight
  )
    return result;
  let bitmap = null;
  let canvas = null;
  try {
    // DOM snapshot of the fixed iframe rectangle, not compositor/desktop readback.
    bitmap = await w.browsingContext.currentWindowGlobal.drawSnapshot(
      new DOMRect(r.x, r.y, r.width, r.height),
      1,
      "rgb(255,255,255)",
      { drawView: true },
    );
    const after = frame.getBoundingClientRect();
    if (!validDialog() || ["x", "y", "width", "height"].some((key) => after[key] !== r[key]))
      return result;
    canvas = w.document.createElementNS("http://www.w3.org/1999/xhtml", "canvas");
    canvas.width = Math.ceil(r.width);
    canvas.height = Math.ceil(r.height);
    canvas.getContext("2d").drawImage(bitmap, 0, 0);
    const data = canvas.toDataURL("image/png");
    const prefix = "data:image/png;base64,";
    if (data.startsWith(prefix) && data.length <= 1400000) result.png = data.slice(prefix.length);
    return result;
  } finally {
    try {
      if (bitmap) bitmap.close();
    } finally {
      if (canvas) {
        canvas.width = 0;
        canvas.height = 0;
      }
    }
  }
})().then(done, () =>
  done({ version: 2, kind: "unavailable", prompt: "unknown", message: "unknown", png: null }),
);
