// Unselected parent implementation, never caller-supplied launch dependencies.
// This owns one fixed-host attempt, not browser policy or a NativeConnector API.
import { RetainedNativeTransport } from "./native-transport.js";

const HOST = "com.halcyonxp.firefox_download_manager";
const EXTENSION = "download-manager@halcyonxp.local";
const HELPER = "download-manager-native-host.exe";
const MAX_FRAME = 1024 * 1024;

function refused() {
  throw new Error("Download protection parent launch refused");
}

function localPath(value, platform) {
  if (typeof value !== "string" || value.length > 32767 || value.includes("\0")) refused();
  const path = value.replaceAll("/", "\\");
  // Local drive or its extended-length spelling; not a UNC/device executable.
  const root = /^(?:[A-Za-z]:\\|\\\\\?\\[A-Za-z]:\\)/u.exec(path);
  if (root === null || platform.isAbsolute(path) !== true) refused();
  for (const part of path.slice(root[0].length).split("\\")) {
    if (!part || part === "." || part === ".." || /[:. ]$/u.test(part) || part.includes(":"))
      refused();
  }
  return path;
}

function launchOptions(info, platform) {
  const manifest = info?.manifest;
  if (
    manifest?.name !== HOST ||
    manifest.type !== "stdio" ||
    !Array.isArray(manifest.allowed_extensions) ||
    manifest.allowed_extensions.length !== 1 ||
    manifest.allowed_extensions[0] !== EXTENSION
  )
    refused();
  const command = localPath(manifest.path, platform);
  const path = localPath(info.path, platform);
  const workdir = platform.parent(command);
  if (platform.filename(command) !== HELPER || path !== platform.join(workdir, `${HOST}.json`))
    refused();
  return Object.freeze({
    command,
    arguments: Object.freeze(["--browser-parent", path, EXTENSION]),
    workdir,
    stderr: "pipe",
    disclaim: true,
  });
}

export class FixedParentLauncher {
  #extension;
  #platform;
  #onMessage;
  #onDisconnect;
  #context = null;
  #startup = null;
  #transport = null;
  #closed = false;
  #failed = false;
  #checking = false;
  #spawnCalled = false;
  #retirement = null;
  #disconnect = Promise.resolve(true);
  #undo = [];

  constructor(extension, platform, { onMessage, onDisconnect }) {
    this.#extension = extension;
    this.#platform = platform;
    this.#onMessage = onMessage;
    this.#onDisconnect = onDisconnect;
  }

