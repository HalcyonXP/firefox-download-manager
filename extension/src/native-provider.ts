import { NativeConnection } from "./native-connection";
import { parentConnector, type ParentTransportApi } from "./parent-connector";

// Replaced by reviewed builders, never a runtime setting or caller message.
// Unbundled source defaults to the ordinary connector.
declare const __DM_PARENT_TRANSPORT__: boolean;

/** Build selection chooses a transport, never permission to capture. */
export function createNativeConnection(): NativeConnection {
  if (typeof __DM_PARENT_TRANSPORT__ !== "undefined" && __DM_PARENT_TRANSPORT__ === true) {
    const api = (browser as typeof browser & { managerParentTransport: ParentTransportApi })
      .managerParentTransport;
    // Missing/failed privileged API must not silently downgrade to ordinary
    // Native Messaging. Parent readiness still excludes capture capability.
    return new NativeConnection(parentConnector(api));
  }
  return new NativeConnection();
}
