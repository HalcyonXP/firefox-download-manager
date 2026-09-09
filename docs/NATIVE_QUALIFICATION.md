# Native artifact qualification and harness boundaries — #28

## Later clean/current-candidate evidence (2026-09-09)

CI `34341064343` at `37c5ed2` passed all three jobs, including the hardened installer driver. Current licensed candidate source is `3e8ae17644da1f75f9612bb49db9bef9424eeacd`, descriptor `f77548a3b61d9382ed934d74e8360ea991ff4e6b95def07aeb9f81f05e59dcc6`, ZIP `e274bcc5d7b7ba98f804ce5da858e6fb5f64d64a224e7c75d7ddba2e13c88b57`. Clean37 native26/default2GiB passed against those exact downloaded bytes on native Windows11 x64, separately from a clean37 local-source build.

That exact-CI observation measured 2.522 s elapsed, 2.297 s helper CPU, 42,389,504-byte peak working set, 39,882,752-byte sampled private peak, 25 samples at 100 ms, 2,147,483,648-byte allocation and nine events / 5,175 bytes. Aggregate read/write I/O was 2,147,484,085 / 2,147,498,287 bytes, not disk-only traffic. Scope and unavailable clean-OS coverage remain unchanged.

Owner-confirmed browser closure subsequently allowed clean37's nine-check licensed Firefox rerun. New draft-driver browser expansion passes twenty actual cases, native26/2GiB and the actual installer lifecycle locally; shared retained-file/generation checks and a default-open large-body fixture gate do not change product behavior. See [FIREFOX_QUALIFICATION.md](FIREFOX_QUALIFICATION.md) for twenty-six policy tests, failures/recovery and final clean-tip gates. The older twelve-process/next-CI statements below describe earlier checkpoints, not the latest observation.

## Objective and earlier integrated evidence

Deliver the owner-authorized, versioned/checksummed MIT personal release on the existing native Windows 11 x64 / Firefox Developer Edition machine. No further in-scope approval is needed; live-profile/process, security and publication boundaries remain. This document records **native-only** evidence, not completion of Firefox or release qualification.

#44 / PR #45 merged as `6482a17892fb2e532077b08ce451a1bf0929de62`. Final PR CI `34331518837`, merged-main CI `34333682602`, and integrated #28 baseline CI `34334123810` at `f90f04cbef2a14dd7f83a54bd321f28e8c9c93cd` passed all three jobs. The original failures and accepted test-contract corrections remain in [PROGRESS_DEADLINES.md](PROGRESS_DEADLINES.md); a passing rerun is not a reconstruction of missing traces.

The integrated CI candidate includes MIT/eight package payloads/nine XPI leaves. Its source is synthetic PR merge `453afad7dcd8684ecae431c2ad357bf22581607d`, descriptor SHA-256 `b2baaa49606757f1242497deaeb4ec4075cf0f994631454d78b16ecff3962698`. This is distinct from the pre-license candidates and the earlier local `26e302b` artifact. Local setup `verify` passed without registration/profile mutation.

A **dirty development driver** at `f90f04c` passed all 26 current native cases and the default 2 GiB run against those exact downloaded bytes. The report binds driver file hashes, source and actual `native_x64` helper execution. This is not the final source/tag or clean-driver checkpoint. Later driver/artifact changes require identified reruns.

## Native case coverage

| Boundary | Actual packaged-helper evidence |
| --- | --- |
| Scheduling / output | 1/2/4/8 selected workers; independent exact bytes and SHA-256 |
| Single streams | Ignored probe ranges, empty file, unknown length, weak ETag and missing validator; exact output/checksum |
| Worker faults | Malformed/missing/out-of-bounds `Content-Range`, changed ETag, truncated body, ignored worker range and a corrupted body byte; failure and no final publication |
| Integrity / ownership | Wrong expected digest; collision suffix with existing file preserved; removal of completed task preserves published output |
| Controls / restart | Active pause/resume, cancel/keep, actual owned-helper kill/restart/explicit resume |
| Durable retained coverage | Joined Pause, actual v4 metadata covering exactly `[0, 2 MiB)`, independent disk-prefix hash, resumed exact output without refetching that prefix |
| Crash with durable prefix | Resume into held workers, kill only the retained helper handle, restart with preserved coverage, explicit Resume, exact output and no prefix refetch |
| Known identity change | Change the retained resource's ETag, then refuse Resume before new worker requests; no final file |
| Deletion | Cancel/delete a proven durable partial, remove the task record, and publish no output |

