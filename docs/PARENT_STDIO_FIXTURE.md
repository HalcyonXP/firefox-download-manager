# Parent stdio fixture preparation

## Scope

This is a separate diagnostic for eventual Firefox SDK process/pipe/lifetime observation, **not a Manager controller, protected dispatcher or install candidate**. No browser execution is established here. The normal extension, capture candidate and paired package do not select it.

Here **fileless** means no download-data file, scan or publication. The diagnostic still needs executable, manifest and build inputs; the SDK controller also requires an owned disposable profile. It is not the existing fileless reputation-service probe, whose runner deliberately has no native-process or registration operations. Sharing that adjective does not make their execution or profile modes interchangeable.

| Component | Entry point | Contract |
| --- | --- | --- |
| Native fixture | `crates/test-server/src/bin/parent_stdio_fixture.rs` | Opt-in std-only binary; fixed private argv and bounded stdio; no engine, network, downloads, registry or descendants |
| Session | `extension/parent-probe/session.js` | One attempt, exact fixture records/PID, retained operation and launcher retirement |
| Bootstrap | `extension/parent-probe/api.js` | Explicit SDK globals/imports, fixed no-argument API and nonce-correlated plain observations |
| Background/schema | `extension/parent-probe/{background.js,schema.json}` | One call, no heartbeat/wakeup/network/capture, no caller-selected native data |
| Builder | `scripts/build-parent-probe.mjs` | Fresh CLI compiler process, fixed owned inputs, five bounded payloads; no archive or browser launcher |
| Archive input | `scripts/qualification/parent_input.py` | Externally pinned compiler domain/source/image/record, exclusive five-member archive and bounded independent readback; no compiler/native/browser execution |
| Observer | `scripts/qualification/parent_observer.js` | Separate named chrome sandbox, parent-process check, two bounded original JSON strings, exact collector removal controls |
| Observation client | `scripts/qualification/parent_observation.py` | One retained automation owner/collector, strict monotonic receipts and sticky command uncertainty; no browser/native launch |
| SDK disable controller | `scripts/qualification/parent_run.py`, `parent_control.js` | Separately opt-in fresh-profile exchange/disable orchestration, exact retained Firefox owner and refusal-preserving cleanup; modeled only, no CLI or compiler |

The bootstrap intentionally supplies **owned fixture metadata**, not `NativeManifests.lookupManifest`. It does not inspect or change shared native registration. A fixture result therefore cannot qualify registry lookup, installed-image checks, private native IPC authentication or browser policy.

## Fixed native contract

Cargo disables automatic binary discovery and exposes `download-manager-parent-fixture` only with `parent-stdio-fixture`. It is never a package payload. A reviewed owner must place its independently identified bytes at the fixed `download-manager-native-host.exe` name in its own domain. The accepted argv is exactly:

```text
--browser-parent <same-directory/com.halcyonxp.firefox_download_manager.json> download-manager@halcyonxp.local
```

The fixture does not open that manifest. Filename/argv checks are diagnostic structure, **not executable attestation or caller authentication**.

It writes a fixed stderr line and a length-prefixed ready record containing its process ID. It accepts at most one exact UTF-8 ping containing `π`, writes one fixed pong, then waits for frame-boundary EOF. Lengths are nonzero and at most1MiB; incomplete, unknown or repeated frames refuse. Write/flush failures refuse. Success exits0; refusal exits2. No raw input, path or error is logged.

The session compares the native PID with the returned SDK owner's PID. Failed metadata extraction must still return that acquired owner to the retained transport for retirement. A rejected SDK invocation remains indeterminate; no replacement is launched. The background cannot supply a host, argv, URL, hash, verdict or arbitrary frame.

## Observations, not authority

Records carry `scope: "owned-parent-stdio-v1"` and `qualification:false`:

- **echoed** records one completed fixed exchange. It is not private Manager readiness, current browser authority, task completion or a policy decision.
- **retired** includes the launcher's separate startup/process/actual-pipe/callback observations. Success also requires a completed exchange and no session failure. Early pipe rejection, process absence and notification dispatch are not joins.

