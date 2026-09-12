// Fixed privileged parent entry. Never accept host names, SDK dependencies,
// native frames/admission IDs, browser context or policy verdicts from callers.
import { ParentApi } from "../protection-bridge/parent-api.js";
import { firefoxLauncherPlatform } from "../protection-bridge/parent-launcher.js";

Cu.importGlobalProperties(["TextEncoder", "TextDecoder"]);
const { NativeManifests } = ChromeUtils.importESModule(
  "resource://gre/modules/NativeManifests.sys.mjs",
);
const { Subprocess } = ChromeUtils.importESModule("resource://gre/modules/Subprocess.sys.mjs");
const { AsyncShutdown } = ChromeUtils.importESModule(
  "resource://gre/modules/AsyncShutdown.sys.mjs",
);
const { AppConstants } = ChromeUtils.importESModule("resource://gre/modules/AppConstants.sys.mjs");
const Timer = ChromeUtils.importESModule("resource://gre/modules/Timer.sys.mjs");

function refused() {
  throw new Error("Download Manager parent API refused");
}

globalThis.managerParentTransport = class extends ExtensionAPI {
  #owner = null;
  #closed = false;
  #retirement = null;

  getAPI(context) {
    if (this.#closed) refused();
    this.#owner ??= new ParentApi(
      this.extension,
      firefoxLauncherPlatform(
        NativeManifests,
        Subprocess,
        AsyncShutdown,
        PathUtils,
        AppConstants,
        Timer,
      ),
      undefined,
      (record) => {
        // Local chrome shutdown observation only: no network, storage, paths,
        // URLs, task data or policy verdict. Observer delivery is not a join.
        const text = JSON.stringify(record);
        if (text.length > 4096) refused();
        Services.obs.notifyObservers(null, "download-manager-parent-retirement", text);
      },
    );
    const call =
      (name, count) =>
      async (...args) => {
        try {
          if (this.#closed || args.length !== count) refused();
          return await this.#owner[name](context, ...args);
        } catch {
          refused(); // No raw SDK error/path or process details cross this API.
        }
      };
    return {
      managerParentTransport: {
        open: call("open", 0),
        ready: call("ready", 1),
        read: call("read", 1),
        postMessage: call("postMessage", 2),
        close: call("close", 1),
      },
    };
  }

  onShutdown() {
    this.#closed = true;
    if (this.#owner !== null && this.#retirement === null) {
      this.#retirement = this.#owner.shutdown();
      // SDK hooks do not await shutdown. The launcher's existing shutdown
      // blocker retains failed ownership; this is not a notification-as-join.
      void this.#retirement.catch(() => {});
    }
  }
};
