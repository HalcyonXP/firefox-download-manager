// Fixed-ID control in a separately owned disposable profile. Disable is not a
// native-process join; the independent observer must receive that evidence.
((args) => {
  const done = args[args.length - 1];
  const id = "download-manager@halcyonxp.local";
  const operation = args[0];
  if (args.length !== 2 || !["absent", "info", "disable"].includes(operation)) {
    done(null);
    return;
  }
  (async () => {
    const { AddonManager } = ChromeUtils.importESModule(
      "resource://gre/modules/AddonManager.sys.mjs",
    );
    await AddonManager.readyPromise;
    if (AddonManager.isReady !== true) return null;
    const addon = await AddonManager.getAddonByID(id);
    if (AddonManager.isReady !== true) return null;
    if (operation === "absent" || (operation === "disable" && addon === null)) {
      // Public lookup can mask provider/database errors as null. Cross-check
      // only this fixed ID in current XPI state and the active extension map.
      // These are current in-memory observations, not a filesystem/race proof.
      const { XPIExports } = ChromeUtils.importESModule(
        "resource://gre/modules/addons/XPIExports.sys.mjs",
      );
      const { ExtensionParent } = ChromeUtils.importESModule(
        "resource://gre/modules/ExtensionParent.sys.mjs",
      );
      if (
        addon !== null ||
        XPIExports.XPIInternal.XPIStates.findAddon(id) !== undefined ||
        ExtensionParent.GlobalManager.getExtension(id) !== undefined ||
        AddonManager.isReady !== true
      )
        return null;
      return { state: "absent" };
    }
    if (addon === null) return null;
    if (
      addon.id !== id ||
      addon.version !== "0.0.1" ||
      addon.type !== "extension" ||
      addon.temporarilyInstalled !== true
    )
      return null;
    if (operation === "disable") {
      await addon.disable();
      if (addon.isActive !== false || addon.userDisabled !== true) return null;
      return { state: "disabled" };
    }
    const { ExtensionParent } = ChromeUtils.importESModule(
      "resource://gre/modules/ExtensionParent.sys.mjs",
    );
    const { AppConstants } = ChromeUtils.importESModule(
      "resource://gre/modules/AppConstants.sys.mjs",
    );
    const extension = ExtensionParent.GlobalManager.getExtension(id);
    if (
      !extension ||
      extension.id !== id ||
      extension.hasShutdown !== false ||
      extension.privateBrowsingAllowed !== false ||
      extension.persistentBackground !== false ||
      extension.hasPermission("nativeMessaging") !== true ||
      AppConstants.MOZ_UPDATE_CHANNEL !== "aurora" ||
      addon.isActive !== true ||
      addon.userDisabled !== false
    )
      return null;
    return {
      state: "active",
      id,
      version: addon.version,
      temporary: true,
      privateAllowed: false,
      persistentBackground: false,
    };
  })().then(done, () => done(null));
})(arguments);
