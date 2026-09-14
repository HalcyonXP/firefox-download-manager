// Unselected fileless SDK process fixture. These constants are fixed by the
// separately reviewed builder, never by an extension API caller.
import { firefoxLauncherPlatform } from "../protection-bridge/parent-launcher.js";
import { ParentFixtureSession } from "./session.js";

Cu.importGlobalProperties(["TextEncoder", "TextDecoder"]);
const { Subprocess } = ChromeUtils.importESModule("resource://gre/modules/Subprocess.sys.mjs");
const { AsyncShutdown } = ChromeUtils.importESModule(
  "resource://gre/modules/AsyncShutdown.sys.mjs",
);
const { AppConstants } = ChromeUtils.importESModule("resource://gre/modules/AppConstants.sys.mjs");
const Timer = ChromeUtils.importESModule("resource://gre/modules/Timer.sys.mjs");
const HOST = "com.halcyonxp.firefox_download_manager";
const ID = "download-manager@halcyonxp.local";

function notify(kind, value) {
  // A separate chrome observer reads a nonce-correlated plain record. It does
  // not retain the API realm/owner and thereby mask a lifetime defect.
  Services.obs.notifyObservers(
    null,
    "download-manager-owned-parent-fixture",
    JSON.stringify({ nonce: __OWNED_FIXTURE_NONCE__, kind, value }),
  );
}

globalThis.managerParentProbe = class extends ExtensionAPI {
  #session = null;
  #retirement = null;
  #retirementFailed = false;
  #closed = false;

  #create(context) {
    // Deliberately NOT NativeManifests.lookupManifest: this diagnostic supplies
    // only its owned fixture metadata and never reads/writes shared registration.
    // A successful fixture observation cannot qualify real manifest lookup,
    // the installed Manager image, its private IPC class or browser policy.
    const fixtureManifests = {
      lookupManifest: (type, name, caller) => {
        if (type !== "stdio" || name !== HOST || caller !== context)
          throw new Error("Owned fixture manifest refused");
        return Promise.resolve({
          path: __OWNED_FIXTURE_MANIFEST__,
          manifest: {
            name: HOST,
            type: "stdio",
            path: __OWNED_FIXTURE_COMMAND__,
            allowed_extensions: [ID],
          },
        });
      },
    };
    const platform = firefoxLauncherPlatform(
      fixtureManifests,
      Subprocess,
      AsyncShutdown,
      PathUtils,
      AppConstants,
      Timer,
    );
    return new ParentFixtureSession(this.extension, platform, notify);
  }

  getAPI(context) {
    return {
      managerParentProbe: {
        run: (...args) => {
          if (args.length !== 0 || this.#closed || this.#retirementFailed)
            return Promise.reject(new Error("Owned parent fixture API refused"));
          if (this.#session === null) this.#session = this.#create(context);
          return this.#session.run(context);
        },
      },
    };
  }

  onShutdown() {
    this.#closed = true;
    if (this.#session !== null && this.#retirement === null) {
      this.#retirement = this.#session.close();
      // Retain the original promise/failure. Missing observation is refusal at
      // the outer controller, not a joined receipt or permission to retry.
      void this.#retirement.catch(() => {
        this.#retirementFailed = true;
      });
    }
  }
};
