// Executed only in the retained owned Firefox chrome context. One load attempt.
// Return fixed terms from its own exception, never raw messages, paths or IDs.
const done = arguments[arguments.length - 1];
const expected = arguments[1];
const terms = [
  ["invalid-extension", "Extension is invalid"],
  ["experiment-apis", "experiment_apis"],
  ["privilege-required", "requires a privileged add-on"],
  ["manifest-version", "manifest_version"],
  ["csp", "content_security_policy"],
  ["schema", "schema"],
  ["unexpected-property", "Unexpected property"],
  ["async-returns", "Async functions must not have return values"],
  ["enum", "enum"],
  ["signature", "signature"],
  ["incognito", "incognito"],
];
function refusal(error, phase) {
  const matched = new Set();
  let complete = true;
  function classify(text) {
    if (typeof text !== "string" || text.length > 2048) {
      complete = false;
      return;
    }
    for (const [term, needle] of terms) if (text.includes(needle)) matched.add(term);
  }
  try {
    classify(error?.message);
    const additional = error?.additionalErrors;
    if (additional !== undefined) {
      if (!Array.isArray(additional)) complete = false;
      else {
        if (additional.length > 8) complete = false;
        for (const text of additional.slice(0, 8)) classify(text);
      }
    }
  } catch {
    complete = false;
  }
  return { version: 1, state: "refused", phase, terms: [...matched].sort(), complete };
}
let phase = "bootstrap";
try {
  const { AddonManager } = ChromeUtils.importESModule(
    "resource://gre/modules/AddonManager.sys.mjs",
  );
  phase = "file-init";
  const file = Components.classes["@mozilla.org/file/local;1"].createInstance(
    Components.interfaces.nsIFile,
  );
  file.initWithPath(arguments[0]);
  phase = "install";
  AddonManager.installTemporaryAddon(file).then(
    (addon) => {
      try {
        done({
          version: 1,
          state: addon.id === expected ? "loaded" : "refused",
          phase: "identity",
          terms: [],
          complete: true,
        });
      } catch (error) {
        done(refusal(error, "identity"));
      }
    },
    (error) => done(refusal(error, "install")),
  );
} catch (error) {
  done(refusal(error, phase));
}
