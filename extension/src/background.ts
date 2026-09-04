import { NativeConnection } from "./native-connection";
import { PROTOCOL_VERSION } from "./protocol";

export const nativeConnection = new NativeConnection();

// Event listeners are registered synchronously because Firefox may recreate
// this Manifest V3 event page. Authoritative task state will live in the helper.
browser.runtime.onInstalled.addListener(() => {
  void PROTOCOL_VERSION;
  void nativeConnection;
});
