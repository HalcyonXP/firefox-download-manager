# Installed companion / Firefox diagnostic slice

> **Automation-policy correction:** the earlier shared driver used an obsolete opt-out and allowed Firefox automation preference overrides. Recorded behavior/output remains source-scoped, but does not establish unchanged default protection policy. See [FIREFOX_TEST_POLICY.md](FIREFOX_TEST_POLICY.md).

This opt-in harness is not install readiness, persistent-XPI qualification, physical-input evidence or final artifact acceptance. Source-specific live observations are recorded below; they do not qualify later changed bytes.

## Scope and ownership

`scripts/probe-installed-browser.py` extends the retained setup lifecycle in `qualification/installed.py` with `qualification/browser_installed.py`. Installation and removal retain the original fresh closed-app/registration guards. Firefox starts only with `BrowserPeer`'s exact retained setup/companion witness and independently unchanged installation binding. Normal profiles, normal Programs folders and existing downloads are not fixtures.

The nominal path requires:

1. Install a clean paired development package into an exclusively owned domain; observe its setup-retained visible companion/tray.
2. Build a separate temporary diagnostic XPI from the actual coordinator, journal, native connection, UI, capture policy and passive click observer. Packaged XPI bytes are not modified.
3. Open an owned Firefox profile; verify native readiness, the exact owned native destination and fixture origin before arming. A trusted ordinary fixture link must produce one correlated cancellation decision/NS_ERROR_ABORT, one Completed Manager task, the matching UI row and independently correct1408-byte output. Competing Firefox output refuses the slice.
4. Observe successful Firefox exit with the companion still retained. Restart that same owned profile and explicitly reinstall the temporary XPI. The same task identity must remain visible without replay. Unarmed Firefox must complete an independently correct fallback download without another Manager task.
5. After browser retirement, independently reconnect a native peer and require the same committed handoff/task identity. Join native/fixture resources, observe successful Manager exit through setup, uninstall the exact owned binding, preserve output and join setup.

The diagnostic uses Manager's add-on ID only in the owned profile because the installed native manifest permits that ID. This is not a unique-ID no-registration API probe. Host permissions and request filters are loopback-only; the policy and wrapper additionally bind one or two exact owned fixture origins, as described in the cross-origin scenario below. No cookies/downloads/all-sites permission is granted. Inspector controls accept only the extension's exact inspector page and closed message shapes. Observations contain bounded counts/classes, not URLs, paths, names, header values or arbitrary error strings. The owned-profile download terminal is queried through the retained browser's diagnostic channel, not a production downloads permission.

Every browser is retained before launch, including failed starts. Browser retirement errors prevent uninstall and success, but cannot skip an attempt to quiesce the owned Manager. Failure-only retention includes browser processes. Successful reporting requires both browser parents to exit0, joined resources, successful Manager/setup exits and fresh final registration absence. Named-helper absence is not a separate successful-exit receipt for each Firefox-managed child.

The diagnostic compiler uses the existing reviewed esbuild synchronous subprocess path, with worker-thread service disabled and a16MiB output bound in that child environment only. Compiler completion waits for its exact process; a failed deadline retains the parent until natural exit rather than killing Node and abandoning a compiler child. No dependency, global environment or normal build selection changes.

## Commands and evidence boundary

Only after fresh closed-app/ownership checks and review of the package/driver:

```powershell
python scripts/probe-installed-browser.py --package artifacts/<clean-paired-package> --firefox <Developer-Edition-executable> --report artifacts/<new-report>.json
```

The existing nine `--fault` checkpoints are supported. Their names denote the current transfer implementation's observation points; for this slice `bridge-started` follows Firefox's real native connection, not an independently launched qualification peer.

Local model/build evidence is separate from the live observations below. A clean older paired package and a changed diagnostic driver/XPI must keep separate source identities. Remaining work includes live fault/recovery cases, production activation/off-on controls, cross-origin eligibility, persistent unsigned exact-XPI restart/click behavior and final-main artifact qualification.


