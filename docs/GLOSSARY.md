# Shared project vocabulary

## Current meaning

- **Canonical repository / `origin`**: [HalcyonXP/firefox-download-manager](https://github.com/HalcyonXP/firefox-download-manager), public and authoritative for code, issues, CI, and future releases. There is no publication-mirror workflow.
- **Private predecessor / archive**: retained original review/Git records, not an alternative development repository. It must remain private; its sensitive original refs must never be imported.
- **Current issue number**: an issue in the canonical repository. [ISSUE_MIGRATION.md](ISSUE_MIGRATION.md) maps historical numbers and implementation commits; old commit messages retain their historical namespace.
- **Manager**: the Firefox UI plus its on-demand Rust native helper, not Firefox's built-in downloads.
- **Explicit capture**: a link context-menu action or pasted direct HTTP(S) URL; never automatic interception.
- **Proposed filename**: a Windows-safe name derived from the URL or entered by the user. #19 resolves this before submission; server metadata cannot choose a path. Collision suffixes are selected safely at final promotion.
- **Worker**: one transfer lane (1/2/4/8), not permission to exceed the separate global/per-host request caps.
- **Partial**: helper-managed, unvalidated download storage; not final output.
- **Checkpoint**: flushed completed ranges followed by durable metadata, in that order.
- **Snapshot**: the helper's authoritative task projection; cached UI state is explicitly stale when disconnected.
- **Promotion**: no-overwrite publication only after complete coverage and validation.
- **Strong resource identity** (#22): equal final URL, size, mode and validators, including a strong ETag; weak tags and dates alone never justify combining persisted/request byte ranges.
- **Ready to install**: versioned, checksummed artifacts and documented setup with release gates passed. It does not imply installation into the user's existing Firefox profile.
- **Qualification gap**: a release criterion for which evidence is missing. Code presence or a mock test is not end-to-end evidence.

## Privacy meanings

- **Writable-history cleanup** (#29): removal from editable branches/tags and tracked content; it does not imply erasure from GitHub caches or closed-PR refs.
- **Isolation, not erasure** (#30): the independent repository does not contain the known original sensitive commits. GitHub retains originals in the private archive.
- **Publication privacy clearance**: a repository-specific, time-scoped audit, not anonymity or a guarantee about future uploads. Git/platform-metadata checks alone do not inspect workflow log/archive content.
- **Public attribution**: GitHub handles, noreply identities, reviewed technical principals, and third-party notices. GitHub's SSH routing principal and Dependabot's published support sign-offs are not private owner contact data; commit author/committer emails remain noreply-only.

## Decision and learning history — 2026-09-08

The user authorized autonomous development toward an installable release, without changing their live browser profile. Initial work through the native host was followed by explicit capture, dashboard/protocol v2, settings, and strong resource identity. Implementation now includes #23 session handoff, #24 polite admission, and #25 integrity/SHA-256. #32 restored the cancellation baseline with deterministic local-ownership evidence. #26–#28 remain security, packaging, and release work.

#19 interpreted “resolved filename” as a visible, editable, Windows-safe proposed name, because the then-current v1 contract had no preview command. That is an implementation decision, not a separately confirmed user preference. Their later opt-in referrer/cookie boundary is now implemented in #23. Mocked Firefox transport was useful rendered-UI evidence, not real Firefox Native Messaging qualification.

Privacy cleanup first rewrote contact metadata without changing writable branch tip trees. GitHub rejected removal of retained PR refs; Support browser sign-in was unavailable and no request was submitted. #30 selected an independent target while preserving private records. Fresh Dependabot refs/logs were audited rather than assuming the new repository remained empty. Pattern false positives for project-board URLs, GitHub SSH routing, and public bot sign-offs were narrowly reviewed and regression-tested.

The user's subsequent request **explicitly authorized public visibility and documentation referring to the new repository**. #8 supersedes the temporary mirror arrangement: regular issues were transferred, numbering was mapped, and the new repository became the single authority. Transfer changed issue IDs and automatically rewrote references; state, authorship, timestamps, comments, and acceptance meaning were verified rather than assuming identity numbers remained stable. No new license preference or installable-release qualification was inferred from permission to make the repository public.

## Cancellation evidence (#32)

- **Local request start**: the scheduler initiated a send, counted before awaiting the HTTP response. It is not the time the remote server records that request.
- **Stop acknowledgement**: all owned worker futures have joined, the final local progress sample is published, reporter ownership closes, and partial coverage/bytes cannot change until an explicit resume. Already-transmitted bytes and remote handler observations cannot be retracted.

The prior 150 ms stable-server-ledger assertion conflated remote observation with local shutdown. A test-only barrier now holds fully received requests before ledger insertion, deterministically demonstrating late observation after successful local cancellation without late workers or disk writes. The regression also verifies that resumed requests skip retained coverage and final bytes match. A final metrics publication after all joins prevents concurrent worker samples from leaving a stale terminal projection. These are measured boundaries, not permission for workers to survive acknowledgement.

## Session vocabulary (#23)

- **Session handoff**: explicit per-download transfer of eligible default-store cookies and optional same-origin referrer/HTTPS Basic or Bearer values. Not a request clone, cookie jar, or consent to inspect arbitrary browsing data.
- **Site permission**: Firefox's optional scheme/host grant, covering all ports; do not confuse it with the helper's exact scheme/host/port **origin confinement**.
- **Needs-session marker**: a required non-secret Boolean introduced in internal v3 and retained in v4. It forbids a restarted task from silently sending without memory-only context; it contains no cookie values.
- **Fresh authenticated retry**: sign in and create a new task. V2 does not accept replacement credentials on resume, and previously retained bytes cannot change authentication context.

Initial testing caught two implementation/evidence mistakes: old persistence tests hardcoded v2 as current/v3 as future, and a new recovery test expected a control error where `resume` deliberately returns an authoritative failed snapshot. Tests now preserve the actual contracts, including strict legacy migration and required-marker corruption rejection. An early blanket 401/403-to-expired mapping was corrected: only supplied context can expire; unauthenticated 401 is required-auth and 403 remains ordinary HTTP failure.

The #23 actual-Firefox slice used a fresh profile, real optional-permission approval, actual native transport, and exact 4 MiB output. Its first test ledger included unrelated favicon traffic; selection was corrected and the test rerun from a new profile. A later run was intentionally blocked by an existing unowned Firefox process. Do not terminate that process, treat prototype automation as a release installer, or publish its raw local profiles/logs. #28 must harden and extend the release harness rather than reuse its early cleanup assumptions blindly.

## Reliability vocabulary (#24)

- **Admission cap**: maximum locally admitted HTTP requests (including probes and redirects), not an assertion about idle/TIME_WAIT sockets or HTTP/2 stream-to-socket ratios.
- **Configured workers**: the persisted user selection. **Effective workers**: current transfer width, reduced on transient retries without changing that selection or retry budget.
- **Origin cooldown**: a shared monotonic not-before deadline from 429/503; peers cannot bypass it by starting another task or saving settings. Already-admitted work may finish.
- **Tail hedge**: one optional duplicate of the sole remaining assignment, not generic worker restart or overlapping committed bytes. It is off by default; the favorable loopback measurement is not general throughput evidence.

#24 closed the previously uncapped-probe path, retained pressure across settings changes, and corrected HTTP-date Retry-After rounding: fractional remaining seconds round up, not down before a server deadline. The old tail regression initially failed because its formerly implicit duplication policy became opt-in; it now opts in explicitly and the default no-duplication baseline is measured separately. Full original range/cancellation invariants remain enforced.

## Integrity terms and learning (#25)

- **Expected SHA-256**: optional immutable per-Add digest obtained by the user, never silently removed on retry, reconnect, or recovery. Not a signature or a server-selected validation policy.
- **Validation lease**: non-cloneable ownership of frozen helper storage and its file lock, spanning validation through one no-overwrite promotion. Unpublished lease drop permits a future full revalidation. Not a continuous guarantee against external mutation of published files.
- **Transfer versus validation progress**: byte counts/download rate describe transfer; the checking/publishing phases have no invented hashing ETA. Cancel is supported while validating, not after promotion begins.

Format-v4 tests exposed the need for genuine old v1/v2/v3 shapes without newer keys. State review also caught the old documentation/Serde nullable-key omission mismatch; v4 now enforces required nullable task/validator keys. An initial phase-metric change broke the established completed-transfer-rate projection; it was replaced with UI-only checking/publishing text, preserving completed history metrics. Independent known vectors and Python fixture digests avoid relying solely on the same hashing implementation for expected test results. See [INTEGRITY.md](INTEGRITY.md).

## Review terms and boundaries (#26)

- **Eligible host pattern**: manifest authority that may be requested, not authority already granted. Canonical `*` URL hosts must never become wildcard grants. Firefox scheme/host grants still cover all ports.
- **Protected recovery data**: exact URLs and remote validators needed for recovery; not a secret-free export. Memory-only supplied context does not imply remote content cannot reflect secrets.
- **Review complete versus release approved**: #26 records findings, regressions and residual risks; #27 installer blockers and #28 final-artifact qualification are not waived.

The review found a real wildcard-pattern construction edge case, not an observed cookie breach. It also corrected stale no-host-access prose, redacted opaque validators, and narrowed the unqualified Firefox 128 claim to the actually exercised 156 API baseline. These are implementation safety decisions, not newly confirmed user preferences. The actual privacy fixture initially violated its own exact-referrer contract and returned AUTH_EXPIRED; the test input, not the server contract, was corrected.

## Packaging terms (#27)

- **Package descriptor**: bounded, closed metadata for fixed local payload leaves, paired version and file SHA-256 values. Hash consistency does not authenticate the publisher or prove the asserted source commit.
- **Installation receipt**: versioned ownership/recovery metadata for generated installation files; not task state, a signature or a compromised-account defense.
- **Candidate artifact**: CI-produced versioned/checksummed output awaiting #28 qualification, not an approved release.
- **Directory lease**: ordinary Windows ancestor handles held without delete sharing while their paths are used. It narrows ordinary rename/reparse races; it does not confer authority over arbitrary other account actors.

- **Generation**: immutable UUID-named helper/XPI/manifest directory. Registration selects the complete current generation; setup does not silently replace its bytes or load its XPI into Firefox.
- **Setup lock domain**: one canonical local-application-data root, normally the current user’s standard environment. Isolated environment overrides are separate domains; this is cooperative file exclusion, not a cross-environment kernel mutex.
- **Recover versus repair**: `recover` replays a bounded known journal conservatively; `repair` has no journal and only rebinds verified current content from absent/stale same-receipt registration. Neither adopts foreign authority.
- **Candidate versus qualified release**: a checksummed builder/CI artifact has consistency/provenance metadata, not #28 browser/installation/resource approval.
- **Static support with system UCRT**: LLVM/MinGW runtime support is linked into the x64 executable; Microsoft UCRT/API DLLs are supplied by Windows. This is not the superseded static MSVC CRT proposal.
- **Windows 11 x64 emulation evidence**: running the x64 artifact on a fresh ARM64 Windows 11 runner; distinct from native x64 coverage and from a factory-clean image without development tools.

- **Descriptor digest versus ZIP digest**: `descriptor_sha256` identifies `package.json`; `PACKAGE-SHA256SUMS.txt` identifies the distributable ZIP. Initial #27 evidence used the ambiguous `package_sha256` key for the descriptor, corrected before release.
- **Same-environment reproducibility**: independent clean Cargo target builds match all package bytes on one environment. Both local and CI comparisons passed; local-versus-CI binaries differed, so cross-environment bit reproducibility is not claimed.

- **Ordinary progress cadence**: minimum spacing between replaceable progress events, not a guaranteed consumer delivery frequency. Missed ticks and unread same-task samples may be coalesced.
- **Terminal snapshot versus ordinary progress**: the terminal snapshot carries final joined metrics; the last ordinary progress event can be older and is not itself a completion barrier.
- **Selected response pause**: test-only gate after a matching request enters the server ledger, before its response starts. Distinct from the pre-ledger observation pause used for cancellation tests; neither is a sleep-based readiness guess.

- **Retry cancellation readiness**: the retry wait becomes ready for a cancellation signal without requiring its timer to expire. It is distinct from when the executor schedules that ready future.
- **Durable control acknowledgement**: the pause/cancel/shutdown result follows safe owned-work stopping and the critical checkpoint. A test deadlock-containment deadline is not a product latency SLO or permission to abandon blocking filesystem work.

## Qualification vocabulary and learning (#28)

- **Artifact slice**: an explicit subset exercised against identified real package bytes. Real Firefox/native transport does not make a manager-page slice the full release/support matrix.
- **Clean driver revision**: committed harness files with a clean Git worktree, not a factory-clean Windows image. Reports bind source revision and file hashes; a dirty driver is recorded, never silently qualified.
- **Boundary probes versus workers**: the helper verifies the first and last resource bytes before scheduling. The Python fixture initially misclassified the last-byte probe as a worker; the selector was corrected, preserving older reports as probe-stage evidence only.
- **Worker-body gate**: test-owned hold after headers for non-probe slow responses. It makes an active UI control observable without assuming a transfer lasts long enough; it does not prove retained nonempty coverage.
- **Expected peer disconnect versus fixture failure**: cancellation/navigation can reset a socket before headers or during a body. Typed connection errors/timeouts are expected; unknown exceptions or unjoined owned handlers invalidate the report.
- **Owned-process resource observation**: helper working set/private usage, CPU, aggregate I/O and event counts for identified inputs. OS cache/kernel/other-process memory and physical-device/Internet performance are not inferred from those fields.

[FIREFOX_QUALIFICATION.md](FIREFOX_QUALIFICATION.md) preserves the probe-gate correction, independently reproduced peer-reset boundary, actual browser successes and remaining gaps. The old prototype's manual registration/signing override/tree cleanup was not reused.

## Owner clarification (#28, ADR 0011)

- **Permissive FOSS / MIT**: the owner authorized broad FOSS reuse; MIT is our implementation choice, not a claim that the owner specified that identifier. Copy, modify, redistribute or sell with its copyright/permission notice; third-party terms remain independent.
- **Existing-machine qualification**: real native Windows 11 x64 / Developer Edition tests in owned isolated application/profile domains on the owner's only computer. Not a clean OS or just an API mock. It replaces the first release's separate clean-machine gate, without marking that unavailable test passed.
- **Unavailable versus failing coverage**: clean-machine evidence is unavailable under the revised plan and must be disclosed; an actual failing correctness/security test still blocks release. A user-authorized environment change is not permission to relax assertions or manufacture a green run.

See [ADR 0011](decisions/0011-license-and-available-qualification.md). Historical no-license/clean-machine prerequisites are superseded; artifact/source identity, native-x64 intent and safety boundaries are retained.

## Progress-test deadlines (#44)

- **Preparation readiness**: the independently published 2 MiB prefix and selected response-barrier arrival, before consuming cadence samples. Not a guarantee of durable retained-range metadata.
- **Active-cadence watchdog**: ten-second test containment for observing the held-prefix samples after readiness; not a product delivery-frequency SLO. Preparation cannot spend this clock.
- **Workflow containment**: a separate, single sixty-second test bound covering preparation/transfer/validation/promotion; neither a product latency promise nor a Windows I/O upper bound. The five-second late-consumer drain bound remains distinct.
- **Controlled counterexample versus reconstruction**: holding a real accepted probe through the old ten-second deadline demonstrates an invalid aggregate test premise. It does not reconstruct the unavailable original CI schedule.

See [PROGRESS_DEADLINES.md](PROGRESS_DEADLINES.md). First-party licensing and existing-machine qualification are preserved separately in #28/PR43; this correction does not require another computer or alter safety constraints.

- **Available rate versus retained transfer history (#44)**: an actual rate must be observed in the controlled active window. A later final sample may have insufficient recent history and legitimately yield `None`; the nullable rate at `Validating` entry is retained through promotion/completion, not forcibly cleared or required to be `Some`. Zero terminal ETA and exact joined bytes remain independent requirements. The CI follow-up and deterministic stale-window counterexample are in [PROGRESS_DEADLINES.md](PROGRESS_DEADLINES.md).

## Expanded native qualification (#28)

- **Durable retained prefix**: joined control acknowledgement plus actual persisted, bounded disjoint half-open coverage and an independent disk-prefix hash. Aggregate received bytes alone do not establish it; adjacent ranges need not be stored as one entry.
- **Retained gate waiter**: an observed server handler held after headers beyond the prefix. Cancelled remote waiters can outlive local worker joins; their count is not the helper's worker/admission count.
- **Evidence sink**: a new bounded report under ordinary owned artifact ancestors, staged and published without replacing a destination. Narrow ASCII report spelling is not the product's download-filename policy. Metadata-only Windows handles do not provide the read-access sharing lease proven by the rename regression.
- **Qualified component versus qualified release**: exact native bytes can pass 26 cases/2 GiB while actual Firefox, source/tag and publication gates remain incomplete. No earlier artifact, dirty driver or unavailable clean-machine test is silently upgraded.

See [NATIVE_QUALIFICATION.md](NATIVE_QUALIFICATION.md) for the demonstrated corrections and remaining uncertainty.

- **Recorded binding versus plausible path**: a verified generation's exact registration can establish bounded test ownership; an unrecorded value is not adopted for deletion from its location/addon ID alone. Preserve unresolved installation authority for reviewed recovery, with no success report.

## Actual Firefox expansion (#28)

- **Private capability denial, not document denial**: Firefox156 withholds extension APIs in the real private window and disables toolbar/link capture; the manager HTML can still render. Qualification also requires actual form non-submission, no new task/network work/output. `incognito: not_allowed` is not a static-document ACL.
- **Validating-phase UI observation**: observe the real rendered phase, Cancel present/Pause absent, then exercise the real control/confirmation/native acknowledgement and no-publication boundary. A 2 GiB fixture makes this observable in the tested run; it is not a promised hashing duration or scheduling SLO.
- **UI-port reconnect versus helper crash**: reattaching the manager UI must preserve task identity without replaying Add. It does not prove termination/recovery of a helper; native retained-handle crash tests and real Firefox/helper restart tests provide separate evidence.
- **Pre-allocation ownership record**: private source/artifact/domain context written before fixture/setup mutation, supplementing the exact binding recorded after a verified install. It is neither automatic cleanup authority for an unknown registration nor a qualification report. Interrupted driver setup and explicit reviewed recovery remain failed-run history.

[FIREFOX_QUALIFICATION.md](FIREFOX_QUALIFICATION.md) records the corrected assumptions, independent mutation baselines and exact candidate scopes.
