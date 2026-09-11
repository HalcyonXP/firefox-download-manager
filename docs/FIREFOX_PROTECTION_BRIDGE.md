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
- `nsIApplicationReputationQuery` takes a raw-byte SHA-256 string, three levels of byte arrays for certificate-chain signature information, and an `nsIArray` of redirect principals. Its `fileSize` is currently a 32-bit unsigned long, consumed when constructing a remote request: large-file behavior must be reviewed before provider qualification. The probe's size is zero; it does not qualify large files.
- Experiment availability, privileged status and persistent installation are separate. The experiment preference's source fallback is not an observation of effective profile settings. Temporary loading does not qualify ordinary persistent installation.
- Windows `DownloadIntegration.downloadDone` writes origin metadata separately and explicitly avoids `IAttachmentExecute.Save`; calling that entire completion routine would also perform unrelated permissions/notification operations. It is not a narrowly scoped substitute for a reputation query.

Source reference: Mozilla revision [`574c275bcf5b4f86198c979b7e61f4a844aba0ea`](https://hg.mozilla.org/releases/mozilla-beta/file/574c275bcf5b4f86198c979b7e61f4a844aba0ea/), particularly `toolkit/components/reputationservice/nsIApplicationReputation.idl`, `ApplicationReputation.cpp`, the installed `DownloadIntegration.sys.mjs`, `toolkit/components/extensions/Extension{,Parent}.sys.mjs` and `toolkit/mozapps/extensions/internal/AddonSettings.sys.mjs`. No vendor source is redistributed by the probe.

## Build and owned observation

Maintainer commands, not installation instructions:

```powershell
python scripts/build-protection-probe.py --output artifacts/<new-probe-directory>
python scripts/probe-download-protection.py --probe artifacts/<clean-probe-directory> --firefox "<Developer Edition executable>" --report artifacts/<new-report>.json --execute-owned-browser
```

The builder creates a unique diagnostic add-on ID and exactly six bounded XPI members. Source/ZIP CRC/payload hashes and manifest authority are reread; output is exclusive and restricted to ordinary paths below `artifacts`. The input validator is separate from, and does not broaden, either product XPI policy. Execution requires clean exact-source inputs, original fresh closed-app/registration preflights, a ticket before domain creation and a retained isolated Firefox owner. No native registration, companion launch, normal profile, general file API or signing workflow is selected. Default mode writes no individual protection preferences; the explicit fileless capability mode below has one narrowly checked experiment-preference override.

For a fresh capability-test profile, additionally pass `--enable-fileless-experiment`. This is default-off, refuses existing profiles and combined browser/companion ownership, and enables only `extensions.experiments.enabled`. Exact effective/default/user-override readback is required; signing and Safe Browsing retain the strict checks. The declared profile mode is included in receipts. See [the separate policy contract](FIREFOX_TEST_POLICY.md#separate-fileless-experiment-mode); the source-scoped observation below establishes temporary API access, not persistent installation.

The driver compares a fixed set of existing signing/experiment/Safe Browsing preferences before/after without creating absent preferences, verifies source/executable identities and requires successful joined Firefox exit before writing a diagnostic report. Failures preserve the domain and retain owners until retirement. Node models execute the actual API source against a controlled service; Python models exercise input and ownership guards. Neither is a live service observation.

The first clean `26fa6ca` owned execution failed at temporary loading, before the service query. The original boolean loader collapsed installation rejection and identity mismatch into the same result, so the cause is unresolved. No success report or protection verdict was obtained; retained browser/driver retirement and fresh final process/registration absence completed. This does not establish a protection-setting incompatibility.

The revised loader distinguishes bootstrap/file initialization/install/identity phases and returns only a bounded, sorted set of fixed error terms from the exact load attempt, never raw messages, paths or identifiers. Matched terms are observations, not a causal diagnosis; incomplete or unknown classification never counts as successful loading or authorizes a retry. Failure records now include explicit retained-browser wait/exit receipts and available before/after owned protection readbacks. The clean `5e90bc6` follow-up observed install-phase refusal with `experiment-apis`, `invalid-extension`, `privilege-required`; the retained browser exited0 and joined, no success report/service query occurred, and final process/key absence passed. Its fixed initial/final protection readbacks matched, but already showed Safe Browsing disabled by the test environment. [FIREFOX_TEST_POLICY.md](FIREFOX_TEST_POLICY.md) documents the obsolete automation opt-out, correction and limits on earlier evidence. Experiment availability remains separately disabled in the observed owned configuration; the corrected opt-out does not enable it.

## Source-scoped service observation

Clean `2b643c244d4e0177093a9f30a82611104a79aa05` driver/probe passed the fileless experiment-mode run. XPI SHA-256: `0dd997d0d54f5a818465ac5c7ccf89b79437515267df623b7e0e43b57e976e03`. Exactly one callback returned a settled **not-blocked** receipt; wrong-page and retired-context access refused, and repeating start returned the same receipt without another dispatch. No real file was downloaded, inspected or published.

The supported automation opt-out was false and its applied marker absent. The experiment preference alone had effective true/default false/user override true. Signing and all inspected Safe Browsing switches were true, matched defaults and had no user override. Both inspected extension-update switches matched enabled defaults; `app.update.disabledForTesting` was absent. The complete selected policy snapshot remained stable through shutdown.

Exact browser exit0/join, driver wait, source/input checks and fresh final process/registration absence passed. The report, six ZIP members and policy/receipt invariants were independently reread and verified. This corrects the earlier test-environment defect for this one scoped execution; it does not retroactively qualify older campaigns or establish a real-file verdict, persistent installation or full protection parity.

## Production integration still required

Before public capture can be enabled, the bridge must bind an independently validated native partial and its final filename/hash/signatures to the actual browser source/referrer/redirect history and any native redirect changes. There must be no caller-chosen path or general scanning endpoint. Missing/stale context, broker loss/restart, service uncertainty or unsupported policy must prevent final publication; a verdict must not be replayable onto another task or changed bytes. Existing file leases, immutable handoff IDs, cancellation proof and exclusive final promotion remain authoritative.

The native publication gate, durable protection context/recovery, metadata extraction, large-file semantics, other applicable browser policy checks and actual same-byte/provider acceptance are not implemented by this fileless probe. Public capture, persistent unsigned installation and final paired release remain open in [PROJECT_PLAN.md](PROJECT_PLAN.md).

### Next binding slice (proposed, not implemented)

1. Bind a native publication challenge to one immutable handoff ID, validation generation, complete 64-bit length, computed SHA-256 and intended final filename. Retain the existing `ValidatedPartial` lock/identity lease while pending; the new opt-in `validate_with_fingerprint` interface computes complete byte identity while retaining that lease, but no challenge or verdict gate is yet exposed. Ordinary task completion remains unchanged; see [INTEGRITY.md](INTEGRITY.md#opt-in-fingerprint-interface). Changed bytes, filename collision/reselection, cancellation or a retired lease invalidate the challenge.
2. Bind the challenge to the actual captured browser source/referrer/redirect context and all relevant native redirect observations, not caller-provided arbitrary URLs or paths. Missing context, bounded-history overflow or changed resource identity must refuse publication. Define sensitive-context storage and restart behavior before enabling capture; an expired/lost session is not permission to publish or replay.
3. Investigate a first **Firefox-classified non-binary** slice with the existing reputation service still performing its URL checks. Reviewed source skips certificate parsing for non-binaries and reads the32-bit size only in `SendRemoteQueryInternal`, after the non-binary completion path. This is not a filename-only waiver: the real service call, exact context and native byte binding remain mandatory. Binary files stay Firefox-owned until signature extraction/remote-query semantics are supported. Oversized fields must never be truncated or fabricated; any unexpected metadata access must fail closed. These source findings do not establish a large-file or provider result.
4. Consume a successful, current, same-binding verdict only at exclusive final publication; a response queued on a pipe is not delivery or authorization. Broker loss/restart, stale/duplicate results, changed context and blocked/unavailable policy must keep the partial unpublished with explicit recovery. Verify these failures and the same-byte positive path before broadening the loopback candidate.