## Live diagnostic observations

A dirty development driver based on6837d3b passed the nominal sequence against clean paired packagec5f1ee9 on Windows11/Firefox Developer Edition156. Report `browser55-third.json` records the temporary XPI SHA256 `adca8b7105fa215c95a4b82bb92d7472f0e91915f2d57d1ce57c76bae0e86f19`, package descriptor SHA256 `69cd648cee93a68843b53aed1411a4062c2fde52f67bf79d0641356fd2342f87`, correct1408-byte Manager/fallback output, same task identity and independent committed receipt, two successful browser exits, successful Manager/setup retirement, uninstall and fresh final absence. This is an identified dirty-source diagnostic, not exact-head, production interception or persistent installation.

All nine injected cleanup checkpoints were then exercised against that package with retained CLI waits. Each refused a success report, recorded its intended failure stage with no cleanup errors, and passed fresh final app/registration absence. Domain-created launched no setup; the other eight joined it. Seven post-install cases observed uninstall. This evidence precedes the later missing-terminal scenario addition.

Two earlier runs refused after restart: UI navigation had replaced the dedicated inspector page, so its strict sender check no longer accepted messages. The second run independently observed a successful fallback download before that refusal. Keeping the inspector in its own tab corrected the harness; original failures and successful cleanup/uninstall remain separate evidence. A modeled tab-role regression covers this distinction. Observer navigation is not a browser-cancellation or native-completion signal.

`--scenario missing-terminal` adds the recovery case now observed at the checkpoint below. It withholds a real correlated terminal callback only from the coordinator, preserving an uncertain `intent`. It requires no native transfer/output before or after restart, then uses the actual confirmation UI to continue the same task and checks correct output/fallback/retirement. Its first attempted execution was refused by the unchanged preflight before any test domain or setup launch; that refusal is separate from the later successful execution.


## Updated phase-aware diagnostic

The current driver requires a paired helper advertising `task_handoff_phase` before arming. Closed probe observations now carry only the native task's phase/state/byte count; Prepared-before-confirmation and Committed-after-completion must match. The actual Manager rows must offer only Open folder at the Prepared and Completed checkpoints, including after restart. This intentionally excludes the olderc5f1ee9 package from new runs without changing the scope of its earlier successful report.

The missing-terminal scenario verifies the actual continuation warning through GetAlertText before AcceptAlert, using the existing owned-browser confirmation helper. The original unexecuted scenario omitted this dialog step; model tests now require its ordering and refuse an unexpected warning. No automatic prompt acceptance preference or normal profile is changed. New-pair live observations are recorded below.


## Clean phase/recovery observations (2df904b)

Clean driver and paired package2df904b passed both nominal and missing-terminal scenarios on Windows11/Firefox Developer Edition156. Reports `browser57-nominal.json` and `browser57-missing-terminal.json` record `harness_worktree_dirty:false`, package descriptor SHA256 `00a3de84c8e97493a1620ae735ccefc9cd6f1479f678c87dc9e14731cc2582a2`, successful browser/Manager/setup retirement and exact uninstall/final absence. Preserved Manager and Firefox fallback files were independently reread against the1408-byte fixture after both runs.

The missing-terminal run observed real browser cancellation, withheld only its coordinator callback, and proved Prepared/Intent with no native request/output before or after browser restart. The actual continuation button and verified warning then authorized the same task, producing independently correct output plus Completed. Both runs checked phase-aware Prepared/Completed controls as applicable and independent committed receipts. All nine intentional cleanup checkpoints then passed expected-stage/no-success-report/no-cleanup-error assertions with retained CLI waits and fresh final absence; domain-created launched no setup, eight joined setup, seven observed uninstall.

