// Unselected, fileless diagnostic. No native publication authority.
// The only service query describes an empty, unsigned loopback text fixture.
this.managerProtection = class extends ExtensionAPI {
  constructor(extension) {
    super(extension);
    this.owner = null;
    this.closed = false;
    this.attempted = false;
    this.callbacks = 0;
    this.metadataReads = 0;
    this.stage = "idle";
    this.result = "none";
  }

  receipt() {
    return {
      version: 2,
      qualification: false,
      scope: "fixed-empty-loopback-context",
      stage: this.closed ? "closed" : this.stage,
      result: this.closed ? "unavailable" : this.result,
      attempted: this.attempted,
      callbacks: this.callbacks,
      metadata_reads: this.metadataReads,
    };
  }

  getAPI(context) {
    if (
      this.closed ||
      context.extension !== this.extension ||
      context.envType !== "addon_parent" ||
      context.viewType !== "tab" ||
      context.isTopContext !== true ||
      context.incognito !== false ||
      context.uri?.spec !== this.extension.baseURI.resolve("probe.html") ||
      (this.owner !== null && this.owner !== context)
    ) {
      throw new Error("Protection probe caller refused");
    }
    if (this.owner === null) {
      this.owner = context;
      context.callOnClose({ close: () => this.onShutdown() });
    }
    return {
      managerProtection: {
        start: async (...args) => {
          if (args.length !== 0) throw new Error("Protection probe arguments refused");
          if (!this.closed && !this.attempted) {
            // Consume the sole attempt BEFORE service construction/dispatch.
            // A synchronous throw is not evidence that no operation started.
            this.attempted = true;
            this.stage = "pending";
            try {
              const service = Cc[
                "@mozilla.org/reputationservice/application-reputation-service;1"
              ].getService(Ci.nsIApplicationReputationService);
              const query = this.fixedQuery();
              service.queryReputation(query, (shouldBlock, status, verdict) => {
                // Saturate observations; never accumulate provider data/errors.
                this.callbacks = Math.min(2, this.callbacks + 1);
                if (this.closed) return;
                this.stage = "settled";
                if (
                  this.result === "unavailable" ||
                  this.callbacks !== 1 ||
                  this.metadataReads !== 15 ||
                  status !== 0 ||
                  typeof shouldBlock !== "boolean" ||
                  !Number.isInteger(verdict) ||
                  verdict < 0 ||
                  verdict > 4 ||
                  (shouldBlock && verdict === 0)
                ) {
                  this.result = "unavailable";
                } else {
                  this.result = shouldBlock ? "blocked" : "not-blocked";
                }
              });
            } catch {
              this.stage = "settled";
              this.result = "unavailable";
            }
          }
          return this.receipt();
        },
        snapshot: async (...args) => {
          if (args.length !== 0) throw new Error("Protection probe arguments refused");
          return this.receipt();
        },
      },
    };
  }

  fixedQuery() {
    const io = Cc["@mozilla.org/network/io-service;1"].getService(Ci.nsIIOService);
    const root = "http://127.0.0.1/";
    const source = io.newURI(`${root}download-manager-protection-probe.txt`);
    const referring = io.newURI(`${root}download-manager-protection-referrer.html`);
    const redirected = io.newURI(`${root}download-manager-protection-redirect.txt`);
    const referrer = Cc["@mozilla.org/referrer-info;1"].createInstance(Ci.nsIReferrerInfo);
    // Fixed synthetic metadata, not a policy preference or captured request.
    referrer.init(Ci.nsIReferrerInfo.NO_REFERRER, false, referring);
    if (referrer.originalReferrer?.spec !== referring.spec || referrer.sendReferrer !== false) {
      throw new Error("Protection fixture referrer refused");
    }
    const principal = Cc["@mozilla.org/scriptsecuritymanager;1"]
      .getService(Ci.nsIScriptSecurityManager)
      .createContentPrincipal(redirected, {});
    const redirects = Cc["@mozilla.org/array;1"].createInstance(Ci.nsIMutableArray);
    const read = (bit, value) => {
      this.metadataReads |= bit;
      return value;
    };
    const unexpected = () => this.unexpectedMetadata();
    // Implementation consumes history entries, despite the older IDL comment
    // describing principals directly. Do not rely on ignored AddRedirects errors.
    redirects.appendElement({
      QueryInterface: ChromeUtils.generateQI(["nsIRedirectHistoryEntry"]),
      get principal() {
        return read(4, principal);
      },
      get referrerURI() {
        return unexpected();
      },
      get remoteAddress() {
        return unexpected();
      },
    });
    const digest = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    return {
      get sourceURI() {
        return read(1, source);
      },
      get referrerInfo() {
        return read(2, referrer);
      },
      get suggestedFileName() {
        return read(8, "download-manager-protection-probe.txt");
      },
      fileSize: 0,
      sha256Hash: String.fromCharCode(...digest.match(/../gu).map((x) => Number.parseInt(x, 16))),
      signatureInfo: [],
      redirects,
    };
  }

  unexpectedMetadata() {
    this.metadataReads |= 16;
    this.result = "unavailable";
    throw new Error("Unexpected protection metadata access");
  }

  onShutdown() {
    // The service exposes no cancellation handle. Closing a context is NOT
    // service retirement. The owned Firefox process must still exit and join.
    this.closed = true;
    this.owner = null;
  }
};
