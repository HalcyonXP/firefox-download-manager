# Installed companion / Firefox diagnostic slice

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


## Prepared cross-origin scenario (not yet observed in Firefox)

`--scenario cross-origin` retains two owned loopback servers on distinct ports. The source preserves `/redirect?fixture=a%2Fb&x=1&x=2`; only its fixed302 Location points to the second server's extensionless attachment with the same query. Readiness supplies exactly one or two validated loopback origins, and reconfiguration disarms capture. Every redirect hop and final native offer must remain inside that set; ordinary production authority is unchanged.

The scenario requires one completed native task/correlated cancellation/independently correct output, temporary reload without replay, and an unarmed redirected Firefox fallback. For that fallback only, the download's observed source may be either the exact clicked redirect URL or exact owned attachment URL; both are predetermined members of this fixture chain, not arbitrary matching origins. Direct scenarios retain a single exact expected source. Both servers are retained in the existing fixture-owner list and joined by existing success/failure cleanup.

The retained two-server HTTP model verifies exact Location/query/body bounds and joined cleanup. It does not establish Firefox's cross-origin callbacks, distinct-host/DNS/TLS behavior, session eligibility or the public Hugging Face GGUF acceptance case. A live run of this changed driver remains required; the2df904b reports above remain scoped to their earlier source.
