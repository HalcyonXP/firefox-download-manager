# Actual Firefox artifact qualification — #28 (not release approval)

## Clean expansion and fresh-workspace CI follow-up

Clean committed driver `1a8ed778e6fc34d6f4e2aead60c4bc5391e08d98` subsequently passed all twenty Firefox checks, native26/default2GiB and the actual native-Windows11-x64 installer lifecycle on the same CI34341064343 candidate. Reports are `firefox28-ci37-clean1a8.json`, `native28-ci37-clean1a8.json` and `install28-native-x64-clean1a8.json`. This supersedes the draft-driver status for those bytes, not final-source/publication gates.

CI `34349940646` then failed two new policy tests because they tried creating a temporary child beneath a nonexistent `artifacts` directory. The populated local checkout had hidden that prerequisite. A fresh source-only fixture with an initially absent container reproduced both errors without browser/registry use. Every policy test now initializes its container; a nested fresh-workspace regression proves the two ownership tests run before packaging. Its unchanged baseline passes and removal of initialization fails the intended assertion. All twenty-seven policy tests pass after restoration.

That CI's package build was skipped. The unconditional lifecycle-observation step also failed to find an unbuilt test executable; **no release lifecycle test ran and no new candidate was produced**. Rust dependency policy passed and ARM lifecycle skipped. This is not a recurrence or reconstruction of #44's progress failure. The failed run remains preserved; the corrected new tip must pass CI before acceptance.

## Current MIT-bearing checkpoint (2026-09-09)

The owner confirmed both Firefox editions were closed and would use only Edge during testing. Fresh closed-process/all-four-view absent-registration checks passed. Clean `37c5ed2` reran the original nine checks against CI `34341064343`'s licensed package: source `3e8ae17644da1f75f9612bb49db9bef9424eeacd`, descriptor `f77548a3b61d9382ed934d74e8360ea991ff4e6b95def07aeb9f81f05e59dcc6`, ZIP `e274bcc5d7b7ba98f804ce5da858e6fb5f64d64a224e7c75d7ddba2e13c88b57`. Report `artifacts/firefox28-licensed-clean.json` resolves the licensed rerun gap **for that original slice**.

An expanded **dirty, source-hashed driver** now passes twenty checks on those same bytes (`artifacts/firefox28-expanded20-draft.json`). It adds:

- Actual toolbar pointer activation and link-context-menu capture; exact encoded/ordered link target reaches HTTP without an implicit cookie/referrer, and output matches independently.
- Cancel/Remove confirmations, preserved completed output, invalid checksum/non-HTTP input refusal, and actual UI-port reconnect without a duplicate task. UI-port reconnect is not a forced native-helper crash; the separate native matrix covers retained-handle termination.
- Real CSP violation events for an inline script, off-origin image and page-realm fetch; the script does not execute and no fixture request arrives.
- An actual private window has no usable toolbar/link capture. **Firefox156 can load the manager's HTML privately but withholds its extension APIs.** The real form reports non-submission, with no new task, fixture request or output. `incognito: not_allowed` is not a document-access-control promise.
- A 2 GiB transfer reaches the actual rendered Validating phase: Cancel is available and Pause absent. A DOM observer invokes the real Cancel control, the real confirmation is accepted, cancellation is acknowledged without final publication, and Remove deletes the retained 2 GiB partial/record. The observer does not mock state, hashing, confirmation or Native Messaging. This positive phase observation is not a guarantee of a minimum validation duration or UI scheduling SLO.
- Joined Pause proves actual v4 `[0, 2 MiB)` coverage and independently hashed disk bytes. Firefox/helper restart and explicit temporary-XPI reload preserve that prefix/checksum; explicit Resume completes without prefix refetch or automatic worker replay.
- After session-loss refusal, the fixture invalidates its old cookie. Fresh sign-in and a distinct opt-in Add collect the renewed cookie and complete, without replacing context on the old retained task.

The same draft shared driver separately passes native26/default2GiB and the actual installation adversarial lifecycle on **native Windows11 x64**, not merely hosted Server/ARM emulation. Signing remains `value: true, user: false`; ordinary profiles remain untouched. Clean committed-driver, new-tip CI, final artifact/source/tag and publication gates remain required. The historical sections below describe their original, narrower inputs—not current omissions silently carried forward.

### Expansion failures, corrections and cleanup review

