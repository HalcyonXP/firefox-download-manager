// Unselected browser-parent component. Metadata is not capture/verdict authority.
// The platform dependency belongs to the trusted parent adapter, never API input.
const MAX_CONTEXTS = 32;
const MAX_REDIRECTS = 8;
const MAX_URI = 16 * 1024;

function refused() {
  throw new Error("Download protection context refused");
}

function uriText(uri) {
  const text = uri?.spec;
  if (typeof text !== "string" || text.length === 0 || text.length > MAX_URI) refused();
  const parsed = new URL(text);
  if (
    !["http:", "https:"].includes(parsed.protocol) ||
    parsed.username !== "" ||
    parsed.password !== "" ||
    parsed.href !== text
  )
    refused();
  return text;
}

function defaultAttributes(attributes, platform) {
  if (
    !attributes ||
    attributes.userContextId !== 0 ||
    attributes.privateBrowsingId !== 0 ||
    platform.cookieStore(attributes) !== "firefox-default"
  )
    refused();
}

function copyUri(uri, platform) {
  const text = uriText(uri);
  const copy = platform.uri(text);
  if (uriText(copy) !== text) refused();
  return copy;
}

function copyReferrer(info, platform) {
  if (info === null) return null;
  if (!info) refused();
  const policy = info.referrerPolicy;
  const send = info.sendReferrer;
  const originalInput = info.originalReferrer;
  if (!Number.isInteger(policy) || policy < 0 || policy > 8 || typeof send !== "boolean") refused();
  const original = originalInput === null ? null : copyUri(originalInput, platform);
  const copy = platform.referrer(policy, send, original);
  if (
    copy === info ||
    copy.referrerPolicy !== policy ||
    copy.sendReferrer !== send ||
    (original === null
      ? copy.originalReferrer !== null
      : uriText(copy.originalReferrer) !== uriText(original))
  )
    refused();
  return copy;
}

function principals(history, platform) {
  if (!Array.isArray(history) || history.length > MAX_REDIRECTS) refused();
  return Object.freeze(
    Array.from(history, (entry) => {
      const principal = platform.historyPrincipal(entry);
      if (principal?.isContentPrincipal !== true) refused();
      defaultAttributes(principal.originAttributes, platform);
      uriText(principal.URI);
      // Native nsIPrincipal URI/origin attributes are read-only. Retain the native
      // principal, not a mutable history entry or caller-reconstructed identity.
      return principal;
    }),
  );
}

class RequestMetadata {
  #data;
  #retire;

  constructor(data, retire) {
    Object.defineProperty(data, "toJSON", { value: () => this.toJSON() });
    this.#data = Object.freeze(data);
    this.#retire = retire;
    Object.freeze(this);
  }

