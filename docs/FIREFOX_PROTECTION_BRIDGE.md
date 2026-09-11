# Firefox download-protection bridge

## Scope and current state

Native transfer ownership remains in the companion. Public automatic capture remains disabled pending protection integration. Windows Internet-zone metadata is provenance, not a Firefox reputation result. The default/manual and capture-candidate manifests do **not** select a privileged API.

`extension/protection-probe/` is the first, separately built **fileless service probe**, not the production bridge. A Firefox **API experiment** is privileged browser-parent code registered through `experiment_apis`; here it exposes only `managerProtection.start()` and `snapshot()`, both without arguments. This additional trust boundary is not general-purpose access to files, profiles, preferences, arbitrary URLs or commands.

The source is restricted to one exact, non-private, top-level extension tab (`probe.html`) and one retained caller context. Content scripts, frames, background pages, other extensions, query/fragment variants and replacement contexts refuse. One attempt is consumed before service construction/dispatch; repeated calls only observe the same attempt. Closing the context/extension revokes observation. The service has no cancellation handle: context closure is **not** service retirement; the owned Firefox process must exit and be joined.

## Fixed query and interpretation

The only query describes an empty unsigned text fixture at a fixed HTTP127.0.0.1 URL. Its zero length, empty-content SHA-256, empty signature array and empty redirect array are constructed inside the privileged component. No caller supplies these fields. No file is opened, downloaded, scanned or published by this probe; it does not issue an HTTP request to that fixture URL. Firefox's existing protection service remains responsible for any normal protection-provider activity.

The direct service callback is classified using its boolean, status and known verdict range. Errors, malformed/contradictory results, uncertain throws and duplicate callbacks produce `unavailable`; callback counts saturate at two. A successful nonblocking policy result is called **not-blocked**, not “safe.” Receipts contain only fixed classifications/counts and always `qualification:false`. Polling is bounded and never reissues the query. A pending/failed observation is not a successful probe.

The owned driver first checks refusal from a query-bearing page, then the fixed query and unchanged receipt after a repeated start, then refusal after caller-context retirement. Initial page text is `starting`, distinct from refusal, so a script that never ran cannot satisfy a negative case.

## Verified source distinctions

Reviewed Firefox source identifies these consequential boundaries:

- `DownloadIntegration.shouldBlockForReputationCheck` can resolve nonblocking when saver hash/signature extraction fails; it also normalizes callback outcomes without exposing the callback status. The probe therefore calls the service directly and checks its status, rather than interpreting that convenience result as successful checking.
- `ApplicationReputation.cpp` checks source/redirect/referrer URL blocklists before its non-binary completion path. A `.gguf`-only policy does not by itself remove this requirement. Internal Firefox fallback behavior still belongs to that service; observing `not-blocked` does not prove every underlying check succeeded.
- `nsIApplicationReputationQuery` takes a raw-byte SHA-256 string, nested byte arrays for signature information, and an `nsIArray` of redirects. Its `fileSize` is currently a 32-bit unsigned long: large-file behavior must be reviewed before provider qualification. The probe's size is zero; it does not qualify large files.
- Experiment availability, privileged status and persistent installation are separate. The experiment preference's source fallback is not an observation of effective profile settings. Temporary loading does not qualify ordinary persistent installation.
- Windows `DownloadIntegration.downloadDone` writes origin metadata separately and explicitly avoids `IAttachmentExecute.Save`; calling that entire completion routine would also perform unrelated permissions/notification operations. It is not a narrowly scoped substitute for a reputation query.

Source reference: Mozilla revision [`574c275bcf5b4f86198c979b7e61f4a844aba0ea`](https://hg.mozilla.org/releases/mozilla-beta/file/574c275bcf5b4f86198c979b7e61f4a844aba0ea/), particularly `toolkit/components/reputationservice/nsIApplicationReputation.idl`, `ApplicationReputation.cpp`, the installed `DownloadIntegration.sys.mjs`, `toolkit/components/extensions/Extension{,Parent}.sys.mjs` and `toolkit/mozapps/extensions/internal/AddonSettings.sys.mjs`. No vendor source is redistributed by the probe.

## Build and owned observation

Maintainer commands, not installation instructions:

```powershell
python scripts/build-protection-probe.py --output artifacts/<new-probe-directory>
python scripts/probe-download-protection.py --probe artifacts/<clean-probe-directory> --firefox "<Developer Edition executable>" --report artifacts/<new-report>.json --execute-owned-browser
```

The builder creates a unique diagnostic add-on ID and exactly six bounded XPI members. Source/ZIP CRC/payload hashes and manifest authority are reread; output is exclusive and restricted to ordinary paths below `artifacts`. The input validator is separate from, and does not broaden, either product XPI policy. Execution requires clean exact-source inputs, original fresh closed-app/registration preflights, a ticket before domain creation and a retained isolated Firefox owner. No native registration, companion launch, normal profile, protection-preference change, general file API or signing workflow is selected.

The driver compares a fixed set of existing signing/experiment/Safe Browsing preferences before/after without creating absent preferences, verifies source/executable identities and requires successful joined Firefox exit before writing a diagnostic report. Failures preserve the domain and retain owners until retirement. Node models execute the actual API source against a controlled service; Python models exercise input and ownership guards. Neither is a live service observation.

The first clean `26fa6ca` owned execution failed at temporary loading, before the service query. The original boolean loader collapsed installation rejection and identity mismatch into the same result, so the cause is unresolved. No success report or protection verdict was obtained; retained browser/driver retirement and fresh final process/registration absence completed. This does not establish a protection-setting incompatibility.

The revised loader distinguishes bootstrap/file initialization/install/identity phases and returns only a bounded, sorted set of fixed error terms from the exact load attempt, never raw messages, paths or identifiers. Matched terms are observations, not a causal diagnosis; incomplete or unknown classification never authorizes loading or a retry. Failure records now include explicit retained-browser wait/exit receipts and available before/after owned protection readbacks. This revised instrumentation has model coverage, not a new live observation.

## Production integration still required

Before public capture can be enabled, the bridge must bind an independently validated native partial and its final filename/hash/signatures to the actual browser source/referrer/redirect history and any native redirect changes. There must be no caller-chosen path or general scanning endpoint. Missing/stale context, broker loss/restart, service uncertainty or unsupported policy must prevent final publication; a verdict must not be replayable onto another task or changed bytes. Existing file leases, immutable handoff IDs, cancellation proof and exclusive final promotion remain authoritative.

The native publication gate, durable protection context/recovery, metadata extraction, large-file semantics, other applicable browser policy checks and actual same-byte/provider acceptance are not implemented by this fileless probe. Public capture, persistent unsigned installation and final paired release remain open in [PROJECT_PLAN.md](PROJECT_PLAN.md).