1. An older guessed `CustomizableUI` module URI was unavailable. Installed-package metadata showed the moved module; the driver uses the already exposed owned-window UI API. No third-party implementation was copied.
2. Chrome `ElementClick` emitted a trusted primary click but opened no tab. A complete Marionette pointer move/down/up sequence did open the toolbar manager. That observed distinction is retained; the internal reason for the earlier event behavior was not established. No extension handler was replaced or browser protection disabled.
3. The first creation refactor filled the filename before the URL. The real URL-change listener intentionally proposed a new name, defeating the expected acknowledgement. The driver now orders URL before explicit filename, preserves a captured URL when requested, and checks the name before submission. Product behavior was not changed.
4. A private-document test wrongly demanded `about:neterror`. Bounded readback instead showed manager HTML/form present and the extension API absent. The corrected safety contract checks denied capabilities and actual non-submission, not an invented document prohibition; toolbar/menu denial remains required.
5. Cleanup review found domain removal preceding fixture join and weaker browser-generation verification than the installer driver. Shared read-only `installation.py` now verifies exact generation/manifest/helper/XPI bytes; normal cleanup refuses unrecorded bindings, and domain removal follows fixture/browser/native/setup joins. A private pre-allocation ownership record supplements verified-binding records; it never itself authorizes deletion or a success report.
6. During that refactor, an **unguarded replacement failed to initialize the binding set and left a second fixture construction**. One setup completed, then `NameError` occurred before Firefox started and again in cleanup. No success report was emitted. Browser runs stopped. A private read-only review tied the exact single generation/receipt and all creation times to the failed invocation, its absent/closed preflight and completed setup call; the new profile/download directories were empty. An explicit reviewed setup-CLI uninstall restored absence in all four views. No manual key deletion or automatic adoption by the qualification cleanup ran; the failed domain and review remain preserved privately. This is not a passed lifecycle or general authority to clean plausible-looking registrations. Initialization now precedes allocation, one fixture is constructed inside protection, and failure-path tests cover these exact boundaries.
7. An initial mutation command used the wrong working directory, invalidating ownership-test baselines. Those logs are not accepted mutation evidence. Correct repository-root invocations first passed each unchanged baseline, then rejected missing pointer sequences, allowed private APIs, duplicate fixtures, a missing owner set and omitted fixture joins. Exact source was restored; all twenty-six policy/real-HTTP/transport tests passed. These policy tests do not launch Firefox or mutate registration and remain separate from the actual twenty-case run.


## Owner clarification after these checkpoints

[ADR 0011](decisions/0011-license-and-available-qualification.md) supersedes the earlier unresolved licensing and separate-clean-machine prerequisites: permissive FOSS is implemented as MIT; the personal release will be qualified on the owner's existing native Windows 11 x64 machine with isolated owned state/profiles. Clean-machine evidence is unavailable, not passed. All other listed functional/safety work remains. Later clean-driver/CI success at `5ce837c` is recorded in [QUALIFICATION_PLAN.md](QUALIFICATION_PLAN.md); the current MIT-bearing results are recorded above; final release inputs still need their own evidence.

## Outcome and inputs

The confirmed project outcome remains a qualified Windows 11 native-x64 / Firefox Developer Edition release. This document records a **subset of that qualification**, not a narrowed release requirement.

Draft PR #43 foundation `0ccc6ea0f39e8e8de74318d45d9a7b4b2100c6ac` passed all three CI jobs in `34292916341`. The downloaded candidate was built from synthetic PR merge `dc84d98a168d947f3d967831423a4841010a4da0`:

- ZIP: `b78435ae05c85b7db3b75e1964b011ab1507270178f12f9790acecefc705d004`
- Descriptor: `0a55a355271957671f5e208c8cc57be463384a2c347d679b88696e6c5ebb09b2`
- Helper: `f15a6603c0f191b34cce451ce7777e44098b0920fa5bd759bdcb1b9255ffa90b`
- XPI: `147aac3ceac6b0d79760f07367a995def6945443f8a3d29d844461b134b1ef0b`

The native matrix/2-GiB CI evidence is Windows Server x64 evidence; the dependent installation job is Windows 11 ARM64 x64-emulation evidence. Neither is silently relabeled as native Windows 11 or a development-tools-free image.

A new local actual-Firefox run passed on Windows 11 `10.0.26200`, Developer Edition `156.0` / `aurora`, Python `3.14.3`. The browser executable digest was `326d50c52b5cbef1fa7c946c1a06e549f23f4a4a92649eaa4411d5cec06cf7b4`. This checkpoint used a **dirty driver worktree** at `0ccc6ea`; its report records driver digest `ccf3f1472b8370b88fc3c5caeb7e7ef5a9a2cd54a48b336f92897cee17055673` and fixture digest `7064a04d0412900b6242117e0816f9f8395a02447eddfcfa32c2534d2f66d479`. Subsequent hardening requires a clean-revision rerun. No qualified release/tag exists.

## What the slice actually exercised