Temporary XPI SHA256 values were `4fa4f73301b9d5214949b6a121be4767d291e0c22d7e717529073d8860d8ee35` (nominal) and `40ba63ec1b6fae6b9ce71f76ca027864d2dd997628e6092265967b677f1e150f` (missing terminal). Independently hashed archive payloads match; archive identity remains separate per build. These reports still assert `qualification:false`: temporary reloading is not persistent unsigned installation, production activation, cross-origin/provider acceptance, physical input or final-main qualification. Later code changes need their own observations.


## Cross-origin scenario

`--scenario cross-origin` retains two owned loopback servers on distinct ports. The source preserves `/redirect?fixture=a%2Fb&x=1&x=2`; only its fixed302 Location points to the second server's extensionless attachment with the same query. Readiness supplies exactly one or two validated loopback origins, and reconfiguration disarms capture. Every redirect hop and final native offer must remain inside that set; ordinary production authority is unchanged.

The scenario requires one completed native task/correlated cancellation/independently correct output, temporary reload without replay, and an unarmed redirected Firefox fallback. For that fallback only, the download's observed source may be either the exact clicked redirect URL or exact owned attachment URL; both are predetermined members of this fixture chain, not arbitrary matching origins. Direct scenarios retain a single exact expected source. Both servers are retained in the existing fixture-owner list and joined by existing success/failure cleanup.

The retained two-server HTTP model verifies exact Location/query/body bounds and joined cleanup. It does not establish Firefox's cross-origin callbacks, distinct-host/DNS/TLS behavior, session eligibility or the public Hugging Face GGUF acceptance case. The later clean1cde75b observations below establish the owned-browser slice; the2df904b reports above remain scoped to their earlier source.


## Clean cross-origin observations (1cde75b / paired2df904b)

Clean driver1cde75b against the unchanged clean paired2df904b helper/setup passed nominal, cross-origin and missing-terminal scenarios on Windows11/Firefox Developer Edition156. Reports `browser58-nominal.json`, `browser58-cross-origin.json` and `browser58-missing-terminal.json` preserve separate harness/package identities and `harness_worktree_dirty:false`.

The cross-origin run observed the trusted extensionless302/query path through two distinct owned loopback origins, correlated cancellation, one independently correct1408-byte Manager output+Completed, temporary reload without replay, and a correct unarmed redirected Firefox fallback. Missing-terminal again required Prepared/Intent/no native output across restart followed by the actual verified continuation warning and same-ID completion. Each run required successful browser/Manager/setup exits, retained resource retirement, uninstall and fresh final app/registration absence. All six preserved Manager/Firefox output files were independently reread against the fixture afterward.

Temporary XPI SHA256 values: nominal `450cb76aa53cf643b4327b35d7cd2baaed47f5882b60013f503681850e15675c`; cross-origin `f00bb5e75e47645bcad41f8e11af39dbd33ab1a45827bd3fe3eb084849c7a2b8`; missing-terminal `b40ce5f5573162baa4fc3f551e9fe4190bea20eb0abf0301c120c6acb11408e7`. Independently hashed payload inventories match across all three archives. The paired package descriptor remains `00a3de84c8e97493a1620ae735ccefc9cd6f1479f678c87dc9e14731cc2582a2`.

These three positive observations do not transfer the earlier nine fault results from2df904b to1cde75b. Distinct loopback ports establish origin separation, not distinct-host/DNS/TLS or public-provider compatibility. All reports remain `qualification:false`, temporary-XPI and not persistent unsigned/final-main/physical-input/install-ready evidence. Production capture is still unselected; the public Hugging Face GGUF criterion remains open.


## Prepared no-native-transfer recovery scenarios

`scripts/probe-installed-recovery.py --package <paired> --firefox <Developer-Edition> --scenario unlinked|aborted-terminal --report <new-report>` reuses the unchanged installed lifecycle, browser-only retained-peer witness and original closed-app/registration guards. It is opt-in; CI runs only fileless models/build checks, not this browser entry point.