The observer receives only nonce-correlated plain JSON, not the API object or retained owner. The nonce is correlation, not authentication. A future external controller must observe the receipt itself and independently retain exact process ownership; notification return alone cannot prove delivery.

Context/extension closure uses the existing launcher guards. Session callbacks never await their own retirement or a response from their serialized read loop. Retirement separately waits for the launcher and running operation. A regression exposed unhandled rejection when a synchronous close hook initiated retirement and its later observer notification failed. Rejection is now observed immediately while retaining the original failed promise; it is not converted into a successful or delivered receipt. The existing native-port scheduling tag, shutdown barrier and3-second exact-owner retirement timer remain unchanged. Explicit shutdown is not prevented by idle keepalive.

## Build boundary and checks

The builder accepts only a canonical, ordinary, new diagnostic subdomain immediately beneath `artifacts`, with fixed native image/manifest names. It verifies exact manifest bytes and rereads source/image hashes around compilation. Hash/path checks do not establish executable provenance; builder tests deliberately use **non-executable metadata bytes** and never launch them.

The output is exactly `api.js`, `background.js`, `schema.json`, `manifest.json`, `LICENSE.txt`, plus a separate nonqualifying hash record. The generated manifest has only `nativeMessaging`, a nonpersistent background and its one diagnostic experiment namespace; no sites, cookies, downloads or capture permissions. The stable Manager add-on ID is valid only in a fresh owned profile, never an existing installation.

Compilation is CLI-only, with `ESBUILD_WORKER_THREADS=0`, `ESBUILD_MAX_BUFFER=16777216` and no binary override set **before Node starts**. An initial test imported esbuild before changing its worker environment and timed out before bundle output. Esbuild caches worker support at module initialization; a later worker inherits the changed environment and may not recognize its worker role. The failed test has no established retained join receipt and remains nonqualifying. Corrected tests use fresh bounded synchronous CLI children and preserve their exit/wait records; no SDK worker, timeout or test-concurrency workaround was introduced.

Component evidence currently comprises four Rust units, seven session models and three builder/bundled-SDK cases. The bundled model exercises actual bootstrap/session/launcher/transport bytes with modeled SDK owners, explicit encoder/decoder import, cross-realm detached framing and process-versus-pipe retirement. Fifteen targeted mutations were rejected and restored: six Rust, seven session, one API-argument execution case and one compiled-global source-policy case. Those models are not actual Firefox or native-fixture-process evidence. An initial standalone execution attempt refused its original closed-app preflight before domain creation or fixture launch; the guard was not relaxed.

A subsequent standalone batch at clean `e852421d151aa2b8a1b863539be8a11527ff47f5` passed five cases after a retained clean-source build. The image SHA256 was `b8c635cc822d87afe837de6d631cb4cd8078aba1cd5346deebf3b99adf825b74`. Roundtrip and boundary EOF exited0; unknown frame, repeated frame and ordinary argv exited2. Every case independently compared expected output and fixed stderr; each permitted ready frame also matched the retained process PID. All five observed natural exit and retained process waits plus reader joins. Original closed-app/native-registration-absence preflights passed before each case and afterward. No Firefox, profile or registration operation occurred. This is **standalone Windows process evidence**, not SDK process/pipe/realm, authentication or installed acceptance.

### Archive input boundary

`BuildExpectation` carries the retained controller's exact compiler domain, source commit/dirty state, nonce and image/build-record hashes. These values must originate outside the input being inspected. A caller that merely copies self-reported hashes has not established compiler provenance, image ownership or execution authority. The module does not launch a compiler or validate a compiler wait receipt itself.

The domain is pinned because the compiled API embeds absolute native paths: moving the archive inputs cannot silently redirect their ownership to another domain. Current source bytes, the exact native manifest, all five payload hashes and the closed extension manifest are checked. `clean=False` is an explicit diagnostic readback option, not permission to relabel a dirty build clean. A reproduced model exposed Python dictionary equality accepting numeric0 in place of `persistent:false`; typed JSON comparison now rejects that substitution.

