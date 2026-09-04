# Project Plan

GitHub execution board: [Firefox Download Manager](https://github.com/users/HalcyonXP/projects/1)

Current state: planning is complete. Issue [#1](https://github.com/HalcyonXP/download-manager/issues/1) is **Ready**; all dependent work remains in **Backlog**.

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

## Proposed architecture

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
│ HTTP client and range validator  │
│ segment scheduler                │
│ random-access partial-file I/O   │
│ integrity and recovery           │
└──────────────────────────────────┘
```

The final architecture choices and trust boundaries are tracked in [#1](https://github.com/HalcyonXP/download-manager/issues/1). The protocol contract is tracked in [#2](https://github.com/HalcyonXP/download-manager/issues/2).

## Delivery milestones

### [M0 — Foundation](https://github.com/HalcyonXP/download-manager/milestone/2)

Establish decisions, project boundaries, protocol design, CI, and deterministic test infrastructure.

- [#1 Record architecture, scope, and security decisions](https://github.com/HalcyonXP/download-manager/issues/1)
- [#2 Define the versioned extension/native-helper protocol](https://github.com/HalcyonXP/download-manager/issues/2)
- [#3 Scaffold the WebExtension, Rust workspace, and CI](https://github.com/HalcyonXP/download-manager/issues/3)
- [#4 Build a deterministic adversarial HTTP test server](https://github.com/HalcyonXP/download-manager/issues/4)

**Exit condition:** both components build in CI, the protocol boundary is documented, and local HTTP fixtures can reproduce correct and incorrect range behavior.

### [M1 — Native download MVP](https://github.com/HalcyonXP/download-manager/milestone/3)

Build the download engine before attaching a browser interface.

- [#5 HTTP probing and strict range-response validation](https://github.com/HalcyonXP/download-manager/issues/5)
- [#6 Safe random-access partial-file storage](https://github.com/HalcyonXP/download-manager/issues/6)
- [#7 Persistent task and segment state](https://github.com/HalcyonXP/download-manager/issues/7)
- [#8 Fixed-concurrency segment scheduler](https://github.com/HalcyonXP/download-manager/issues/8)
- [#9 Pause, resume, cancellation, retries, and progress](https://github.com/HalcyonXP/download-manager/issues/9)

**Exit condition:** the native helper can safely download deterministic fixtures with 1/2/4/8 workers, pause and resume them, and produce byte-identical output.

### [M2 — Firefox integration](https://github.com/HalcyonXP/download-manager/milestone/4)

Connect the engine to an explicit, accessible Firefox workflow.

- [#10 Windows Native Messaging host](https://github.com/HalcyonXP/download-manager/issues/10)
- [#11 Context-menu and creation dialog](https://github.com/HalcyonXP/download-manager/issues/11)
- [#12 Queue and progress dashboard](https://github.com/HalcyonXP/download-manager/issues/12)
- [#13 Local settings and diagnostic logging](https://github.com/HalcyonXP/download-manager/issues/13)

**Exit condition:** Firefox can create and control direct downloads while reconstructing accurate state after its UI closes and reopens.

### [M3 — Reliability and authenticated downloads](https://github.com/HalcyonXP/download-manager/milestone/5)

Protect correctness across restarts, changing resources, authentication, and hostile server behavior.

- [#14 Resource identity and crash recovery](https://github.com/HalcyonXP/download-manager/issues/14)
- [#15 Minimal authenticated-session handoff](https://github.com/HalcyonXP/download-manager/issues/15)
- [#16 Fallback, retry, and throttling hardening](https://github.com/HalcyonXP/download-manager/issues/16)
- [#17 Integrity validation and optional checksums](https://github.com/HalcyonXP/download-manager/issues/17)

**Exit condition:** interruption, resource mutation, malformed range responses, and expired authentication cannot result in a falsely successful or silently corrupted file.

### [M4 — Local release](https://github.com/HalcyonXP/download-manager/milestone/6)

Review, package, document, and qualify the first local release.

- [#18 Permission and native-helper security review](https://github.com/HalcyonXP/download-manager/issues/18)
- [#19 Windows installation and removal](https://github.com/HalcyonXP/download-manager/issues/19)
- [#20 End-to-end qualification and first release](https://github.com/HalcyonXP/download-manager/issues/20)

**Exit condition:** a clean Windows 11 environment can install, use, upgrade, and remove the extension/helper through documented steps, and GitHub provides checksummed release artifacts.

## Critical path

```text
#1 ──► #2 ──► #3
              ├──► #5 ─┐
#4 ───────────┘         │
#3 ──► #6 ──► #7 ──────┼──► #8 ──► #9 ──► #10 ──► #11/#12 ──► #13
                        │
                        └────────────────────► #14/#16/#17
#10 + #11 + #14 ─────────────────────────────► #15
M2 + M3 ──► #18 ──► #19 ──► #20
```

Some work may proceed in parallel, but issue acceptance criteria define completion—not code presence alone.

## Correctness invariants

A release must preserve these invariants:

1. Every written byte belongs to exactly one validated assignment.
2. Completed segment coverage contains no gaps or overlaps.
3. A ranged response is accepted only when its status and `Content-Range` match the request.
4. Resource size and validators remain consistent throughout a segmented download.
5. Resume never combines bytes from resources known to be different.
6. A final file is exposed only after size/integrity checks and successful promotion from partial state.
7. Existing files are never silently overwritten.
8. Credentials never appear in routine logs or persistent state by default.

## Release quality gates

- Formatting, linting, unit tests, and integration tests pass in GitHub Actions.
- Adversarial HTTP fixtures cover malformed and changing responses.
- The final output is byte-identical for all supported worker counts.
- Firefox and helper restart paths are tested.
- Extension permissions are justified and reviewed.
- CPU, memory, disk, and progress-event behavior are measured on a multi-gigabyte fixture.
- Installation, upgrade, and removal are tested from Windows paths containing spaces.
- Known limitations and troubleshooting are documented.

## Planning conventions

- Milestones describe user-visible delivery stages.
- Issues contain testable acceptance criteria and dependency references.
- `priority: critical` identifies milestone exit-path work.
- `priority: high` identifies important work that does not block the earliest vertical slice.
- Area labels identify ownership boundaries without duplicating milestones.
- Scope changes should update this document and the architecture decision record in the same pull request.