`unlinked` prepares one fixed owned-fixture reservation outside the journal. The diagnostic binds verified destination/origin, loaded history and initially empty native tasks, and retains a seed-attempt flag before dispatch so an uncertain preparation cannot become another UUID. `aborted-terminal` captures a real fixture click/terminal, but a separately armed **diagnostic-only** command proxy substitutes an actual native Abort for the coordinator's commit attempt. No terminal or receipt is fabricated; the actual Aborted receipt leaves Cancelled visible. Production bundles never import this fault proxy.

Both scenarios require the same task and unresolved presentation across temporary reload, the actual cleanup button and exact warning before accepting the alert, one retained Aborted identity, zero native output/extra HTTP requests and independent native status after browser retirement. Only a subsequent explicit unarmed click produces the independently correct Firefox file. The report states `native_output_bytes:0`, `output_owner:firefox` and `native_handoff_phase:aborted`; its common size/hash fields describe that Firefox file, **not Manager completion**. The existing two-successful-browser-exit, Manager/setup joins, fixture retirement, exact uninstall and final absence requirements remain.

Models reject invalid/boolean byte counts, unsolicited requests/output, unexpected warnings and committed/unknown snapshots. The source-scoped observations below now cover these recovery scenarios. Native history/tombstones remain retained; neither acknowledgement nor unlinked discard is expiry or deletion authority.


## Clean recovery observations (6f8b209 and23bfa69 / paired2df904b)

Clean6f8b209 nominal execution passed one correct1408-byte native output+Completed, temporary reload/no replay and independently correct Firefox fallback (`recovery59-nominal.json`). The first unlinked execution reached uninstall and successful browser exits, but refused success reporting because the recovery subclass had not initialized the Firefox executable hash. Cleanup joined setup, observed uninstall and reported no cleanup errors; no success report was created. A fileless regression reproduced the missing identity before launch. Commit23bfa69 records the initial hash, preserving the existing final comparison rather than weakening it.

Clean23bfa69 unlinked and aborted-terminal executions then passed (`recovery60-unlinked.json`, `recovery60-aborted-terminal.json`) against unchanged clean paired2df904b on Windows11/Firefox Developer Edition156. Both observed the actual warning, same-ID Aborted status across recovery, no native transfer/output, an explicit correct1408-byte Firefox retry, independent native status and joined browser/Manager/setup/fixture retirement, uninstall and fresh final absence. Preserved Firefox outputs and empty native destinations were independently reread. These are Firefox-only output reports, not Manager completion reports.

The three observed temporary archives have matching payload inventories and SHA256 values: nominal `720feb8c3a44434c00e9b71341f6d8dfd73722a4be7195587445f4d95577070d`, unlinked `ae37ce0408ce336b1160699cc4e2e4f18da32ff54698da07764cd9a41517ccb7`, aborted-terminal `a2745f20a98c1ae56eb375022207fc2b5e8f7638d6dad061b15a343ffc89b6f6`. The different harness commits remain explicit; the hash initialization changed no extension/native bytes. These observations do not qualify subsequent capture-control changes, persistent unsigned installation, ordinary production activation, public-provider behavior or final-main artifacts. Earlier fault results retain their earlier source scope.

## Prepared capture preference scenario

`probe-installed-browser.py --scenario capture-toggle` exercises the actual checkbox against the armed loopback diagnostic: Off must produce a correct Firefox file and no native task/request/output; verified On then uses the existing one-task native completion path. Before reload it saves Off; after reload it must observe Off without reapplying it, arm the diagnostic gate, and obtain a correct Firefox file without native replay. The first completed owned Firefox fixture is independently verified and exclusively archived as `firefox-off.bin`; only its exact completed owned-profile history entry and original fixture file are removed, preventing later checks from accepting stale output. Unexpected history, bytes, requests, files or failed removal refuse. Original ownership/preflight/retirement guards remain. Clean4f5c006 execution now covers this scenario, as scoped below; production still has no selected interceptor/site authority.


