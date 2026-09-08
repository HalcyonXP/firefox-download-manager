# Project Plan

GitHub execution board: [Firefox Download Manager](https://github.com/users/HalcyonXP/projects/1)

Current state is maintained on the project board. Planning is complete; implementation proceeds through the lowest-numbered issue in **Ready** status, with dependent work retained in **Backlog** until it is unblocked.

Current protocol: v2 (paired helper/extension upgrade, #20); v1 remains archived.

Project vocabulary and autonomous handoff decisions: [GLOSSARY.md](GLOSSARY.md).

## Public, authoritative repository (2026-09-08)

The user explicitly authorized public visibility and requested that all documentation refer to **[HalcyonXP/firefox-download-manager](https://github.com/HalcyonXP/firefox-download-manager)**. #8 supersedes the temporary two-repository arrangement. This public repository now owns code, work issues, milestones, CI, and future releases. `origin` points here; there is no publication-remote synchronization step.

Twenty-two regular issues were transferred with their states and comment history. Current documentation uses their new numbers; [ISSUE_MIGRATION.md](ISSUE_MIGRATION.md) records the mapping and links historical implementations to cleaned public commits. The owner-private planning board retains the corresponding work statuses. The predecessor stays private solely as an archive, not an alternative source of truth.

Implementation is complete through #22. The cancellation-observation investigation #32 temporarily gates #23, #24, and #25; after the baseline is verified they return to Ready in numeric order. Privacy work is recorded in #29 and #30. Public visibility is independently verified, but it does not waive hosted-CI, real Firefox, installation, performance, or release-artifact qualification. See [PUBLICATION_PRIVACY.md](PUBLICATION_PRIVACY.md) and [ADR 0009](decisions/0009-public-authority.md).

## Product goal

Create a trustworthy local download manager for Firefox Developer Edition that can improve throughput when an HTTP(S) server limits individual connections and supports byte-range requests.

The Firefox extension captures user intent and displays state. A Rust native helper owns networking, scheduling, disk writes, validation, and recovery.

## Scope

### Initial scope

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
- Automatic interception of every built-in Firefox download in the first release
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
- [#28 End-to-end qualification and first release](https://github.com/HalcyonXP/firefox-download-manager/issues/28)

**Exit condition:** a clean Windows 11 environment can install, use, upgrade, and remove the extension/helper through documented steps, and GitHub provides checksummed release artifacts.

## Critical path

```text
#9 -> #10 -> #11 -> #12
#11 + #12 -> #13
#11 -> #14 -> #15
#13 + #14 + #15 -> #16 -> #17 -> #18 -> #19/#20 -> #21
#13 + #15 + #17 -> #22/#24/#25
#18 + #19 + #22 -> #23
M2 + M3 -> #26 -> #27 -> #28
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

#32/#33 restored the failed cancellation baseline; public PR and merged-main Windows CI passed with repeated cancellation regressions. #23 implements per-download default-store session handoff, not full session cloning. Its conservative private/container/partition limitations, permission granularity, memory-only recovery behavior, and fresh-task retry are in [AUTHENTICATION.md](AUTHENTICATION.md) and ADR 0010. Wire v2 is unchanged; internal task state is v3. #24/#25 and the security/packaging/real-browser release gates remain required.