Packing exclusively creates `parent-stdio-fixture.xpi`; an archive appearing after validation is not overwritten. Readback requires an externally retained archive hash, five exact uncompressed ordinary members, bounded sizes, no duplicate/extra members, comments or extra fields, actual member-byte equality and a final input/source recheck. Failed or partially written domains remain for their owner to handle. These checks are integrity boundaries, not protection against an actively hostile same-user process or permission to use an existing profile. Bundles embed owned paths and remain private, nonqualifying test inputs, never release/CI publication inputs.

Twelve metadata-only Python tests and eight rejected/restored mutations cover the compiler-domain pin, external build hash, dirty-state pin, typed manifest, actual archive bytes, final reread, exclusive output and image hash. A retained CLI compiler using **non-executable native metadata bytes** also passed builder-to-archive interoperability. The final validator independently reread that archive with unchanged compiled sources; no native process or Firefox was launched by this check.

Maintainer checks, not installation instructions:

```powershell
cargo test -p download-manager-test-server --bin download-manager-parent-fixture --features parent-stdio-fixture --locked -j 1
node --test scripts/parent-fixture-session.test.mjs scripts/parent-fixture-build.test.mjs
python -m unittest discover -s scripts -p test_parent_input.py
node --test scripts/parent-observer.test.mjs
python -m unittest discover -s scripts -p test_parent_observation.py
```

## Independent observation boundary

The observer/client is a **driver component**, not a complete live driver or a new profile-mode authorization. The caller must separately own and review its disposable Firefox profile/process. No product entry, preference setting, browser launch or native launch selects this component.

The collector lives in a separately named Marionette chrome sandbox and requires the default parent process. Its controller creates a fresh collector ID distinct from the fixture nonce. Both are correlation labels, not native/browser authentication. Snapshot/removal commands require the same collector; an occupied slot is never replaced or adopted. The exact observer inverse is retained before registration, including registration that acts then throws. The immutable slot remains after removal, so the same sandbox cannot be rearmed.

Only two original ASCII JSON strings of at most8192 bytes each are retained. No API object, extension context, process owner or raw exception is retained in the records. Foreign nonces are ignored; malformed/oversized data, subjects and excess matching notifications cause sticky refusal. Original JSON is preserved rather than parsed and reserialized: the external Python parser must reject duplicate members and nonfinite numbers itself. Invalid records do not become public report content.

Observer removal is separate from native retirement. It first observes a synchronous, collector-bound positive control, then calls the exact removal inverse and checks that a subsequent control does not invoke this observer. It never enumerates unrelated observers. A model first exposed a false removal result when both removal and notifications were silently suppressed; requiring positive control prevents that. Cleanup still attempts the inverse if that control fails. Removal exceptions or late callbacks remain failures, and removal is not blindly retried.

The external validator requires an immutable cumulative prefix, ordered echoed/retired records, the same positive u32 PID, exact booleans and every nested startup/process-exit/actual-pipe/I/O/hook observation. Master `successful:true` cannot replace those checks. Repeated polling of identical records is allowed; duplicate notifications, rewritten prefixes, truncation and PID changes refuse. Exchange, SDK retirement and collector removal are distinct checkpoints, all `qualification:false`; none independently joins the outer Firefox process.

The client retains one verified automation process identity and one named sandbox, restores content context after commands and attempts removal even after uncertain delivery. A reproduced lost-command model initially allowed later valid records to restore success. Command failures now invalidate evidence permanently while retaining cleanup. A separate interruption regression confirmed that cancellation must also invalidate evidence; interruption still propagates rather than being swallowed. A missing/replaced sandbox or failed cleanup cannot create a replacement observer or a successful receipt.

Eleven actual-source observer models, nine Python receipt/client models and thirteen rejected/restored mutations pass. The bundled API/session/launcher/transport model now delivers its actual JSON into a separate collector sandbox; an independently waited Node run also passed those records through the Python validator. These are modeled SDK owners, not native or Firefox execution.