The older twelve-case reports remain narrower. Aggregate received bytes during Pause/kill do not prove durable retained ranges. Coverage here also does not exhaust the Rust adversarial/retry/session suites or the remaining actual-Firefox matrix.

The selective worker-body gate permits boundary probes and the prefix, then holds all four active workers beyond the prefix. Readiness is based on gate waiters and published prefix bytes, not a sleep or total request count. All initial active requests have been observed before Pause. Cancelled server-side waiters persist until release: they are not local workers and cancellation does not retract transmitted requests. Persisted ranges may be adjacent entries; an independent half-open coverage check rejects gaps, overlaps, missing tails and out-of-bounds bytes without requiring one coalesced entry.

## Resource observation (draft driver, not a product ceiling)

The new 2 GiB run measured 2.625 s transfer-plus-validation elapsed, 2.391 s helper CPU, 43,786,240-byte peak working set, 39,686,144-byte peak sampled private usage, 26 samples at 100 ms, 2,147,483,648-byte reported allocation and nine events / 5,175 event bytes. Independent final SHA-256 was `382045c648d7c2a42a01bb3132186a0397d40e2e4e85a99377dbc79c20e6671e`.

Owned-process measurements exclude OS cache/kernel/other-process memory. Aggregate I/O includes more than disk traffic; loopback timing is neither physical-device nor Internet/VPN performance. Development tools remain installed. No clean-OS or ARM-emulation evidence is relabeled as native-x64 coverage.

## Shared evidence-sink and lifecycle policy

`scripts/qualification/support.py` serves both native and Firefox drivers:

- Reject report spelling before canonicalization: streams/ADS, devices, traversal, trailing-dot/space ambiguity, non-ASCII names and aliases/junctions are not report sinks. This deliberately narrow **test-report** policy is not the product's download-filename policy.
- Recheck before publishing, lease ordinary ancestors with Windows `GENERIC_READ` and no delete sharing, write/fsync a bounded exclusive partial, then use Windows no-replace rename. Failed writes/renames preserve the partial and existing destination; they are not successful reports.
- Bound JSON metadata/report size to 64 KiB and reject duplicate members, invalid UTF-8, nonfinite numbers and excessive nesting without echoing contents. Include `support.py` in evidence identity.
- Refuse qualification under optimized Python, which would disable assertions.

Native ownership is recorded before child launch. Reader-start/handshake failures kill/join only that retained child and join started readers; resource-API initialization is inside the same cleanup boundary. Domain deletion requires every recorded native owner to be closed. Unresolved fixture/child/domain cleanup preserves the domain with a create-new private `.git/native28-recovery-*.private.json` ticket and no success report. These are ordinary ownership/race protections, not hostile same-account isolation or a filesystem-latency guarantee.

## Failed attempts and demonstrated corrections

