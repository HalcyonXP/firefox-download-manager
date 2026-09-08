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
