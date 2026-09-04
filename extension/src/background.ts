import { PROTOCOL_VERSION } from "./protocol";

// Event listeners are registered synchronously because Firefox may recreate
// this Manifest V3 event page. Authoritative task state will live in the helper.
browser.runtime.onInstalled.addListener(() => {
  void PROTOCOL_VERSION;
});
