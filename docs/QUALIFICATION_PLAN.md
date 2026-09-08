# Final-artifact qualification — #28

## Objective and established baseline

The user-authorized outcome is a qualified, versioned, checksummed release ready for the personal Windows 11 / Firefox Developer Edition workflow. This is not permission to modify the live Firefox profile, weaken system protections or bypass a failed gate. No qualified release exists yet.

Dependencies #25/#26/#27 are merged. Packaging PR #38, final feature `47ca40b`, passed public CI `34276355376`; main is `9928058`. Merged-main CI `34277988932` failed at the release-target progress test (`progress_events_are_rate_limited_while_snapshots_remain_complete`, minimum four-event assertion); #28 qualification is paused while that baseline is resolved. This is a real failed test, not artifact-finalization infrastructure. Candidate ZIP SHA-256 from the final PR run: `59e655820e4a14b31349d3e0b89123eece1f0ce6b99154913131d09d39602075`. That identifies a candidate, not release approval.

Actual setup lifecycle passed on Windows Server 2025 x64 and a fresh hosted Windows 11 ARM64 machine running the x64 artifact under emulation. The same candidate passed an isolated native Windows 11 x64 helper probe locally, without registration or Firefox. Same-environment repeated clean-target builds matched; cross-environment binaries differed. Packaging evidence, recovery limits and the reviewed LLVM/MinGW/UCRT recipe remain in [PACKAGING_PLAN.md](PACKAGING_PLAN.md).

## Test boundaries and unresolved environment coverage

- **Native artifact tests:** drive the exact packaged helper over real stdio with fresh self-owned application state and deterministic loopback HTTP. No Firefox API mocks, registration or browser-profile access. This can run safely alongside an unowned browser because it does not share the installation/state/registration domain.
- **Firefox E2E:** actual final XPI, actual native registration and packaged helper; explicit form/permission/control behavior, snapshots and file bytes. The local Playwright CLI drives Edge and cannot qualify this Firefox boundary. Reuse the owned-profile Marionette approach, not a mocked browser transport, in a disposable environment.
- **Clean environment:** the hosted Windows 11 ARM64 runner is a fresh VM/current-user state, not a factory image without developer tools. Its x64-emulation results must stay distinct from native Windows 11 x64 coverage. A local count-only preflight on 2026-09-09 found 19 unowned Firefox processes; none was stopped or its profile inspected. Native local browser qualification remains unavailable while that precondition fails.
- **Release support matrix:** do not silently reinterpret the user's personal native-x64 outcome as ARM-only support or describe limited/emulated evidence as complete native coverage. Record exact OS/browser/architecture/artifact identities; unresolved required coverage blocks release approval.

## Planned cases (not completed evidence)

1. All 1/2/4/8-worker outputs match independent fixture bytes and SHA-256. Include ignored-range single-stream fallback and empty/unknown-length structural cases.
2. Malformed/overlapping/out-of-bounds ranges, changed identity, truncation, rejected ranges, retry/cooldown and checksum mismatch never publish corrupt output. Map existing adversarial Rust coverage to exact-artifact cases; do not claim every case merely from a handful of examples.
3. Pause/resume/cancel, joined acknowledgement and stable local bytes; forced termination only of a retained owned helper process handle; restart requires safe explicit recovery and preserves the checksum/resource identity. Late remote observations are not confused with local worker ownership.
4. Firefox explicit creation, monitoring, permission consent/revoke, checksum success/failure/cancellation, settings, helper and Firefox restart, session-loss refusal and fresh Add. Test actual packaged CSP/private-window restrictions and no broad permission escalation. No signing-preference changes or live-profile installation.
5. At least 2 GiB fixture with bounded server/helper state: independent final digest, exact size, observed helper working-set/CPU, logical/allocated output bytes, process aggregate I/O, event counts/rates and elapsed time. Windows process I/O includes networking/stdio and is **not disk-only traffic**; logical output throughput is not physical device throughput. Loopback results are not Internet/VPN performance claims.
6. Installation/upgrade/removal and state preservation tied to the exact qualified artifact; repeat final publication-input review including separate log/artifact/cache contents. Review signing/first-party licensing prerequisites deliberately. Existing temporary unsigned-XPI workflow is explicit, not permanent signed installation.
7. Publish only after required evidence passes and remaining support gaps are resolved. The release tag/source, tested artifact checksum, harness revision and release notes must identify the same inputs; do not assume a later rebuild is byte-identical.

## Vocabulary

- **Artifact boundary**: the checksummed executable/XPI actually used by a test, not merely an equivalent source checkout or Cargo test binary.
- **Native-only versus Firefox E2E**: real helper process/network/disk versus the additional real Firefox/permission/Native Messaging path. Neither label subsumes the other.
- **Working set / CPU time / aggregate I/O**: owned-process OS measurements. Report sampling cadence and scope; do not relabel them as whole-system or physical-disk measurements.
- **Qualification gap**: an unsatisfied acceptance boundary, not a test success with a footnote that silently removes the requirement.

## Draft harness checkpoint (not qualification)

`qualification/native.py` and `fixture.py` are first-party, standard-library-only drafts. The initial native-only smoke reached cancellation after worker/fallback/adversarial/checksum/pause cases, then failed because the harness omitted wire-v2's required `partial_policy`. The input was corrected to explicit `keep`; product validation was not weakened. No complete matrix, restart or multi-gigabyte measurement is claimed at this checkpoint, and the corrected draft has not yet been rerun. The merged-main failure above is a separate blocker requiring its own focused issue before qualification continues.