  // Internal trusted-parent use only; no serialized browser API exposes this.
  read() {
    if (this.#data === null) refused();
    return this.#data;
  }

  release() {
    if (this.#data === null) return;
    this.#data = null;
    const retire = this.#retire;
    this.#retire = null;
    retire();
  }

  toJSON() {
    throw new Error("Download protection context is not serializable");
  }
}

export class RegisteredRequestContexts {
  #extension;
  #platform;
  #owner = null;
  #closed = false;
  #busy = false;
  #records = new Map();

  constructor(extension, platform) {
    this.#extension = extension;
    this.#platform = platform;
  }

  capture(context, requestId, tabId) {
    if (this.#busy) refused();
    this.#busy = true;
    try {
      return this.#capture(context, requestId, tabId);
    } catch {
      // Native getter/URL failures must not leak paths, URLs or browser errors.
      refused();
    } finally {
      this.#busy = false;
    }
  }

  #requireLive(context) {
    // BaseContext.callOnClose does not close an already-unloaded caller. Read
    // closure last: even a reentrant lifetime getter must not revive this owner.
    if (context.active !== true || context.unloaded !== false || this.#closed) refused();
  }

  #capture(context, requestId, tabId) {
    this.#requireLive(context);
    if (
      this.#closed ||
      context.extension !== this.#extension ||
      context.envType !== "addon_parent" ||
      context.viewType !== "background" ||
      context.isTopContext !== true ||
      context.incognito !== false ||
      context.uri?.spec !== this.#extension.baseURI.resolve("_generated_background_page.html") ||
      (this.#owner !== null && this.#owner !== context) ||
      typeof requestId !== "string" ||
      !/^[1-9][0-9]{0,15}$/u.test(requestId) ||
      !Number.isSafeInteger(Number(requestId)) ||
      !Number.isSafeInteger(tabId) ||
      tabId < 0
    )
      refused();
    if (this.#owner === null) {
      this.#owner = context;
      try {
        context.callOnClose({ close: () => this.close() });
      } catch {
        // Unknown close-hook delivery cannot authorize later reuse.
        this.close();
        refused();
      }
    }
    this.#requireLive(context);
    const prior = this.#records.get(requestId);
    if (prior) {
      if (prior.read().tabId !== tabId) refused();
      return prior;
    }
    if (this.#records.size >= MAX_CONTEXTS) refused();
    const policy = this.#extension.policy;
    const remoteTab = context.xulBrowser.frameLoader.remoteTab;
    if (!policy || remoteTab === undefined) refused();
    const wrapper = this.#platform.lookup(Number(requestId), policy, remoteTab);
    const channel = wrapper?.channel;
    const loadInfo = channel?.loadInfo;
    const browser = wrapper?.browserElement;
    if (
      !channel ||
      !loadInfo ||
      !browser ||
      String(wrapper.id) !== requestId ||
      wrapper.method !== "GET" ||
      wrapper.type !== "main_frame" ||
      wrapper.frameId !== 0 ||
      wrapper.parentFrameId !== -1 ||
      wrapper.statusCode !== 200 ||
      wrapper.canModify !== true ||
      (wrapper.errorString !== null && wrapper.errorString !== "") ||
      wrapper.matches({ types: ["main_frame"], incognito: false }, policy) !== true ||
      this.#platform.browserData(browser).tabId !== tabId
    )
      refused();
    defaultAttributes(loadInfo.originAttributes, this.#platform);
    const sourceURI = copyUri(channel.URI, this.#platform);
    if (wrapper.finalURL !== sourceURI.spec) refused();
    const referrerInfo = copyReferrer(this.#platform.httpReferrer(channel), this.#platform);
    const redirects = principals(loadInfo.redirectChain, this.#platform);
    // A wrapper survives redirects and can replace its channel. It is not a
    // frozen context. Retain only the copied metadata/native principals.
    if (
      this.#closed ||
      wrapper.channel !== channel ||
      channel.loadInfo !== loadInfo ||
      wrapper.browserElement !== browser ||
      String(wrapper.id) !== requestId ||
      wrapper.finalURL !== sourceURI.spec ||
      uriText(channel.URI) !== sourceURI.spec ||
      this.#platform.browserData(browser).tabId !== tabId
    )
      refused();
    defaultAttributes(loadInfo.originAttributes, this.#platform);
    const stillMatches = wrapper.matches({ types: ["main_frame"], incognito: false }, policy);
    if (this.#closed || stillMatches !== true) refused();
    this.#requireLive(context);
    const snapshot = new RequestMetadata(
      { requestId, tabId, sourceURI, referrerInfo, redirects },
      () => this.#records.delete(requestId),
    );
    this.#records.set(requestId, snapshot);
    return snapshot;
  }

  close() {
    this.#closed = true;
    for (const record of this.#records.values()) record.release();
    this.#records.clear();
    this.#owner = null;
    this.#extension = null;
    this.#platform = null;
  }
}

// Bind these SDK objects only inside the trusted parent implementation. This
// factory performs no preference, observer, network, cancellation or file work.
export function firefoxContextPlatform(ChannelWrapper, ExtensionParent, Cc, Ci) {
  const global = ExtensionParent.apiManager.global;
  const io = Cc["@mozilla.org/network/io-service;1"].getService(Ci.nsIIOService);
  return Object.freeze({
    lookup: (id, policy, remoteTab) => ChannelWrapper.getRegisteredChannel(id, policy, remoteTab),
    browserData: (browser) => global.tabTracker.getBrowserData(browser),
    cookieStore: (attributes) => global.getCookieStoreIdForOriginAttributes(attributes),
    uri: (text) => io.newURI(text),
    httpReferrer: (channel) => channel.QueryInterface(Ci.nsIHttpChannel).referrerInfo,
    historyPrincipal: (entry) => entry.QueryInterface(Ci.nsIRedirectHistoryEntry).principal,
    referrer: (policy, send, original) => {
      const copy = Cc["@mozilla.org/referrer-info;1"].createInstance(Ci.nsIReferrerInfo);
      copy.init(policy, send, original);
      return copy;
    },
  });
}
