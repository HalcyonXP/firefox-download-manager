// Only in a separately owned, nonce-named Marionette chrome sandbox. This
// observer retains bounded strings, never the add-on API realm or process owner.
((args) => {
  const topic = "download-manager-owned-parent-fixture";
  const key = "__ownedParentFixtureObserverV1";
  const refused = () => {
    throw new Error("Owned parent observer refused");
  };
  if (args.length !== 4 || Services.appinfo.processType !== 0) refused();
  const [operation, nonce, collector, done] = args;
  const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/u;
  if (
    !["install", "snapshot", "remove"].includes(operation) ||
    typeof done !== "function" ||
    typeof nonce !== "string" ||
    !uuid.test(nonce) ||
    typeof collector !== "string" ||
    !uuid.test(collector) ||
    collector === nonce
  )
    refused();
  if (operation === "install") {
    if (Object.hasOwn(globalThis, key)) refused();
    const records = [];
    let failed = false;
    let state = "uncertain";
    let removed = false;
    let removalAttempted = false;
    let checking = false;
    let controlSeen = false;
    const observer = {
      observe(subject, observedTopic, data) {
        try {
          if (observedTopic !== topic) return;
          if (
            typeof data !== "string" ||
            data.length > 8192 ||
            !/^[\x20-\x7e\r\n\t]*$/u.test(data)
          ) {
            failed = true;
            return;
          }
          const value = JSON.parse(data);
          if (value === null || typeof value !== "object" || value.nonce !== nonce) return;
          if (
            checking &&
            subject === null &&
            value.collector === collector &&
            value.control === "before-remove-check" &&
            Object.keys(value).sort().join(",") === "collector,control,nonce"
          ) {
            controlSeen = true;
            return;
          }
          if (state === "closed") {
            failed = true;
            removed = false;
            return;
          }
          if (subject !== null || records.length >= 2) {
            failed = true;
            return;
          }
          // Keep exact original JSON. The external parser rejects duplicates,
          // nonfinite numbers, unknown fields/types, ordering and PID changes.
          records.push(data);
        } catch {
          failed = true;
        }
      },
    };
    const owner = Object.freeze({
      nonce,
      collector,
      snapshot() {
        return {
          version: 1,
          qualification: false,
          collector,
          state,
          removed,
          failed,
          records: [...records],
        };
      },
      install() {
        try {
          Services.obs.addObserver(observer, topic, false);
          state = "active";
        } catch {
          failed = true;
        }
      },
      remove() {
        if (!removalAttempted) {
          removalAttempted = true;
          state = "closed";
          checking = true;
          try {
            Services.obs.notifyObservers(
              null,
              topic,
              JSON.stringify({ nonce, collector, control: "before-remove-check" }),
            );
          } catch {
            failed = true;
          } finally {
            checking = false;
          }
          if (!controlSeen) failed = true;
          // Cleanup still attempts the exact inverse even if the control failed.
          try {
            Services.obs.removeObserver(observer, topic);
            // removeObserver returns void. Check only this callback with a
            // synchronous control notification, never enumerate other observers.
            removed = controlSeen;
            Services.obs.notifyObservers(
              null,
              topic,
              JSON.stringify({ nonce, control: "removed-check" }),
            );
          } catch {
            failed = true;
            removed = false;
          }
        }
        return this.snapshot();
      },
    });
    // Retain the exact inverse before registration, even if add acts then throws.
    Object.defineProperty(globalThis, key, { value: owner, writable: false, configurable: false });
    owner.install();
  }
  if (!Object.hasOwn(globalThis, key)) refused();
  const owner = globalThis[key];
  if (!owner || owner.nonce !== nonce || owner.collector !== collector) refused();
  done(operation === "remove" ? owner.remove() : owner.snapshot());
})(arguments);
