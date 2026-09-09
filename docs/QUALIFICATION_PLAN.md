# Final-artifact qualification — #28

## Objective and established baseline

The user-authorized outcome is a qualified, versioned, checksummed release ready for the personal Windows 11 / Firefox Developer Edition workflow. This is not permission to modify the live Firefox profile, weaken system protections or bypass a failed gate. No qualified release exists yet.

Dependencies #25/#26/#27 are merged. Two baseline interruptions were subsequently resolved independently: #39 / PR #40 replaced an invalid progress-consumer count premise; #41 / PR #42 separated retry cancellation readiness from durable acknowledgement. Their failure/evidence history is in [PROGRESS_REGRESSION.md](PROGRESS_REGRESSION.md) and [RETRY_CANCELLATION_REGRESSION.md](RETRY_CANCELLATION_REGRESSION.md). Both fixes are merged; main `01a49d0e76d699aa64c1a6e100de92da1f1a413a` passed all three CI jobs in `34289550859`. #28 has resumed, not been approved.

The current main candidate (not the earlier #27 PR artifact) has ZIP SHA-256 `fa579637936638bc5f4e8aca6398ad63dc1ca6f1ee8b85db7235c5b55f483c95`, descriptor `c198f4b8b22e0982c819f4160fc6076ccbae0a38f9a006f3625bcef5698e15ce`, and helper `f15a6603c0f191b34cce451ce7777e44098b0920fa5bd759bdcb1b9255ffa90b`. Earlier candidate identity and packaging evidence remain historical, not silently relabeled as the intended release.

Actual setup lifecycle passed on Windows Server 2025 x64 and a fresh hosted Windows 11 ARM64 machine running the x64 artifact under emulation. The same candidate passed an isolated native Windows 11 x64 helper probe locally, without registration or Firefox. Same-environment repeated clean-target builds matched; cross-environment binaries differed. Packaging evidence, recovery limits and the reviewed LLVM/MinGW/UCRT recipe remain in [PACKAGING_PLAN.md](PACKAGING_PLAN.md).

## Owner clarification and revised environment gate (2026-09-09)

The owner confirmed permissive FOSS intent and that this is their only computer. We selected MIT for first-party code and revised the first release to qualify **the existing native Windows 11 x64 / Firefox Developer Edition setup**, not require a separate clean OS. [ADR 0011](decisions/0011-license-and-available-qualification.md) separates confirmed intent from these implementation decisions. All safety/correctness requirements remain; the original clean-machine criterion is superseded, not checked off.

Use owned fresh profiles, application data and installation generations, plus System32-only child PATH and the reviewed Windows DLL/import policy. Do not uninstall development tools, alter security/OS settings, create accounts, or touch the live profile. Report the existing-machine context and unavailable clean-machine test in release notes. This does not narrow support to ARM emulation or imply a factory-clean/developer-tools-free OS. License-bearing packages/XPI change the artifact bytes and require new checksums and qualification.

## Test boundaries and environment evidence

- **Native artifact tests:** drive the exact packaged helper over real stdio with fresh self-owned application state and deterministic loopback HTTP. No Firefox API mocks, registration or browser-profile access. This can run safely alongside an unowned browser because it does not share the installation/state/registration domain.
- **Firefox E2E:** actual final XPI, actual native registration and packaged helper; explicit form/permission/control behavior, snapshots and file bytes. The local Playwright CLI drives Edge and cannot qualify this Firefox boundary. Reuse the owned-profile Marionette approach, not a mocked browser transport, in an owned, disposable application/profile domain on the existing machine.
- **Unavailable clean-machine evidence (not required under the revised personal-release plan):** the hosted Windows 11 ARM64 runner is a fresh VM/current-user state, not a factory image without developer tools. Its x64-emulation results must stay distinct from native Windows 11 x64 coverage. A local count-only preflight earlier on 2026-09-09 found 19 unowned Firefox processes. At `2026-09-09T09:35:02+10:00`, a fresh count-only preflight found zero; none was stopped or its profile inspected by this work. This opens an owned-profile local testing avenue, but must be rechecked before each registration mutation. Windows Sandbox remains unavailable; no feature was enabled.
- **Release support matrix:** do not silently reinterpret the user's personal native-x64 outcome as ARM-only support or describe limited/emulated evidence as complete native coverage. Record exact OS/browser/architecture/artifact identities; unresolved required coverage blocks release approval.

## Planned cases (not completed evidence)

1. All 1/2/4/8-worker outputs match independent fixture bytes and SHA-256. Include ignored-range single-stream fallback and empty/unknown-length structural cases.
2. Malformed/overlapping/out-of-bounds ranges, changed identity, truncation, rejected ranges, retry/cooldown and checksum mismatch never publish corrupt output. Map existing adversarial Rust coverage to exact-artifact cases; do not claim every case merely from a handful of examples.
3. Pause/resume/cancel, joined acknowledgement and stable local bytes; forced termination only of a retained owned helper process handle; restart requires safe explicit recovery and preserves the checksum/resource identity. Late remote observations are not confused with local worker ownership.
4. Firefox explicit creation, monitoring, permission consent/revoke, checksum success/failure/cancellation, settings, helper and Firefox restart, session-loss refusal and fresh Add. Test actual packaged CSP/private-window restrictions and no broad permission escalation. No signing-preference changes or live-profile installation.
5. At least 2 GiB fixture with bounded server/helper state: independent final digest, exact size, observed helper working-set/CPU, logical/allocated output bytes, process aggregate I/O, event counts/rates and elapsed time. Windows process I/O includes networking/stdio and is **not disk-only traffic**; logical output throughput is not physical device throughput. Loopback results are not Internet/VPN performance claims.
6. Installation/upgrade/removal and state preservation tied to the exact qualified artifact; repeat final publication-input review including separate log/artifact/cache contents. Include the MIT first-party license and required third-party notices. Existing temporary unsigned-XPI workflow is explicit, not permanent signed installation or publisher authentication.
7. Publish only after required evidence passes and remaining support gaps are resolved. The release tag/source, tested artifact checksum, harness revision and release notes must identify the same inputs; do not assume a later rebuild is byte-identical.

## Vocabulary

- **Artifact boundary**: the checksummed executable/XPI actually used by a test, not merely an equivalent source checkout or Cargo test binary.
- **Native-only versus Firefox E2E**: real helper process/network/disk versus the additional real Firefox/permission/Native Messaging path. Neither label subsumes the other.
- **Working set / CPU time / aggregate I/O**: owned-process OS measurements. Report sampling cadence and scope; do not relabel them as whole-system or physical-disk measurements.
- **Qualification gap**: an unsatisfied acceptance boundary, not a test success with a footnote that silently removes the requirement.

## Native harness checkpoints (not release approval)

The first native-only smoke failed because the driver omitted wire-v2's required `partial_policy`; explicit `keep` corrected the driver, not product validation. After baseline recovery, all twelve implemented main-artifact cases passed: worker counts, single fallback, malformed range/changed validator/truncation refusal, checksum mismatch, pause/resume, cancellation, and actual owned-helper kill/restart with explicit resume and exact output. This does not yet cover every planned adversary, retained-range reuse after a durable checkpoint, or the Firefox UI.

A subsequent **draft-driver** run transferred and validated 2 GiB with four workers on native Windows 11 x64: exact digest `382045c648d7c2a42a01bb3132186a0397d40e2e4e85a99377dbc79c20e6671e`, 2.521 s elapsed, 2.328 s helper CPU, 42,430,464-byte peak helper working set, 38,043,648-byte peak sampled private usage (100-ms sampling, 25 samples), 2-GiB reported file allocation, nine events / 5,175 event bytes. These are observed loopback/process values, not physical-device throughput, total system RAM (OS cache is excluded), Internet performance or a product resource ceiling. The artifact was clean main; the driver worktree was dirty and its exact file hashes were recorded (`native.py` `ba64e52a49616b9ee402567450956067ee7f0941dbf34fa683f9b244df53c618`, fixture `c45763bea0998a3cd1a498583916cc56edd3ffe2ab2380780a3407ab034027b7`). Later driver changes require a fresh source-identified run.

Current hardening confines reports beneath `artifacts`, refuses existing reports before process launch, verifies unchanged artifact/driver identities, records actual owned-helper execution architecture with `IsWow64Process2`, and writes a report only after cleanup. The fixture caps owned handler records at 32, applies a socket timeout before header parsing and closes/joins accepted sockets/threads. Seven policy/real-HTTP/transport tests now pass, including an incomplete-header shutdown case; that case takes roughly the ten-second socket timeout here, not a claimed instantaneous interrupt. A new hardened smoke also passed. CI is configured to run those tests and the actual candidate's native matrix/2-GiB fixture; foundation CI `34292916341` passed all three jobs; newer fixture/browser changes still need final-tip CI.

## Firefox harness safety decision

The historical authentication prototype is **not** a permitted final-artifact harness as-is: it points to a debug helper/unpackaged extension, registers manually, changes the signing preference and has a PID/tree-kill fallback. None of those paths was executed for #28. Replace them with the reviewed setup CLI and packaged XPI, default signing protection, bounded owned-profile Marionette transport, exact registration ownership and conservative cleanup. The local Playwright CLI is useful for Edge rendering but cannot replace actual Firefox Developer Edition Native Messaging/permission tests. A new actual packaged-XPI manager-page slice now passed, including permission readback and session-loss restart refusal. Its driver was dirty; it is not full qualification. [FIREFOX_QUALIFICATION.md](FIREFOX_QUALIFICATION.md) records exact identity, safety boundaries, failed attempts, successes and remaining gates.

## Updated native evidence interpretation

A clean-driver rerun at `0ccc6ea` passed the main `01a49d0` artifact matrix and 2-GiB path: 2.522 s elapsed, 2.297 s helper CPU, 43,536,384-byte peak helper working set, 39,346,176-byte peak sampled private usage, 25 samples at 100 ms and nine events / 5,175 event bytes. This is a clean **source revision**, not a clean OS or an accepted resource ceiling.

Browser harness work subsequently exposed that the fixture recognized only the first-byte probe, not the last-byte verification. The prior malformed/change/truncate cases therefore proved probe-stage refusal. The corrected fixture now permits both probes and injects worker-stage faults; a new actual candidate native smoke passed. Do not upgrade old report coverage retroactively. The new two-boundary-probe and real TCP-reset regressions, plus the scoped Firefox run, are described in [FIREFOX_QUALIFICATION.md](FIREFOX_QUALIFICATION.md).

## Latest pre-license checkpoint

Before this licensing/environment change, clean harness `5ce837c` and CI `34298068858` passed all three jobs. The same exact candidate (synthetic source `534ad557f75bae62e03158cbee44a1a946511f90`, descriptor `128e436c9f111a9db3bd7bd73f2f3ae13ab5b4ded4afc4e592954b74d9420539`) passed the native matrix/2-GiB path and nine real-Firefox slice checks locally. This supersedes earlier pending/dirty-driver checkpoint wording, not the remaining functional gates. [PR #43's evidence comment](https://github.com/HalcyonXP/firefox-download-manager/pull/43#issuecomment-5594476156) records identity, results and the narrow audit scope. That candidate predates bundled first-party licensing and is not the new release input.
