# Actual Firefox artifact slice — #28 (not release approval)

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

Seven harness policy/real-HTTP/transport tests currently pass. Browser-facing code and new fixture changes need final-tip CI and clean-driver evidence before any acceptance decision.

## Remaining gates

Toolbar/link-menu creation, Cancel/Remove and relevant validation controls, CSP/private-window enforcement, the complete safe-restart/fresh-session matrix, all critical adversaries, clean-machine/support criteria, accepted resource interpretation, exact artifact freeze/tag linkage, first-party licensing/signing handling and final public-input/log/artifact/cache review remain separate. See [QUALIFICATION_PLAN.md](QUALIFICATION_PLAN.md). Do not merge #28 or publish a qualified release on the strength of this slice alone.
