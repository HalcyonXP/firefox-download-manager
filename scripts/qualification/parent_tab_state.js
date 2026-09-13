// Diagnostic-only snapshot of the original Marionette chrome window.
// No enumeration, prompt dismissal, selection, preference writes or tab creation.
const expected = arguments[0];
if (typeof expected !== "string" || !/^[\x21-\x7e]{1,128}$/u.test(expected)) {
  throw new Error("Owned tab observation refused");
}
const originalWindow = window;
const tabBrowser = originalWindow.gBrowser;
const selectedTab = tabBrowser?.selectedTab;
const selectedBrowser = tabBrowser?.selectedBrowser;
const element = originalWindow.document?.documentElement;
const collapsed = originalWindow.gNavToolbox?.collapsed;
let selectedControl = null;
if (selectedBrowser) {
  const { NavigableManager } = ChromeUtils.importESModule(
    "chrome://remote/content/shared/NavigableManager.sys.mjs",
  );
  const id = NavigableManager.getIdForBrowser(selectedBrowser);
  if (typeof id === "string") selectedControl = id === expected;
}
return {
  version: 1,
  window_modal: element ? element.hasAttribute("window-modal-open") : null,
  navigation_collapsed: typeof collapsed === "boolean" ? collapsed : null,
  selection_consistent:
    selectedTab?.linkedBrowser && selectedBrowser
      ? selectedTab.linkedBrowser === selectedBrowser
      : null,
  selected_control: selectedControl,
  selected_blank: selectedBrowser?.currentURI
    ? selectedBrowser.currentURI.spec === "about:blank"
    : null,
};