1. Reviewed packaged setup installed a new owned generation; copied helper/XPI hashes, registration scope and paired native authority were checked.
2. Firefox loaded that generation's **temporary packaged XPI**, opened its actual manager page and connected through real Native Messaging. No mocked browser/native APIs were substituted.
3. Actual settings UI persisted an owned destination, two default workers and zero retries in isolated state. Creation explicitly selected four workers for these fixtures.
4. SHA-256 completion produced independently verified exact 8-MiB output; a wrong digest displayed `CHECKSUM_MISMATCH` without publishing its final file.
5. Actual Pause/Resume controls operated while a worker-body gate held completion, then produced exact output. This is not proof of nonempty durable partial reuse.
6. A genuine optional-permission prompt was observed on the owned manager tab and accepted for the synthetic loopback site. HttpOnly cookie collection, explicit same-origin referrer and exact signed-query spelling/order reached the helper's fixture; output matched and secret inputs reset.
7. The actual Firefox permissions API showed precisely `cookies` plus `http://127.0.0.1/*`, then no cookie/HTTP(S) authority after the UI Revoke action. These Firefox grants cover all ports; helper context still confines one exact origin.
8. An authenticated task was paused, the owned Firefox/helper closed, then the same **owned** profile/state restarted and the temporary XPI explicitly reloaded. Resume returned a failed `AUTH_REQUIRED` task with no final output. This checks session-loss refusal, not fresh authenticated retry or every restart state.
9. Same-artifact, same-version upgrade/cleanup/uninstall preserved completed bytes and removed the owned registration. All Firefox/helper processes were absent afterward, as were the fixed host key in HKCU/HKLM 32/64 views.

The runtime reported private browsing disallowed. That is **not** an actual private-window enforcement test. Signing preference readback remained `value: true, user: false` across the run/restart; no signing override was written.

## Ownership and environment contract

`scripts/qualification/firefox.py` uses the installed unpatched Developer Edition's Marionette protocol. The local Playwright CLI targets Edge and cannot substitute for this Firefox/native/permission evidence. Firefox's temporary-addon API requires owned-profile system-context automation; that authority is not granted to the extension or a live profile.

- Count-only closed-process and all-view absent-registration checks precede installation; closed-process checks repeat before each setup mutation.
- Only the reviewed setup CLI mutates registration. No manual registry deletion, retired development installer or prototype fallback is used.
- Profile, local/roaming app data and downloads are exclusively owned test paths. Native children use a System32-only PATH. Fresh application state does not imply a factory-clean OS or absence of installed development tools.
- Default signing, TLS, Safe Browsing, updates, proxy and sandbox preferences are not overridden. Marionette's broad recommended-preference bundle is disabled in the owned profile; only explicit test startup/automation/privacy preferences are written.
- An endpoint must report the exact owned profile before navigation/system-context commands. Framing is bounded/correlated. Shutdown is through the verified owned session, never PID/tree killing. An unresolved process, changed registration or interrupted setup preserves its domain and a private recovery ticket, not a successful report.
- Reports are create-new beneath `artifacts`, identify before/after inputs and are emitted only after cleanup. Raw profiles, screenshots and recovery tickets are not public inputs. Same-account races are not converted into a hostile-account isolation claim.

## Failed attempts and resulting contract corrections

The first pause case timed out; a single diagnostic reproduction found valid/enabled submission, the expected row in Failed, and only one classified probe plus one classified body request. Source inspection established that `ProbeClient` verifies **both first and last bytes** before downloading. The Python fixture recognized only byte zero, so its new worker-body gate held the last-byte verification. The gate selector, not product behavior or deadline, was corrected; a real-HTTP regression proves both boundary probes bypass worker faults/gates.

This also sharpens prior native evidence: the earlier bad-range/change/truncate cases demonstrated **probe-stage refusal**, not worker-stage refusal. The corrected fixture now lets both probes succeed and injects those faults into worker responses; the actual native smoke passed again. Old reports are retained with their narrower meaning.

The next browser attempt reached all then-implemented UI/installation checks but failed fixture shutdown. The original exception was not classified, so its precise cause is unavailable. A deterministic real TCP-reset reproduction established an overstrict fixture boundary: a peer reset before headers produced `live=0` and one handler failure. Typed peer disconnects/timeouts are now expected across handler entry as well as body writing; unexpected exceptions and unjoined threads still refuse success. The regression also injects an unexpected exception and verifies refusal/listener closure. Subsequent complete slices, including permission readback and session-loss restart, passed. No blanket exception suppression, test disabling or product assertion relaxation was used.

Seven harness policy/real-HTTP/transport tests passed at that checkpoint. The later eighteen-test report/fixture/ownership boundary and expanded native-artifact matrix are recorded in [NATIVE_QUALIFICATION.md](NATIVE_QUALIFICATION.md). The body handler still had a broad `OSError` catch and treated gate exhaustion as an expected timeout at the earlier checkpoint; later real-HTTP regressions exposed and corrected those distinctions without assigning a cause to the old unclassified Firefox failure. Final-tip CI and clean-driver evidence remain required.

## Remaining gates

The current twenty-case expansion resolves the listed browser boundaries for its identified draft driver/current candidate, not every possible restart, browser version or deployment environment. Require the committed clean driver, current-tip CI, mapped critical-adversary coverage, accepted support/resource limitations, final exact artifact/source/tag linkage and full public-input/log/artifact/cache review. Clean-machine evidence remains unavailable, not passed or required for this personal release. See [QUALIFICATION_PLAN.md](QUALIFICATION_PLAN.md). Do not merge #28 or publish solely on the strength of an intermediate candidate's browser result.
