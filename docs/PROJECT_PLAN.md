# Project Plan

GitHub execution board: [Firefox Download Manager](https://github.com/users/HalcyonXP/projects/1)

Current state is maintained on the project board. Planning is complete; implementation proceeds through the lowest-numbered issue in **Ready** status, with dependent work retained in **Backlog** until it is unblocked.

Current protocol: v2 (paired helper/extension upgrade, #20); v1 remains archived.

Project vocabulary and autonomous handoff decisions: [GLOSSARY.md](GLOSSARY.md).

## Current owner workflow correction (2026-09-10)

The owner reports that an ordinary GGUF click used Firefox's built-in downloader after installing the XPI/restarting. They explicitly require **setup.exe → visible tray companion → install XPI once → restart Firefox → ordinary download click automatically starts in Manager**, and short instructions. v0.1.0's manual capture/no-tray/temporary-XPI qualification did not prove that experience. Do not present that older workflow as the requested solution or diagnose an absent extension without evidence.

[ADR0013](decisions/0013-install-restart-click.md) accepts this next-release direction, superseding manual-only capture/temporary user installation and the browser-owned engine lifetime for M5, while preserving v0.1.0's immutable source/assets and historical evidence. [USER_WORKFLOW.md](USER_WORKFLOW.md) is the short target acceptance script, not instructions that make v0.1.0 behave differently. Independent companion lifetime is our implementation choice; automatic Windows-logon startup is not requested or authorized.

The owner approved the user-facing infographic and said Proceed. #50 now develops the visible companion independently of #49: [COMPANION_DESIGN.md](COMPANION_DESIGN.md) records the engine-owner split, native preview, selected/rejected dependency approaches and remaining IPC/setup gates. Preview code/evidence is not an installed companion or a newly qualified release. The original GGUF URL is now known in #49; do not infer it remains unknown from the earlier direction snapshot.

Persistent signing authority/approval and safe browser handoff are unresolved implementation gates. No weaker signing setting, temporary-addon reload, manual Add or mocked transport may substitute for the required restart/click test. The owner now explicitly authorizes autonomous completion without further routine approvals; see [ADR0014](decisions/0014-autonomous-completion.md). Owned isolated tests remain subject to operational ownership/closed-app preflights, never inspection of their normal profile. Credentials/account access are external prerequisites, not presumed available. Existing networking/storage/credential/VPN/privacy boundaries remain.

## Public, authoritative repository (2026-09-08)

The user explicitly authorized public visibility and requested that all documentation refer to **[HalcyonXP/firefox-download-manager](https://github.com/HalcyonXP/firefox-download-manager)**. #8 supersedes the temporary two-repository arrangement. This public repository now owns code, work issues, milestones, CI, and future releases. `origin` points here; there is no publication-remote synchronization step.

Twenty-two regular issues were transferred with their states and comment history. Current documentation uses their new numbers; [ISSUE_MIGRATION.md](ISSUE_MIGRATION.md) records the mapping and links historical implementations to cleaned public commits. The owner-private planning board retains the corresponding work statuses. The predecessor stays private solely as an archive, not an alternative source of truth.

Implementation includes #23 authenticated handoff, #24 shared polite request admission, and #25 integrity/SHA-256. #32/#33 resolved the cancellation-observation baseline and actual public CI passed; that temporary gate is no longer active. The #26 security review is recorded; #27 remediated its packaging blockers within the documented ownership/fault model and merged. #39/#41 baseline corrections also merged and main CI passed. Candidate qualification #28 / PR43 is accepted, merged and Done. #46 / PR47 records final-main qualification, the published/verified v0.1.0 release and administrative closeout under ADR0012. Privacy work is recorded in #29 and #30. Public visibility is independently verified, but it does not waive hosted-CI, real Firefox, installation, performance, or release-artifact qualification. See [PUBLICATION_PRIVACY.md](PUBLICATION_PRIVACY.md) and [ADR 0009](decisions/0009-public-authority.md).

## Owner clarification: FOSS and the available computer (2026-09-09)

The owner explicitly wants permissive FOSS and has only this computer. MIT is the implementation selected for first-party code; third-party terms stay intact. Qualification will use the existing native Windows 11 x64 / Firefox Developer Edition installation with isolated owned test domains. No new machine/OS/account or protection change is requested. Clean-machine coverage is declared unavailable, not passed. [ADR 0011](decisions/0011-license-and-available-qualification.md) records the scope change, licensing meaning and remaining gates. Earlier checkpoint references to unresolved licensing/required clean-machine coverage are historical and superseded by this decision.

## Candidate acceptance and publication ordering (ADR0012)

The earlier single-issue sequence conflicted: #28 included publication, the guide requires acceptance before merge, and publication requires authoritative merged-main CI. [ADR0012](decisions/0012-qualification-publication-sequence.md) explicitly transfers the final-main/artifact/tag/privacy/publication criteria to dependent #46, without waiving any release or M4 exit gate. #28/PR43 can close only after its mapped candidate criteria and current PR CI pass; #46 becomes Ready only after that merge and successful main CI. A candidate or a Done #28 is not an install-ready release.

[RELEASE_MATRIX.md](RELEASE_MATRIX.md) records the layered critical-adversary coverage and support/resource limits. CI34351352220 and clean8b3 actual native26/2GiB, Firefox20 and native-Windows11-x64 installer runs passed against the exact source50c1c99295b7a0447b102c82b1d61d8100de88ad candidate. Final-main bytes must be identified and requalified by #46. This administrative split implements the owner's existing authorized outcome; the owner did not specifically request or name the split.

## Product goal

Create a trustworthy local download manager for Firefox Developer Edition that can improve throughput when an HTTP(S) server limits individual connections and supports byte-range requests.

The Firefox extension captures user intent and displays state. A Rust native helper owns networking, scheduling, disk writes, validation, and recovery.

## Scope

### Released v0.1.0 scope (historical; next workflow below)

- Windows 11
- Firefox Developer Edition
- Explicit **Download with Manager** activation
- Direct HTTP and HTTPS URLs
- Validated single-stream and segmented downloads
- 1, 2, 4, or 8 workers; default 4
- Queueing, progress, pause, resume, retry, cancel, and recovery
- Configurable local destination
- Cookie-authenticated direct downloads after the unauthenticated MVP
- Local installation and unsigned/development extension workflow

### Non-goals

- VPN detection, configuration, IP rotation, or route management
- Torrent, magnet, FTP, SFTP, or media-extraction support
- Blanket interception of every browser request/download. The historical first release is manual-only; M5 adds safe supported ordinary-click capture, not arbitrary POST/blob/private/auth replay.
- Circumventing account, subscription, or application-level access controls
- Cloud accounts, synchronization, telemetry, analytics, or a remote updater
- Cross-platform packaging in the first release

## Architecture

```text
┌──────────────────────────────────┐
│ Firefox WebExtension             │
│                                  │
│ context menu / creation dialog   │
│ queue and progress dashboard     │
│ settings and local diagnostics   │
│ Native Messaging client          │
└─────────────────┬────────────────┘
                  │ versioned JSON messages
┌─────────────────▼────────────────┐
│ Rust native helper               │
│                                  │
│ protocol boundary                │
│ persistent task state            │
│ lifecycle / retry / progress     │
│ HTTP client and range validator  │
│ segment scheduler                │
│ random-access partial-file I/O   │
│ integrity and recovery           │
└──────────────────────────────────┘
```

The accepted component boundaries and decisions are recorded in [ARCHITECTURE.md](ARCHITECTURE.md) and its linked architecture decision records. The threat model and sensitive-data rules are recorded in [SECURITY.md](SECURITY.md). The versioned extension/helper contract is defined in [PROTOCOL.md](PROTOCOL.md).

## Delivery milestones

### [M0 — Foundation](https://github.com/HalcyonXP/firefox-download-manager/milestone/1)

Establish decisions, project boundaries, protocol design, CI, and deterministic test infrastructure.

- [#9 Record architecture, scope, and security decisions](https://github.com/HalcyonXP/firefox-download-manager/issues/9)
- [#10 Define the versioned extension/native-helper protocol](https://github.com/HalcyonXP/firefox-download-manager/issues/10)
- [#11 Scaffold the WebExtension, Rust workspace, and CI](https://github.com/HalcyonXP/firefox-download-manager/issues/11)
- [#12 Build a deterministic adversarial HTTP test server](https://github.com/HalcyonXP/firefox-download-manager/issues/12)

**Exit condition:** both components build in CI, the protocol boundary is documented, and local HTTP fixtures can reproduce correct and incorrect range behavior.

### [M1 — Native download MVP](https://github.com/HalcyonXP/firefox-download-manager/milestone/2)

Build the download engine before attaching a browser interface.

- [#13 HTTP probing and strict range-response validation](https://github.com/HalcyonXP/firefox-download-manager/issues/13)
- [#14 Safe random-access partial-file storage](https://github.com/HalcyonXP/firefox-download-manager/issues/14)
- [#15 Persistent task and segment state](https://github.com/HalcyonXP/firefox-download-manager/issues/15)
- [#16 Fixed-concurrency segment scheduler](https://github.com/HalcyonXP/firefox-download-manager/issues/16)
- [#17 Pause, resume, cancellation, retries, and progress](https://github.com/HalcyonXP/firefox-download-manager/issues/17)

**Exit condition:** the native helper can safely download deterministic fixtures with 1/2/4/8 workers, pause and resume them, and produce byte-identical output.

### [M2 — Firefox integration](https://github.com/HalcyonXP/firefox-download-manager/milestone/3)

Connect the engine to an explicit, accessible Firefox workflow.

- [#18 Windows Native Messaging host](https://github.com/HalcyonXP/firefox-download-manager/issues/18)
- [#19 Context-menu and creation dialog](https://github.com/HalcyonXP/firefox-download-manager/issues/19)
- [#20 Queue and progress dashboard](https://github.com/HalcyonXP/firefox-download-manager/issues/20)
- [#21 Local settings and diagnostic logging](https://github.com/HalcyonXP/firefox-download-manager/issues/21)

**Exit condition:** Firefox can create and control direct downloads while reconstructing accurate state after its UI closes and reopens.

### [M3 — Reliability and authenticated downloads](https://github.com/HalcyonXP/firefox-download-manager/milestone/4)

Protect correctness across restarts, changing resources, authentication, and hostile server behavior.

- [#22 Resource identity and crash recovery](https://github.com/HalcyonXP/firefox-download-manager/issues/22)
- [#23 Minimal authenticated-session handoff](https://github.com/HalcyonXP/firefox-download-manager/issues/23)
- [#24 Fallback, retry, and throttling hardening](https://github.com/HalcyonXP/firefox-download-manager/issues/24)
- [#25 Integrity validation and optional checksums](https://github.com/HalcyonXP/firefox-download-manager/issues/25)

**Exit condition:** interruption, resource mutation, malformed range responses, and expired authentication cannot result in a falsely successful or silently corrupted file.

### [M4 — Local release](https://github.com/HalcyonXP/firefox-download-manager/milestone/5)

Review, package, document, and qualify the first local release.

- [#26 Permission and native-helper security review](https://github.com/HalcyonXP/firefox-download-manager/issues/26)
- [#27 Windows installation and removal](https://github.com/HalcyonXP/firefox-download-manager/issues/27)
- [#28 End-to-end candidate qualification](https://github.com/HalcyonXP/firefox-download-manager/issues/28)
- [#46 Final-main artifact qualification and first release publication](https://github.com/HalcyonXP/firefox-download-manager/issues/46)

**Exit condition (revised 2026-09-09):** the exact package can install, run with Firefox Developer Edition, upgrade and uninstall on the owner's existing native Windows 11 x64 computer using isolated test profiles/application state, and GitHub provides checksummed release artifacts. A separate clean-machine test is unavailable and is not required for this personal release; release notes must say so. This replaces—not satisfies—the former clean-machine criterion. See [ADR 0011](decisions/0011-license-and-available-qualification.md).

### [M5 — Install, restart, click](https://github.com/HalcyonXP/firefox-download-manager/milestone/6)

The earlier milestones describe the completed scoped v0.1.0 release, not completion of the owner's newly confirmed workflow.

- [#48 Reconcile the owner workflow, architecture and acceptance](https://github.com/HalcyonXP/firefox-download-manager/issues/48)
- [#49 Prove persistent signing/install and safe Firefox handoff](https://github.com/HalcyonXP/firefox-download-manager/issues/49)
- [#50 Visible Rust tray companion, authenticated bridge and setup.exe](https://github.com/HalcyonXP/firefox-download-manager/issues/50)
- [#51 Automatic supported ordinary-click capture](https://github.com/HalcyonXP/firefox-download-manager/issues/51)
- [#52 Signed packaging and concise installation](https://github.com/HalcyonXP/firefox-download-manager/issues/52)
- [#53 Exact final-main owner-flow qualification/publication](https://github.com/HalcyonXP/firefox-download-manager/issues/53)

**Exit condition:** ordinary setup installation, real tray visibility, normal persistent signed-XPI installation, Firefox restart and an ordinary GGUF click produce one automatically running task and correct output in the native manager, without a competing browser output, temporary-loading API, manual Add/menu substitution or protection downgrade. Safe fallback, IPC/tray/setup/recovery/adversary/privacy/resource gates and new exact-artifact release verification must also pass. Signing/account authority and browser mechanism are gates, not assumed available. Working version target0.2.0 is an implementation choice, not a published promise.

```text
#48 -> #49 / #50 (independent work)
#49 + #50 -> #51
#49 + #50 + #51 -> #52
merged #49–#52 + authoritative main CI -> #53
```

## Historical initial-release critical path

```text
#9 -> #10 -> #11 -> #12
#11 + #12 -> #13
#11 -> #14 -> #15
#13 + #14 + #15 -> #16 -> #17 -> #18 -> #19/#20 -> #21
#13 + #15 + #17 -> #22/#24/#25
#18 + #19 + #22 -> #23
M2 + M3 -> #26 -> #27 -> #28 -> successful merged-main CI -> #46
```

Some work may proceed in parallel, but issue acceptance criteria define completion—not code presence alone.

## Correctness invariants

A release must preserve these invariants:

1. Every written byte belongs to exactly one validated assignment.
2. Completed segment coverage contains no gaps or overlaps.
3. A ranged response is accepted only when its status and `Content-Range` match the request.
4. Resource size and validators remain consistent throughout a segmented download.
5. Resume never combines bytes from resources known to be different.
6. Segmentation and reuse of nonempty completed coverage require a strong ETag; weak/absent identity uses a fresh single stream or fails (decision #22).
7. A final file is exposed only after size/integrity checks and successful promotion from partial state.
8. Existing files are never silently overwritten.
9. Credentials never appear in routine logs or persistent state by default.
10. Pause/cancel acknowledgement follows worker stop and a bytes-first critical checkpoint.
11. Automatic retries and progress/event memory are explicitly bounded.

## Release quality gates

- Formatting, linting, unit tests, and integration tests pass in GitHub Actions.
- Publication privacy checks remain clean for this public repository (#8, #29, #30); never import the private archive's retained Git refs.
- Adversarial HTTP fixtures cover malformed and changing responses.
- The final output is byte-identical for all supported worker counts.
- Firefox and helper restart paths are tested.
- Extension permissions are justified and reviewed.
- CPU, memory, disk, and progress-event behavior are measured on a multi-gigabyte fixture.
- Installation, upgrade, and removal are tested from Windows paths containing spaces.
- Known limitations and troubleshooting are documented.

## Project views and automation

Saved views provide:

- A Kanban board grouped by status
- The current `M2 — Firefox integration` milestone
- Native-helper work
- Firefox-extension work
- Security-sensitive work
- An unfiltered all-work table

Transferred issues retained their board statuses. Stale predecessor PR cards were removed; current dependency PRs are tracked here. Existing added-item/linked-PR/closure workflows still require verification. The inherited repository auto-add rule has not been reconfigured for the new repository; explicitly add new issues and PRs to the project rather than claiming that automation has run. The board is an owner-private planning view; public issue/milestone links above remain usable without it.

## Planning conventions

- Milestones describe user-visible delivery stages.
- Issues contain testable acceptance criteria and dependency references.
- `priority: critical` identifies milestone exit-path work.
- `priority: high` identifies important work that does not block the earliest vertical slice.
- Area labels identify ownership boundaries without duplicating milestones.
- Automation outcomes must be verified; workflow automation does not replace acceptance review.
- Scope changes should update this document and the architecture decision record in the same pull request.

## Implementation decisions since the public-authority handoff

#32/#33 restored the failed cancellation baseline; public PR and merged-main Windows CI passed with repeated cancellation regressions. #23 implements per-download default-store session handoff, not full session cloning. Its conservative private/container/partition limitations, permission granularity, memory-only recovery behavior, and fresh-task retry are in [AUTHENTICATION.md](AUTHENTICATION.md) and ADR 0010. Wire v2 is unchanged. #25 advances internal task state to v4 while retaining v3 session meaning. #24 and #25 are implemented; security/packaging/real-browser release gates remain required.

#24 extends concurrency admission to probes/redirects as well as transfers, retains 429/503 guidance across peer tasks and settings, reduces effective retry widths, and permits only bounded identity revalidation after a worker 416. Optional tail duplication is now off by default; local fixture timing/request-cost evidence and its limits are in [RELIABILITY.md](RELIABILITY.md). No wire change or benchmark-driven arbitrary worker restart policy is introduced.

### Integrity checkpoint (#25)

[INTEGRITY.md](INTEGRITY.md) records completion ordering, streamed SHA-256, immutable recovery expectations, explicit mismatch retention, and cancellation/file-lock limits. Unit/native integration and actual helper kill/restart cover checksum behavior; they do not extend the earlier real-Firefox evidence or waive large-file/clean-install qualification. Documentation audit also removed stale front-matter claims that #32 still blocked features and that authentication was still unadvertised; merged code, board state, and later plan notes already agreed those were resolved.

### Pre-packaging security checkpoint (#26)

[SECURITY_REVIEW.md](SECURITY_REVIEW.md) records the scoped review, fixed wildcard-permission and validator-redaction findings, explicit Firefox 156/CSP/private-window policy, actual native log/state inspection, and dependency audits. Review completion is not release approval. #27 must resolve development-installer reparse/ownership/transactional-upgrade findings and re-run the affected security checks. #28 must qualify the final policy/artifacts in Firefox and the target installation environment, without touching an unowned live profile.

### Packaging checkpoint (#27)

[PACKAGING_PLAN.md](PACKAGING_PLAN.md) records the local Rust setup/candidate-artifact design, ownership and recovery requirements, and remaining evidence. The Rust setup coordinator/CLI, bounded helper probe, candidate builder and preservation/fault tests now exist; old development scripts are refusal-only stubs. The initial static MSVC proposal was replaced by a reviewed pinned LLVM/MinGW/UCRT release target after distribution-term review. Actual public package CI `34274073613` passed on Windows Server x64 and Windows 11 ARM64 with x64 emulation; repeated clean-target builds were byte-identical within each environment, not across environments. The CI candidate also passed a native Windows 11 x64 probe locally without registration or browser use. #28 final browser/resource qualification and release remain pending.

### Post-packaging baseline correction (#39)

#27 / PR #38 merged as `9928058` after two successful PR runs. Merged-main CI `34277988932` then failed a release-target progress-event minimum-count assertion. #39 blocks #28 until this baseline is verified; #28 returned to Backlog, with its draft qualification harness preserved separately. [PROGRESS_REGRESSION.md](PROGRESS_REGRESSION.md) records the invalid timing/count premise, explicit response-barrier replacement, late-consumer counterexample and evidence limits. Prior #32 cancellation-observation work remains intact. This interruption changes the immediate sequence, not release scope or supported-platform requirements.

### Additional retry-control baseline correction (#41)

#27 / PR #38 merged as `9928058`. #39 / PR #40 addresses a failed merged-main progress-count assertion, but its CI `34283625146` then failed an existing one-second probe-cancellation timeout while the progress cases passed. #41 is an independent focused correction from main; [RETRY_CANCELLATION_REGRESSION.md](RETRY_CANCELLATION_REGRESSION.md) separates deterministic retry readiness from joined, durably checkpointed acknowledgement. #39 is paused behind #41, and #28 remains in Backlog. Resolve #41 with its own gates, update #39 onto verified main, and require the combined baseline before resuming qualification. No failed CI result or required release coverage is waived.


#41 / PR #42 subsequently merged as `78a92ea` after CI `34286434188` passed all three jobs. #39 is now updated onto that main and its full combined gate must pass; merged-main CI `34287899024` was still pending at this integration checkpoint. #28 stays blocked until the current baseline is verified. A successful Dependabot Updates run is not the CI workflow and is never used as that gate.

## Pre-#44 checkpoint: artifact slices, not release approval

The preceding #39/#41 integration notes are historical: merged-main CI `34287899024` passed, combined #39 PR CI `34288270754` passed, #39 merged as `01a49d0`, and main CI `34289550859` passed all three jobs. #28 resumed; draft PR #43 foundation `0ccc6ea` passed CI `34292916341` with native candidate/2-GiB evidence as well as existing package lifecycles.

A clean-driver native Windows 11 x64 main-artifact run passed. A later dirty-driver real Firefox 156/aurora packaged-XPI slice passed settings, checksum success/mismatch, pause/resume, actual optional cookie/site permission plus revocation, session-loss restart refusal and owned upgrade/removal. Fixture regressions correct last-byte probe classification and expected peer resets; no product behavior or release criteria were weakened. [QUALIFICATION_PLAN.md](QUALIFICATION_PLAN.md) and [FIREFOX_QUALIFICATION.md](FIREFOX_QUALIFICATION.md) distinguish inputs, old narrower evidence, successes and remaining gates. No live profile was used, unowned process terminated, signing preference overridden, OS feature enabled or qualified release published.

## New progress-deadline investigation (#44)

The owner has renewed autonomous execution through install readiness; no further approval is needed within the established scope. #28's MIT and existing-machine decisions are preserved in draft PR #43 (not reverted on this independent main-based correction branch). A second computer or clean OS is not required; safety, privacy, ownership and exact-artifact gates remain.

CI `34321346203` at #28 tip `26e302b` failed two existing release-target progress deadlines, with 29 other lifecycle cases passing. #44 is In Progress from main `01a49d0`, whose engine/test source is identical to the failing tip. #28 is temporarily Backlog. The earlier #39/#41 successes remain historical; they do not waive this new failed gate. [PROGRESS_DEADLINES.md](PROGRESS_DEADLINES.md) records observations, diagnostic intervention and unresolved cause. Do not merge a diagnostics-only passing rerun as an explanation or release approval.

#44's diagnostics-only CI `34325455523` passed, but that alone did not meet acceptance. Real connected-probe counterexamples now demonstrate valid preparation outlasting the old aggregate ten-second bound. The correction retains ten-second active-cadence and five-second drain watchdogs, introduces explicit whole-workflow containment, and preserves all output/spacing/coalescing assertions. Phase-clock, missing-event and keyed-coalescing mutations fail as intended. Final combined gates remain required; the unavailable original trace is not replaced with a speculative cause.

CI `34327670669` then exposed the inherited mandatory-terminal-rate assertion in a controlled delayed case, not a repeated deadline failure. #44 explicitly reconciles that predicate with the nullable rate/sliding-window contract: retain the observed active-window rate requirement and phase-history equality, but do not invent `Some` at completion after insufficient recent sampling. A deterministic stale-window unit and history-clearing mutation cover this correction; production behavior and final-state/bytes/ETA/output requirements remain unchanged.

## #28 resumed after #44 merge (2026-09-09)

#44 / PR #45 merged as `6482a17892fb2e532077b08ce451a1bf0929de62` after final PR CI `34331518837` passed all three jobs. Merged-main CI `34333682602` is pending at this checkpoint and remains an authoritative gate. #28 is In Progress again on its preserved branch/draft PR #43. The merge retains both the qualification/licensing vocabulary and #44's phase/rate decisions, rather than choosing one side of the documentation conflicts. MIT/eight-payload packaging/nine-leaf XPI and ADR 0011 remain intact.

The latest license-bearing local `26e302b` package passed twelve native checks and 2 GiB from a clean harness. The last actual Firefox run remains the clean pre-license `5ce837c` candidate slice, not a license-bearing rerun. Fresh count-only observation found nineteen unowned Firefox processes; none was stopped or its profile accessed. Native/harness work continues independently. Remaining actual controls/security/restart and artifact/adversary/report-safety/publication work is recorded in #28. The owner requires autonomous completion through install readiness, without additional in-scope approval requests; established safety/privacy/ownership constraints still apply.

Merged-main CI `34333682602` subsequently passed all three jobs, as did integrated #28 CI `34334123810` at `f90f04c`. The expanded native artifact matrix passed 26 cases/2 GiB with a dirty identified driver. [NATIVE_QUALIFICATION.md](NATIVE_QUALIFICATION.md) preserves scope and the report-sink, constructor, adaptive-prefix and fixture-error corrections; clean-driver/new-tip qualification is still pending. The latest browser count is twelve, not authority to terminate or inspect those processes. #28 remains In Progress/draft, with no tag/release.

Clean driver `520f029` subsequently passed all 26 native cases/2 GiB on the same licensed CI candidate. Installer-driver review then extended the shared report/metadata/cleanup policy; twenty local harness tests passed, with actual lifecycle CI pending at that checkpoint.

CI `34341064343` at `37c5ed2` subsequently passed all three jobs, including the hardened installer and exact candidate native26/2GiB. Clean37 locally passed native26/2GiB against the downloaded licensed artifact. The owner confirmed both Firefox editions closed and would use Edge; fresh preflights passed and clean37 reran the original nine actual Firefox checks on the MIT-bearing bytes. A later dirty, source-hashed driver expands that to twenty actual checks, with a separate native26/2GiB and actual setup lifecycle pass on the existing native Windows11 x64 machine. Twenty-six policy tests and baseline-verified mutations pass separately.

[FIREFOX_QUALIFICATION.md](FIREFOX_QUALIFICATION.md) records toolbar/menu/controls/CSP, private capability denial (not static-document blocking), validating-phase cancellation, durable-prefix restart and renewed-session Add, plus failed assumptions and an interrupted driver install with preserved evidence/reviewed setup-only recovery. No product behavior/protection, live profile, licensing or environment scope was changed. Clean1a8 subsequently passed all three local exact-candidate drivers. CI34349940646 failed two new policy tests because their `artifacts` parent did not exist before packaging; its later lifecycle executable lookup also failed because the build was skipped, without running lifecycle tests. A fresh source-only reproduction, per-test container setup and missing-setup mutation now pass the twenty-seven-test policy gate. Corrected new-tip/final-artifact gates, mapped adversarial/support/resource acceptance and full publication review remain. #28/PR43 stays In Progress/draft; no release/tag exists.


## Published outcome — #46

[v0.1.0](https://github.com/HalcyonXP/firefox-download-manager/releases/tag/v0.1.0) was published2026-09-09T15:09:37Z from the exact tested main-d03 ZIP. Anonymous public source/tag/notes and four-asset readback verified its identity/checksums. [Release record](releases/v0.1.0.md) preserves qualification, privacy snapshots, the checksum-attachment correction, unavailable coverage and audit failures. M4's release exit conditions are satisfied; the board/PR tracks administrative closeout. Later auditor/docs commits and their CI artifacts do not replace the immutable product tag. Earlier pending/no-release checkpoints below and in qualification documents are historical, not the current publication state.

## Final-main checkpoint — #46 (before publication)

PR43 merged as `d03a56c373bfee37776d23031908a93fc68da89a`; authoritative main CI34357070862 passed all three jobs. After verified Backlog→Ready→In Progress, #46 froze that intended product source and exact CI ZIP `ff67edcd4713bfe37407970645c51527d9a99e97837d089304a192499bcfd62e` (descriptor `cc3a40e68b74eebaceab586cef5c6a3230c4963a1a48bf4da6ea2d70e94abc2a`). Clean-d03 drivers passed installer7, native26/default2GiB and actualFirefox20 on the existing native Windows11 x64 installation. Owned cleanup completed and normal Firefox may reopen; no further browser-profile mutation is planned for these unchanged bytes.

The completed archive review closed the former then-running-CI gap:89 new archives plus54 same-ID/current-API-digest matching previously reviewed artifacts,860 newly scanned streams/604 unique payloads, no findings/gaps/new contacts. Seven inventoried uninspectable caches were deleted and absence verified, not content-inspected. A separate issue/PR metadata-count contradiction blocked publication and prompted the explicit dual-endpoint/totals correction in `PUBLICATION_PRIVACY.md`. Final corrected metadata/release-input review and tag/assets/publication verification remain required. Follow-up auditor/docs commits do not change or relabel the frozen main-d03 product artifact; M4 is still incomplete.

## #50 transport foundation and authority clarification (2026-09-10)

The owner explicitly directs autonomous completion without routine approval requests (ADR0014); operational ownership/closed-app preflights and missing credentials remain genuine conditions. #49–#53 issue text now reflects that distinction. Only recognized signing secret/environment-variable names were checked: none were present. This does not establish that the owner has no publisher account; no credential store or browser profile was searched and no submission was made.

PR56's initial d88e7b7 CI34391997304 passed all three jobs. New unintegrated IPC source is separate evidence: [LOCAL_IPC.md](LOCAL_IPC.md) records actual same-user/cross-process transport tests and failed/surviving-mutation history. The underlying pending-write discovery required the explicit narrow compiler-policy exception in ADR0015, not weaker shutdown assertions. Only after independent authority, engine/stdio coordination and setup lifecycle gates pass can #50 close; #51–#53 remain Backlog. Final new-head CI is required for these changed inputs, and no old UI/package report is relabeled.

Before engine integration, the initial 64 KiB IPC draft was reconciled with the existing Rust/Firefox 1 MiB native-message contract. IPC now shares the native constant and tests a decoder-valid maximum-size body; metadata/log bounds are unchanged. See LOCAL_IPC.md. This is an explicit draft refinement, not a delivered bridge or a change to v0.1.0.

## Installation scope

Target local Firefox Developer Edition installations on Windows 11. A public AMO listing is not required. Signing and listing are distinct: unlisted signing involves Mozilla submission without a public listing. Preserve browser protections and verify persistent installation/restart behavior using exact artifact bytes. See ADR0013.

## #50 first engine/IPC integration (2026-09-10)

The previous multi-client coordinator was a proposal. The selected first bridge preserves one active browser controller and one engine/settings owner; further connections cannot dispatch while it is active. The transport's four reservations do not imply four engine controllers. [COMPANION_DESIGN.md](COMPANION_DESIGN.md) records the trade-off and bounded async session design. The opt-in `local-bridge` native-host API and preview worker now serve wire2 over authenticated pipes without letting client loss stop the engine. Actual pipe/engine tests are separate from Firefox, native stdio forwarding, installed private authority and package qualification, which remain unfinished. Do not close #50 or promote #51 based on this increment. Current-head validation and new preview evidence are required; old reports remain version-specific.

## #50 protected runtime-record component

An opt-in setup component now implements protected create-new record files and independent permission readback; [PRIVATE_RUNTIME_RECORD.md](PRIVATE_RUNTIME_RECORD.md) defines its scope, protocol and evidence. It requires an already owned, protected directory and does not prepare/adopt installed state. An additional opt-in exclusive directory creator now supplies a read-only bootstrap DACL, retained no-delete lease and independent initial/final descriptor verification below a caller-owned parent; it never adopts existing entries. Installed caller/receipt integration, record/generation binding, singleton coordination, stdio forwarding and installed lifecycle qualification remain open. The default setup package and current memory-only preview do not select this feature.


## #50 installed-image/record composition

The optional installed-runtime component now inspects/retains a content-consistent current image and receipt under existing setup coordination, and publishes/reads a closed protected endpoint/capability record from a privately owned bound server exposed only after successful publication. This is not the installed caller or state-lock singleton integration. The selected next deployment uses one image for companion/bridge entry modes, covered by the existing helper digest; current preview and legacy package selection are unchanged. Native discovery deadlines, stdio cancellation/joining, application entry modes, setup/shortcut migration and actual browser acceptance remain open. See COMPANION_DESIGN.md and PRIVATE_RUNTIME_RECORD.md.


## Next integration checkpoint

Connect the existing visible-shell gate, retained image/receipt binding, engine state-lock ownership and protected runtime publication in one application entry. Then connect native stdio forwarding and verify one small independently checked transfer through that entry before expanding installer UI/shortcut and ordinary-click capture. Use isolated fixtures until shared-registration/browser preflights pass. Persistent signed-XPI qualification remains a separate required gate; a temporary or synthetic connection is not completion of the install/restart/click workflow.


Checkpoint update: the opt-in application now connects the visible-shell gate, image/receipt inspection, engine state lock, private publication and shared controller loop. Real executable stdio/IPC tests pass one small transfer with disconnect/reconnect and joined stalled-I/O cleanup. Production setup/probe/shortcut selection and actual installed Firefox acceptance remain incomplete; the next checkpoint is paired setup integration, not additional standalone primitive expansion.

A concrete relay integration defect was found and corrected before installed selection: private-parent-pipe loss could leave an I/O-only pump blocked on external stdio. Dedicated liveness/input observation now retires those processes; normal and parent-loss cases are separate tested observations, not a process-tree containment assumption.

The paired development candidate now selects the application and compatible setup probe/UI together, preserving the eight payload roles and existing helper receipt digest. CLI/probe compatibility is not a shortcut ownership migration. Real-binary mock-registration lifecycle and native no-install UI checks are available; production selection remains refused. Next: owned actual GUI-to-tray/bridge handoff, explicit shortcut receipt/journal ownership, persistent signed XPI/capture and exact installed qualification. See COMPANION_CANDIDATE.md; this is not the end-user quick start.