## Clean capture-control regression batch (4f5c006 / paired2df904b)

Clean4f5c006 against unchanged clean paired2df904b passed all six scenarios: capture-toggle, nominal, cross-origin, missing-terminal, unlinked and aborted-terminal. Reports `capture60-<scenario>.json` record separate source/package identities and `harness_worktree_dirty:false`. The toggle case observed the actual checkbox: armed Off produced correct Firefox bytes and no native task/request/output; verified On produced one correct native output+Completed; Off remained saved after restart without being reapplied, and an armed diagnostic again left output to Firefox without native replay.

The other five paths passed their existing native/UI/cancellation/restart/recovery assertions. Every case required successful browser/Manager/setup exits, fixture retirement, uninstall and fresh app/registration absence. Eleven preserved output files, including the exclusively archived initial Off control, were independently reread; both recovery native directories remained empty. All six XPI hashes were independently checked and payload inventories matched:

| Scenario | Temporary XPI SHA256 |
| --- | --- |
| capture-toggle | `75e6b152edb9c346784260fdd17296f54b348d92146f07628099177b43acc106` |
| nominal | `829a095705f8dd6c78a9ea847622011d389287bcc911422f971ec81f54ac75ed` |
| cross-origin | `2efc57af374235e5833bfc288a5031dc63ad370b77c13428fc4e41baeca0ddf8` |
| missing-terminal | `023043669d756207d87183355ca9e0de4c83db2830fc93aa59132aa624a72415` |
| unlinked | `a9d1e4d0f3d8819e2ecec84cfcc72467c672be3afd758b93ea951756ec79e718` |
| aborted-terminal | `e760d319f08b40ae13c6bfeb5a4ef09fe2e31a28fcf056bee317ba5820f85b5a` |

These are temporary-XPI/owned-loopback observations, not persistent installation, distinct-host/DNS/TLS/public-provider, normal-profile or physical-input acceptance. Earlier fault results retain their older source scope. No native bytes, permissions or production capture selection changed for the batch. [Normal XPI installation observation](FIREFOX_PERSISTENCE.md) separately observed an owned-default signature requirement atbc36eb5/packaged2df904b, not persistent installation.


## Prepared container/private negative controls

`probe-installed-browser.py --scenario unsupported-contexts` uses the unchanged paired input and original setup/owned-peer/registration/browser lifetime guards. It creates a new container through Firefox's `ContextualIdentityService` and a private window through `OpenBrowserWindow`, only in the retained owned browser/profile. No normal profile, extension permission, protection preference or production capture selection changes. The new container record remains confined to the owned fixture profile; no container history is adopted or deleted.

Each context requires independently observed private-mode/container attributes, actual trusted fixture click, exactly one complete correct Firefox file, exclusive byte-checked archival (`firefox-container.bin` / `firefox-private.bin`), zero native task/pending/offer/output and exactly the expected fixture request count. Only that exact completed owned-profile download entry/file is removed before the next case. Capture must remain available, armed and verified On throughout, preventing disabled capture from passing the negative controls. The exact created tab/window must close and the prior handle set must be restored; the enclosing driver still owns/joins the browser process. The following default-store case requires the existing one-native-task completion, temporary reload/no replay, independent Firefox fallback, successful browser/Manager/setup exits, fixture joins and uninstall.

The diagnostic adds a read-only observer under existing webRequest authority, restricted to the exact configured loopback origins and fixed attachment path. At most64 closed method/frame/store/private classes are retained; no raw request URLs, cookie-store IDs, headers or credentials are recorded. Overflow refuses capture. Container traffic must report a nondefault store. Private execution remains disallowed by the manifest, so its request must **not** appear in the extension: privileged owned-context inspection establishes private mode separately. This is not private request interception or session replay.