  #requireCaller(context) {
    if (this.#checking) refused();
    this.#checking = true;
    try {
      const extension = this.#extension;
      if (
        context !== null &&
        (this.#context === null || this.#context === context) &&
        context.extension === extension &&
        extension.id === EXTENSION &&
        context.envType === "addon_parent" &&
        context.viewType === "background" &&
        context.isBackgroundContext === true &&
        extension.persistentBackground === false &&
        context.isTopContext === true &&
        context.incognito === false &&
        context.uri?.spec === extension.baseURI.resolve("_generated_background_page.html") &&
        extension.hasPermission("nativeMessaging") === true &&
        extension.hasShutdown === false &&
        context.active === true &&
        context.unloaded === false &&
        this.#platform.windows === true &&
        !this.#closed
      )
        return;
      refused();
    } catch {
      refused();
    } finally {
      this.#checking = false;
    }
  }

  // Internal brokers need the same live guard even when they are only retaining
  // a pre-admission Hello. This neither starts a process nor sends a message.
  assertCaller(context) {
    this.#requireCaller(context);
  }

  // SDK event-page scheduling tag for this actual retained native-port attempt.
  // This is not peer authentication, process creation or publication authority.
  get native() {
    return true;
  }

  // A true result observes transport startup only, not a private native Hello,
  // installed authority, captured request, permission to cancel or policy result.
  start(context) {
    if (this.#closed) return Promise.resolve(false);
    this.#requireCaller(context);
    if (this.#startup === null) {
      this.#context = context;
      this.#startup = Promise.resolve().then(() => this.#start(context));
    }
    return this.#startup;
  }

  #register(install, remove, context, retainOnFailure = false) {
    // A throwing registration may already have installed its hook. Always retain
    // the exact inverse before attempting it, and never proceed on uncertainty.
    this.#undo.push({ remove, retainOnFailure });
    try {
      install();
    } catch {
      this.#failed = true;
      refused();
    }
    this.#requireCaller(context);
  }

  #keepAlive(context) {
    // Inspect membership of this owner only, never enumerate other native ports.
    const ports = context.activeNativePorts;
    const contains = WeakSet.prototype.has.bind(ports, this);
    if (contains()) {
      // Its provenance is unknown: neither adopt/remove it nor disarm the
      // shutdown observation by claiming a clean no-invocation retirement.
      this.#failed = true;
      refused();
    }
    this.#register(
      () => {
        if (context.activeNativePorts !== ports) refused();
        context.trackNativeAppPort(this);
        if (context.activeNativePorts !== ports || !contains()) refused();
      },
      () => {
        if (context.activeNativePorts !== ports) refused();
        context.untrackNativeAppPort(this);
        if (context.activeNativePorts !== ports || contains()) refused();
      },
      context,
      true,
    );
  }

  async #start(context) {
    try {
      this.#requireCaller(context);
      const extension = this.#extension;
      const platform = this.#platform;
      const hook = { close: () => this.close() };
      const blocker = async () => {
        const receipt = await this.close();
        if (!receipt.successful) refused();
      };
      if (platform.shutdown.isClosed !== false) refused();
      this.#register(
        () => platform.shutdown.addBlocker("Download Manager parent transport", blocker),
        () => {
          if (platform.shutdown.removeBlocker(blocker) !== true) refused();
        },
        context,
        true,
      );
      this.#register(
        () => context.callOnClose(hook),
        () => context.forgetOnClose(hook),
        context,
      );
      this.#register(
        () => extension.callOnClose(hook),
        () => extension.forgetOnClose(hook),
        context,
      );
      // Revoke on any removal; do not depend on event-listener ordering relative
      // to the SDK's permission-set update. No permission is requested or changed.
      const removed = () => {
        void this.close();
      };
      this.#register(
        () => extension.on("remove-permissions", removed),
        () => extension.off("remove-permissions", removed),
        context,
      );
      this.#keepAlive(context);
      const info = await platform.lookup(context);
      this.#requireCaller(context);
      const options = launchOptions(info, platform);
      this.#requireCaller(context);
      this.#transport = new RetainedNativeTransport({
        spawn: () => {
          this.#requireCaller(context);
          this.#spawnCalled = true;
          return platform.spawn(options);
        },
        timer: platform.timer,
        onMessage: (message) => {
          this.#requireCaller(context);
          return this.#onMessage(message);
        },
        onDisconnect: () => {
          // Never await this owner from the transport's own callback. Its close
          // observes callbacks, so doing so would create a retirement cycle.
          void this.close();
        },
      });
      const started = await this.#transport.start();
      this.#requireCaller(context);
      return started;
    } catch {
      // Lookup/metadata refusal performs no native invocation. Registration
      // uncertainty is recorded separately; the transport retains spawn failure.
      void this.close();
      return false;
    }
  }

  // Internal transport payloads remain opaque here. A future API/broker must
  // separate closed ordinary commands from parent-only decisions; exposing this
  // as an unrestricted extension API would not establish policy authority.
  postMessage(context, message) {
    this.#requireCaller(context);
    if (this.#transport === null) refused();
    let copy;
    try {
      // Caller serialization can run getters. Recheck authority after freezing
      // plain JSON data, before handing it to the transport's bounded queue.
      const text = JSON.stringify(message);
      if (typeof text !== "string" || new TextEncoder().encode(text).length > MAX_FRAME) refused();
      copy = JSON.parse(text);
    } catch {
      refused();
    }
    this.#requireCaller(context);
    this.#transport.postMessage(copy);
  }

  close() {
    if (this.#retirement !== null) return this.#retirement;
    this.#closed = true;
    this.#retirement = Promise.resolve().then(() => this.#retire());
    if (this.#transport !== null) void this.#transport.close();
    try {
      this.#disconnect = Promise.resolve(this.#onDisconnect()).then(
        () => true,
        () => false,
      );
    } catch {
      this.#disconnect = Promise.resolve(false);
    }
    return this.#retirement;
  }

  async #retire() {
    if (this.#startup !== null) await this.#startup;
    const transport = this.#transport === null ? null : await this.#transport.close();
    const disconnected = await this.#disconnect;
    let hooksRemoved = true;
    const guards = [];
    for (const undo of this.#undo.splice(0).reverse()) {
      if (undo.retainOnFailure) {
        guards.push(undo);
        continue;
      }
      try {
        undo.remove();
      } catch {
        hooksRemoved = false;
        this.#undo.push(undo);
      }
    }
    const settled =
      !this.#failed && disconnected && hooksRemoved && (transport?.successful ?? true);
    // Keep failed native-port scheduling and shutdown guards, not just Booleans.
    // If removing an earlier guard fails, do not disarm the later blocker.
    for (const undo of guards) {
      if (!settled || !hooksRemoved) {
        hooksRemoved = false;
        this.#undo.push(undo);
        continue;
      }
      try {
        undo.remove();
      } catch {
        hooksRemoved = false;
        this.#undo.push(undo);
      }
    }
    // Keep this owner/receipt retained on failure. An SDK call rejection remains
    // indeterminate even when no Process object was returned; never retry it.
    return Object.freeze({
      spawn_called: this.#spawnCalled,
      transport,
      hooks_removed: hooksRemoved,
      successful: !this.#failed && disconnected && hooksRemoved && (transport?.successful ?? true),
    });
  }

  toJSON() {
    throw new Error("Download protection parent launcher is not serializable");
  }
}

// Bind matching SDK modules only within the trusted parent implementation. This
// factory itself does not read registration, spawn, change policy or touch files.
export function firefoxLauncherPlatform(
  NativeManifests,
  Subprocess,
  AsyncShutdown,
  PathUtils,
  AppConstants,
  Timer,
) {
  return Object.freeze({
    windows: AppConstants.platform === "win",
    lookup: (context) => NativeManifests.lookupManifest("stdio", HOST, context),
    spawn: (options) => Subprocess.call(options),
    shutdown: AsyncShutdown.profileBeforeChange,
    isAbsolute: (path) => PathUtils.isAbsolute(path),
    parent: (path) => PathUtils.parent(path),
    filename: (path) => PathUtils.filename(path),
    join: (...parts) => PathUtils.join(...parts),
    timer: (milliseconds, callback) => {
      const id = Timer.setTimeout(callback, milliseconds);
      return () => Timer.clearTimeout(id);
    },
  });
}
