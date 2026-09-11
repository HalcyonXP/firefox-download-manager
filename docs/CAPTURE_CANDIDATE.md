# Automatic-capture development candidate

**Implemented with source-scoped owned-browser evidence, not install-ready.** This is a candidate product entry, not the loopback diagnostic. It has no inspector, arming command, seeded task or fault-injection controls. Default `npm run build` and paired-package selection remain manual-only; immutable v0.1.0 artifacts are unchanged.

## Build and identity

From the repository root, with existing Node/Python dependencies:

```powershell
python scripts/build-capture-candidate.py --output artifacts/<new-candidate-directory>
```

The parent must already exist. The builder exclusively creates a directory below `artifacts`, refuses alias parents/previous output, uses the existing bounded synchronous esbuild compiler, and retains/waits its exact parent on an interrupted wait rather than killing it and abandoning a child. Eight fixed XPI assets are size/hash checked, create-new archived and independently reread. `BUILD.json` identifies source commit/dirty state and payload hashes; `candidate.json` identifies the XPI hash and explicitly records `candidate:true`, `qualification:false`. Build failures preserve the output directory.

The XPI is `download-manager-capture-candidate.xpi`, version0.2.0, stable ID `download-manager@halcyonxp.local`. It is unsigned and separately named. This directory is not a paired installer package and is not accepted by the ordinary package or existing narrow manual-XPI persistence input policy. A compatible companion must advertise both `prepared_handoff` and `task_handoff_phase`. Existing paired2df904b remains a separately identified native integration input, not a newly built or final qualified package.

## Authority and behavior

`extension/candidate/manifest.json` explicitly selects:

- Existing nativeMessaging, menus and own storage, plus webRequest/webRequestBlocking.
- HTTP(S) host authority and a passive `click.js` content script, top-frame only, no about:blank injection.
- Existing optional cookies for explicit manual session operations only. Automatic capture never requests/collects cookies or Authorization context.
- Existing private-execution denial and restrictive extension-page CSP; no web-accessible resources, external messaging or remote update URL.

The candidate has its own strict manifest validator; additional authority does not become permissible in the manual manifest. Firefox's normal installation/website-permission controls remain authoritative. No permission prompt, preference override or installation action occurs during building.

`automatic-background.ts` selects the existing coordination/policy without diagnostic arming. New cancellation authorization requires all of: completed listener activation, loaded/verified On preference, verified host/API permissions, connected native transport and both handoff capabilities. Missing/failed permissions, unsupported requests or an unavailable/incompatible companion leave Firefox in control.

`CaptureAccess` observes permission changes and immediately revokes authorization before asynchronous readback. Revision checks prevent old permission reads from restoring stale grants. Partial listener registration stays failed; it cannot recover into authority without a revocation observer. This state is independent of the saved capture preference and never changes existing transfers or native history.

The Manager UI separates the On preference from website authority. **Allow / check website access** requests only the declared website origins from its button handler. Required webRequest API permissions remain part of the independent authority check, not an optional permission request. Firefox validates optional-request declarations before filtering already-held permissions; requesting required API permissions would reject before displaying a prompt. It never requests on startup, never infers authority from the prompt's result and never automatically replays an uncertain request. A background readback follows approval, denial or failure. Off remains a separate immediate control for new capture authorization.

The underlying conservative policy is unchanged: prior trusted same-tab click, default-store nonprivate top-frame anonymous GET, bounded validated attachment/redirect chain and immutable terminal binding; observed cancellation precedes native commit. The cross-origin option is selected, but `capture-protection.ts` now limits candidate eligibility to canonical `http://127.0.0.1` origins (including explicit ports) while the protection gap below remains open. Every initial/redirect origin must pass; public downloads stay Firefox-owned. The broader declared website authority remains separately checked/revocable, not evidence that public capture is permitted. This boundary is not a protection verdict or public-provider/TLS/session acceptance.

## Acceptance still required

The source/build/permission models and prior loopback diagnostic observations are different evidence layers. The f573c19 candidate campaign below covers owned startup/click, actual Firefox permission denial/regrant with API revocation, Off/restart, unsupported contexts and retained installed lifecycle. Remaining race/provider/persistence/final-artifact cases and later changed bytes require separate qualification. Browser-protection implications and exact-artifact compatibility remain release gates, not consequences of unchanged preference values alone.