Matching Firefox source review at revision `574c275bcf5b4f86198c979b7e61f4a844aba0ea` covered `remote/marionette/{driver,evaluate}.sys.mjs` and `xpcom/ds/nsObserver{Service,List}.cpp`. Scripts are function-wrapped with an appended asynchronous callback; named sandboxes are cached only while their window remains valid and unchanged. Observer notification invokes a cloned observer list synchronously. Removal during service shutdown can return without acting, whereas notification refuses during shutdown; a returned removal call alone is not evidence. This source review does not establish live realm/global/wire compatibility.

The current client requires a readable owned chrome realm. Explicit add-on disable followed by observed native retirement can be tested before Firefox exits; whole-browser shutdown needs a separately reviewed way to observe late receipts and retain outer/native lifetime evidence. No such late shutdown delivery path is implemented. The separate first-case controller below retires the native fixture before Firefox shutdown instead.

## SDK exchange/disable controller

`ParentRun` is a separate default-off controller library. Its exact-boolean `parent_stdio_experiment=True` mode is not selected by the service-probe CLI or any product. It requires 64-bit Windows with assertions, a retained already-created `DomainPlan`, externally established clean build/archive expectations and an independently pinned Firefox image. There is no compiler or runnable CLI here: the separately reviewed outer supervisor must retain the build/process owners and `ParentRun` **before** calling `execute()`. Metadata-only image domains must never be supplied for execution. Hashes and the build expectation class alone do not establish executable provenance.

The controller exclusively claims a new run ticket before preflight; neither a consumed controller/domain nor an existing profile can be reused. Cleanup before execution also closes that controller permanently. Original closed-app/all-view registration guards run before preparation, immediately before browser ownership and after retirement. Inputs are rechecked before launch, before temporary loading and after shutdown. Isolated home/local/roaming directories and an exclusively new profile are mandatory; no registration or normal-profile operation is selected. This distinct profile mode uses the existing exact experiment preference tuple, not a new protection exception. See [FIREFOX_TEST_POLICY.md](FIREFOX_TEST_POLICY.md).

The first case is ordered: retained Firefox owner and parent-process PID correlation → validated policy → independent observer installation → fixed-ID absence check → one temporary load → active fixed-ID temporary background metadata → one echoed exchange → one explicit add-on disable → successful SDK retirement receipt → checked observer removal → policy readback → exact Firefox wait/exit0 → final preflights/input checks → exclusive nonqualifying report. The fixed control script accepts only `absent`, `info` or `disable`, never arbitrary add-on IDs, enable/uninstall or native data. Disable completion and disabled metadata are not native retirement.

Load/disable attempts are marked before dispatch and never replayed after uncertain delivery. Cleanup attempts the exact observer inverse and retained browser even when preceding controls fail; cancellation propagates after those attempts. A start attempt without a returned process stays unknown. Missing native receipts cannot be replaced by browser exit, pipe absence or a PID. A modeled regression first showed a replaced browser process being adopted during close; the controller now preserves its original reference and refuses to close a replacement, including one reporting the same PID. A separate regression showed that a fresh profile alone did not reject an already-present fixed add-on ID; an explicit absence check now refuses before load or disable. This check is not atomic protection against a hostile same-user actor or arbitrary concurrent add-on replacement. Failed domains and owners remain retained, and no success report is emitted on refusal. The caller must not exit/discard a controller with unresolved ownership; this library supplies no name/PID-based termination fallback.

At `2b291fe`, eighteen Python controller models, eight actual-source control-script models and seventeen rejected/restored mutations cover ordering, exact modes/types, consumed domains, existing profiles, early/late faults, uncertain commands, interruption during cleanup, unknown native/outer lifetime, process replacement and nonzero exit. They execute no browser, native image or registry operation. The controller has **not** run in Firefox. Actual event-page idle, explicit-disable realm survival and SDK pipe/global compatibility remain unobserved. Whole-browser shutdown with an active native process is deliberately not this case; its late-receipt strategy remains open.

Maintainer model checks:

```powershell
python -m unittest discover -s scripts -p test_parent_run.py
node --test scripts/parent-control.test.mjs
```

### Readiness and fixed-ID consistency

The pre-load and cleanup `absent` receipt now has one definition: the public add-on lookup returned `null`, the XPI startup-state lookup and active-extension lookup both returned `undefined` for this exact ID, and AddonManager remained ready. The control awaits `readyPromise` and requires exact `isReady:true` before and after the awaited lookup, with another readiness check after the absence lookups. These are in-memory consistency observations, not complete filesystem absence, atomic exclusion of replacement or authentication. No unrelated add-on IDs are enumerated and no database load/repair, SDK patch or preference override is added.

Matching Firefox source review at revision `574c275bcf5b4f86198c979b7e61f4a844aba0ea` found that `AddonManager.getAddonByID` can produce `null` after a synchronous provider failure. Separately, `XPIDatabase.getAddon` catches database/repository failures and its wrapper can return `null`. Not every asynchronous provider rejection is swallowed: `promiseCallProvider` returns the provider promise without awaiting it in the try block. A public `null` alone cannot distinguish these paths. `XPIInternal.XPIStates.findAddon` checks the requested ID across its current locations without catching those lookups; `ExtensionParent.GlobalManager.getExtension` reads that ID from its map. The control reads `XPIExports` locally, not through cached internal exports across restarts.

Two modeled regressions reproduced a false absence receipt despite known own-ID XPI/runtime state and acceptance while AddonManager was not ready. A further case reproduced the weaker absence interpretation on the cleanup-disable branch; both branches now share the same checks. Twelve actual-source control models and seven additional rejected/restored mutations cover ready settlement, readiness loss during awaits/lookups, both exact-ID checks and cleanup consistency. These are not live SDK observations. Source review also confirms that ordinary `disable()` defaults to no system-add-on override, respects the enterprise disable policy and awaits the bootstrap-disable path; that promise still cannot substitute for independent native process/pipe receipts.

Source paths: `toolkit/mozapps/extensions/AddonManager.sys.mjs`, `toolkit/mozapps/extensions/internal/{XPIDatabase,XPIProvider,XPIExports}.sys.mjs`, and `toolkit/components/extensions/ExtensionParent.sys.mjs`. No native, Firefox or profile operation was executed for this hardening.

### Outer launcher versus browser parent

Windows Firefox may execute its launcher and browser parent as different processes. Matching `browser/app/winlauncher/LauncherProcessWin.cpp` source selects `eWaitForBrowser` for `--marionette`, creates and retains a browser process, then normally waits and forwards its exit code. That is a delegated launcher wait, not an independently acquired browser-parent handle in the Python controller. The source also has fallback paths; an outer exit code alone cannot establish every inner wait.

The current controller requires `Services.appinfo.processID` to equal its retained `Popen.pid`; it conservatively refuses a separate-parent topology before loading the XPI. It is not yet a general Windows Firefox owner. Actual topology has not been observed for this controller, and normal browser configuration was not inspected. Before live selection, review retaining the launcher and actual browser-parent lifetimes separately where needed, without disabling the launcher, changing mitigations or weakening the PID guard. Historical output/UI observations and waited outer launchers remain source-scoped; they must not be silently relabeled as direct browser-parent-handle joins.

## Before live selection

A complete independently supervised build/run batch still needs exact compiler/image/Firefox ownership review and actual SDK launch/pipe/realm observations. The controller models are not execution authority or a completed live batch. Idle, explicit add-on disable and browser shutdown remain distinct cases. Unknown startup or failed cleanup must preserve the domain and refuse success.

Archive integrity and standalone fixture evidence do not qualify persistent installation, actual event-page lifetime, private helper readiness, one-controller integration, request/handoff/context association, complete policy enforcement or real-file publication. Those remain requirements in [FIREFOX_PROTECTION_BRIDGE.md](FIREFOX_PROTECTION_BRIDGE.md) and [PROJECT_PLAN.md](PROJECT_PLAN.md).