1. **ADS preflight:** an ADS-shaped `.json` name passed the old checks and reached an intentionally nonexistent setup launch. No process or stream was created. The spelling guard now rejects it before launch, including before Firefox's closed-app check.
2. **Metadata-only directory handle:** the initial access-0 lease did not prevent an actual Windows directory rename. `GENERIC_READ`, matching the setup lease, does participate in sharing checks. The real rename test and an access-0 mutation demonstrate the difference; the test restores its retained path even on assertion failure. The original failed run left a small ignored test domain; it was not swept as unknown data or treated as release input.
3. **Worker status classification:** the first new worker-ignored case expected `RANGE_RESPONSE_INVALID`; the actual helper returned `HTTP_STATUS`. Source inspection confirmed the scheduler rejects non-206 status before range-header validation. The driver now requires that exact established error rather than broadening it or changing the product.
4. **Adaptive request size:** the first retained fixture wrongly expected four 2 MiB requests, borrowing the one-worker test's shape. Four workers on 8 MiB use 1 MiB requests; the received-prefix observation outlasted that incorrect count predicate and gate deadlines refused success. Gate-waiter readiness and exact coverage replace the request-count premise. No deadline was extended.
5. **Canonical local path spelling:** Rust persisted an extended local-drive path while Python used its ordinary canonical spelling. The reader now strips only that prefix for the exact owned drive, then retains parent/ordinary-file/alias checks. It does not accept UNC/device namespaces or arbitrary canonical equivalence.
6. **Fixture failure classification:** review found an older broad body `OSError` catch and a gate deadline represented as an expected socket timeout. Only typed peer disconnects/socket timeouts remain expected. Gate exhaustion and unrelated I/O exceptions invalidate success; real HTTP failure tests preserve this distinction. Earlier unclassified Firefox cleanup history is not retroactively assigned a cause.

Eighteen policy/real-HTTP/transport/owned-synthetic-child tests passed. Access-0 leases, skipped constructor cleanup and missing final-prefix coverage mutations failed their intended assertions; exact source was restored and the suite passed again. Policy mocks and the synthetic Python child are not compiled-helper or Firefox E2E evidence. The 26-case artifact run is separate.

## Remaining work

Commit and rerun the identified harness; require final-tip CI and final exact-artifact checks. The latest count-only check found twelve unowned Firefox processes, so no browser/registration mutation was attempted. Actual toolbar/link-menu, Cancel/Remove/validation controls, CSP/private-window enforcement, broader recovery/fresh-session paths, installation lifecycles, final artifact/tag linkage and publication review remain in [QUALIFICATION_PLAN.md](QUALIFICATION_PLAN.md). MIT and the existing-machine plan are settled, not current blockers.

## Clean native rerun and installer-harness follow-up

Committed clean driver `520f02992ba46dfd1511cb374d10e7dbd8035330` subsequently passed the same 26 cases/2 GiB against descriptor `b2baaa49606757f1242497deaeb4ec4075cf0f994631454d78b16ecff3962698`. Its separate resource observation was 2.625 s elapsed, 2.250 s CPU, 43,507,712-byte peak working set and 38,854,656-byte sampled private peak (26 samples). This supersedes the pending clean-rerun status for those bytes, not final artifact/Firefox approval.

Review then found that the older installation test driver could overwrite a report, wrote it before final cleanup, and could adopt an unrecorded registration for deletion from its location/addon ID alone. This is a **qualification-driver** boundary, not a demonstrated production-setup defect. Previous successful CI lifecycles remain evidence of their completed runs, not proof that the old report writer rejected every failed cleanup.

The installation driver now uses the same spelling/bounds/lease/no-replace reporter **after** cleanup, refuses optimized Python, checks all four registration views before fresh test authority, repeats closed-app checks, and binds unchanged artifact/driver identity. It verifies exact UUID generation/manifest/helper/XPI bytes before recording a normal installed binding. Unrecorded/changed registration or unresolved cleanup preserves the domain with a private `install28-recovery` ticket; it is never adopted merely because it looks like a test path. Existing synthetic malformed/foreign key fixtures remain deliberately test-only, under closed/absent preconditions—not an installation mechanism, browser harness or retired-script fallback.

Twenty policy tests passed locally, including preflight ordering, no deletion for an unrecorded binding, optimized-Python refusal and synthetic generation/payload/namespace checks. These tests do not mutate registration or stand in for the next actual hosted/local lifecycle. The updated driver still requires its own CI and exact-package checks before release.