Seven new Python models exercise strict scalar/context/native-state checks, byte-checked exclusive archival, exact window-close dispatch and embedded JavaScript. A TypeScript model checks origin scoping and bounded classified observations. Installed Firefox156 module/API source inspection corrected the container service import to its current `moz-src` URI before execution. An initial archival model used a short-path temporary alias and was corrected to the canonical owned path; the path guard was retained. Complete npm gates/243 TypeScript tests/14 protocol examples and116 Python tests pass. Seven restored-source mutations reject omitted active-preference/native-work/context-class/evidence/window-close/origin/bound guards. Closed report records distinguish each context's private/store class, new extension observations, Firefox bytes and zero native tasks/offers. The cleanff5e960 execution below now covers this scenario. Two-port redirects, earlier contexts and older temporary XPIs do not qualify these changes.


## Clean context and regression observations (ff5e960 / paired2df904b)

Cleanff5e960ea378c560a20701f6026462895c344be8 against unchanged clean paired2df904b passed unsupported-contexts and six regressions: nominal, capture-toggle, cross-origin, missing-terminal, unlinked and aborted-terminal. Reports `contexts62-<scenario>.json` retain separate harness/package identities and `harness_worktree_dirty:false`.

The new context case observed a real nonzero container identity, nonprivate mode and one nondefault-store extension request classification. Its trusted click produced exactly1408 correct Firefox bytes, no native task/offer/pending/output and no extra fixture download request. The private window independently reported private mode and zero container identity; its click produced the same correct Firefox-only bytes without an additional extension request observation. Both cases required capture available, armed and verified On, then exact window closure. A following ordinary default-store click still produced one correct native output+Completed, temporary reload/no replay, independently correct Firefox fallback and a same-ID committed native receipt.

The private report's `container:default` means zero `userContextId`, **not** an observed `firefox-default` cookie store or private interception authority. Private-mode metadata came from the owned browser; private requests were withheld from the extension under unchanged private-execution denial. No cookies, container-management or private permission was added to the XPI.

All six related paths retained their existing capture/toggle/persisted-Off/redirect/missing-terminal/recovery assertions. Every case required successful browser/Manager/setup exits, fixture joins, uninstall and fresh app/registration absence. Fifteen preserved output files, including the two context archives and initial Off archive, were independently reread and matched the fixed1408-byte fixture; both recovery native directories remained empty. Seven temporary XPI SHA256 values were independently checked and all payload inventories matched:

| Scenario | Temporary XPI SHA256 |
| --- | --- |
| unsupported-contexts | `19a4ebc2d4875f11f64710375347c74243a524b4d83405d5cd12829144bd2312` |
| nominal | `ec82ed96f832fc50b59b3532f1108cc45753f907770d7c850b61e62729423152` |
| capture-toggle | `a32cce84fc56ea2f7105106ac967af006dcecbdcff1ce45fa5d9249e91e0ae93` |
| cross-origin | `f12738d88d2bf1c5e224b3b0b11b321ffb05257657bc9f749ee318872ac27b43` |
| missing-terminal | `847b46b03ae21a57c736883b55fccf96f5f27dfe00809673bbb4877d57b6ff1a` |
| unlinked | `83340d4065bdd8750f34d6d0b771a093d3faaf1d1c50f9110473ec1eb4a44500` |
| aborted-terminal | `54dc95a0996b72baff80d46af50af9bbfe7bd9e1623beb0dad717f9d89cc1766` |

These results establish owned container/private fallback and the recorded regression slice, not persistent unsigned installation, production activation/permissions, distinct-host/DNS/TLS/public-provider/session/race/new-tab, physical-input or final-main acceptance. Older fault checkpoints retain their own source identities. Native source/package, protections and production selection were unchanged; the separate owned-default unsigned-XPI refusal remains as recorded in FIREFOX_PERSISTENCE.md.