Exact unsigned-XPI persistence remains unresolved as recorded in [FIREFOX_PERSISTENCE.md](FIREFOX_PERSISTENCE.md). Do not rerun unchanged defaults, inspect normal profiles, change signature enforcement, introduce signing, or use a temporary load to claim persistence. [PROJECT_PLAN.md](PROJECT_PLAN.md) defines the delivery blocks and remaining acceptance.

Local verification covers complete npm/Python gates, strict candidate/manual manifest policy, real candidate build/ZIP readback, interrupted-wait models and targeted authority mutations. These source/build models remain distinct from the source-scoped live result below.

## Consolidated owned campaign

```powershell
python scripts/test-candidate-campaign.py --package artifacts/<reviewed-paired-package> --candidate artifacts/<clean-candidate-directory> --firefox "<Developer Edition executable>" --report artifacts/<new-report>.json --execute-owned-browser
```

Requires the original fresh closed-app/registration preflights and reviewed owned-state execution. It creates no normal-profile state. The candidate is independently checked against its clean source/hash/eight-asset inventory, then loaded without diagnostic arming into the retained isolated browser. This is **temporary loading, not persistence**; the narrow normal-install observation policy is not broadened.

The17 fixed harmless loopback cases cover direct and cross-origin capture, Off, navigation, POST, iframe, blob, new-tab, cookie/Set-Cookie/Vary fallback, container/private contexts, website revocation, denial, regrant and saved-Off restart. Revocation uses the public API only inside the owned candidate page; denial/regrant uses its actual button and the normal visible Firefox permission buttons, with a source/request-bound read-only witness. It does not directly grant permission or invoke a notification callback.

Success requires3 independently correct native files,13 Firefox-only archived files, exact UI task identities and settled journal/authority readback, two successful retained browser lifetimes, same companion lifetime, independent native Committed receipts, fixture/process joins, uninstall and unchanged output. Existing cleanup refusal/retention remains authoritative. Models and a real bounded HTTP/ZIP check are separate from execution reports; a completed report must identify its exact candidate/native inputs and harness source. This campaign does not establish public-provider, same-URL race, download-protection parity, persistent installation or final-artifact acceptance. CI now discovers all `test_*.py` policies/build-only models in one step, retaining earlier cases; no live-browser CLI runs in CI.

### Observed candidate campaign

Clean `f573c19a02214113117b55d803c5973d50368a8c` harness/candidate with separately identified clean paired `2df904b9e04a83a6bd48c1ae4d3e728d9c30f1fb` native input passed all17 cases. All16 output files, native receipts, input hashes/inventories and retained cleanup were independently checked; no campaign repair or retry was required. Candidate XPI SHA-256: `d52ed31a884dc9d80e8e310c9dba749a87c8ffbdf4348c66c4d33080cf94e375`. This result predates the later loopback protection restriction and native zone-marker source; it does not qualify those changed bytes. Two successful temporary-load lifetimes are not persistent installation.

## Download-protection integration gap

Firefox's `DownloadIntegration.shouldBlockForReputationCheck` obtains the saver SHA-256, signature information and redirect history before calling its application-reputation service. The current native handoff does not run that Firefox completion pipeline. Source inspection establishes the missing integration, not that a specific malicious file bypassed a verdict or that every file type would receive the same check.

Accordingly, unchanged signing/TLS/Safe Browsing preferences are **not proof of equivalent download protection**. Release selection and public-provider capture acceptance remain blocked on an explicitly reviewed solution that preserves the applicable protection requirements. No enforcement preference is changed, no protection waiver is inferred, and a temporary harmless-fixture pass cannot close this gap.

Matching Firefox source at [574c275bcf5b4f86198c979b7e61f4a844aba0ea](https://hg.mozilla.org/releases/mozilla-beta/file/574c275bcf5b4f86198c979b7e61f4a844aba0ea/toolkit/components/reputationservice/ApplicationReputation.cpp) checks source/redirect/referrer URL blocklists before its non-binary completion path. A filename-only restriction therefore cannot by itself remove this integration requirement. New native Windows [Internet-zone provenance](SECURITY.md#windows-download-provenance) is separate: it does not implement this Firefox service, an antivirus scan or a reputation verdict.
