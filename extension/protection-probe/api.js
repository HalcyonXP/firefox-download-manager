// Unselected, fileless diagnostic. No native publication authority.
// The only service query describes an empty, unsigned loopback text fixture.
this.managerProtection = class extends ExtensionAPI {
  constructor(extension) {
    super(extension);
    this.owner = null;
    this.closed = false;
    this.attempted = false;
    this.callbacks = 0;
    this.stage = "idle";
    this.result = "none";
  }

  receipt() {
    return {
      version: 1,
      qualification: false,
      scope: "fixed-empty-loopback-text",
      stage: this.closed ? "closed" : this.stage,
      result: this.closed ? "unavailable" : this.result,
      attempted: this.attempted,
      callbacks: this.callbacks,
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
              const uri = Cc["@mozilla.org/network/io-service;1"]
                .getService(Ci.nsIIOService)
                .newURI("http://127.0.0.1/download-manager-protection-probe.txt");
              const digest = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
              const query = {
                sourceURI: uri,
                referrerInfo: null,
                suggestedFileName: "download-manager-protection-probe.txt",
                fileSize: 0,
                sha256Hash: String.fromCharCode(
                  ...digest.match(/../gu).map((x) => Number.parseInt(x, 16)),
                ),
                signatureInfo: [],
                redirects: Cc["@mozilla.org/array;1"].createInstance(Ci.nsIMutableArray),
              };
              service.queryReputation(query, (shouldBlock, status, verdict) => {
                // Saturate observations; never accumulate provider data/errors.
                this.callbacks = Math.min(2, this.callbacks + 1);
                if (this.closed) return;
                this.stage = "settled";
                if (
                  this.result === "unavailable" ||
                  this.callbacks !== 1 ||
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

  onShutdown() {
    // The service exposes no cancellation handle. Closing a context is NOT
    // service retirement. The owned Firefox process must still exit and join.
    this.closed = true;
    this.owner = null;
  }
};
