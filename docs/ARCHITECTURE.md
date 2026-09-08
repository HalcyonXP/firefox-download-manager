# Architecture

Status: accepted baseline for the initial local release

Last updated: 2026-09-04

## Purpose

This document defines the product boundary and component responsibilities for the Firefox Download Manager. Later changes that alter a boundary or invariant require an architecture decision record (ADR) and, when scope changes, a matching update to `PROJECT_PLAN.md`.

## Product boundary

The initial product is a local download manager for Firefox Developer Edition on Windows 11. A user explicitly selects **Download with Manager** for a direct HTTP or HTTPS URL. The manager may use one validated stream or 1, 2, 4, or 8 validated byte-range workers, with four workers as the default.

The release includes:

- a Firefox WebExtension for user intent, controls, and display;
- a Rust native helper for networking, scheduling, persistence, validation, and direct disk writes;
- a versioned JSON Native Messaging protocol between them;
- local installation and removal tooling for the extension and native host; and
- deterministic local HTTP fixtures for correct and hostile server behavior.

### Non-goals

The initial release does not:

- detect, configure, inspect, start, stop, or route through a VPN;
- rotate addresses or bypass account, subscription, or application access controls;
- support torrents, magnets, FTP, SFTP, media extraction, or streaming-site discovery;
- automatically intercept or cancel every Firefox download;
- provide cloud accounts, synchronization, telemetry, analytics, advertising, or a remote updater;
- execute remote code or load extension code from a CDN;
- provide cross-platform packaging; or
- promise that multiple connections improve every server's throughput.

The helper uses the operating system's ordinary network route.

## Architectural principles

1. Correct bytes are more important than optimistic speed.
2. The native helper is authoritative for task and file state; the UI is reconstructible.
3. Untrusted input is validated at every boundary and again where it is consumed.
4. A malformed ranged response is never merged into output.
5. A safe single-stream fallback or explicit failure is preferable to corruption.
6. Credentials remain narrowly scoped, short-lived, and absent from routine logs and state.
7. The final pathname is not exposed until validation and promotion succeed.
8. Existing final files are never silently overwritten.

## System context

```text
Untrusted web content
        │ URL/referrer selected by explicit user action
        ▼
┌─────────────────────────────────────────────┐
│ Firefox WebExtension (Manifest V3)          │
│ context menu · creation dialog · dashboard  │
│ settings UI · Native Messaging client       │
└───────────────────┬─────────────────────────┘
                    │ framed, versioned JSON
                    │ commands, snapshots, events
┌───────────────────▼─────────────────────────┐
│ Rust Native Messaging helper               │
│ protocol boundary · task state · HTTP       │
│ scheduler · range validation · file I/O     │
└──────────────┬─────────────────┬────────────┘
               │ TLS/HTTP(S)     │ user-scoped filesystem
               ▼                 ▼
       Untrusted servers   state/.part/final files
```

The browser, native message, network, persisted state, settings, and filesystem values are trust boundaries. See [SECURITY.md](SECURITY.md).

## Component responsibilities

### Firefox WebExtension

The extension owns:

- explicit context-menu and toolbar entry points;
- URL-entry confirmation and user-visible filename/destination choices;
- queue, progress, error, settings, and control interfaces;
- collecting only the browser-session material required by an authenticated direct download;
- Native Messaging connection management and protocol negotiation; and
- rebuilding all displayed task state from helper snapshots.

The extension does not download or assemble file bodies, decide completed byte coverage, write destination files, or treat in-memory UI state as authoritative.

The initial extension uses Manifest V3 with a Firefox event-page background script and `nativeMessaging` and `menus` permissions. Its on-demand connection object sends `hello`, validates bounded response/event envelopes, enforces connection-local event sequence continuity, coalesces absolute progress into its latest task map, and atomically replaces that map only after a complete paginated snapshot. A disconnected context retains only a non-authoritative display copy; reconnecting starts a fresh sequence and helper snapshot. See [ADR-0002](decisions/0002-firefox-manifest-v3.md).

### Rust native helper

The helper owns:

- strict message decoding, limits, validation, and stable protocol errors;
- URL validation and HTTP(S) request policy;
- redirects, probing, validators, and resource identity;
- bounded retry, backoff, throttling, and concurrency;
- non-overlapping segment assignments and strict response validation;
- random-access writes to a partial file;
- crash-safe task metadata and recovery;
- final size, coverage, and optional checksum validation;
- collision-safe promotion to the final pathname; and
- bounded, redacted local diagnostics.

The helper runs with the interactive user's privileges and is not a Windows service. Firefox launches it through the per-user native-host registration. Standard output is reserved for complete framed messages. Clean stdin EOF and every session error invoke cooperative engine shutdown: downloading work becomes durably paused after workers join and bytes are checkpointed, while interrupted probing/validation fails safely. A later helper validates persistence and emits a complete snapshot immediately after negotiation. Only one helper can lock the state root and only one writer may own a task at a time. See [ADR-0003](decisions/0003-helper-lifecycle.md).

### Deterministic test server

The local fixture is test-only. It generates reproducible bytes and controlled HTTP failures without internet access. Production packages must not expose or start the fixture.

## Main data flows

### Add a download

1. The user explicitly invokes the manager or enters a URL.
2. The extension rejects unsupported schemes and presents editable, non-authoritative metadata.
3. The extension sends a versioned `add` command with a correlation identifier.
4. The helper validates the complete message, URL, destination policy, settings, and optional sensitive request material.
5. The helper creates a stable task ID and persists initial state before reporting acceptance.
6. A small ranged GET probes the resource. The helper records the final URL, size, validators, and filename metadata under the redirect and logging policies.
7. The helper selects segmented mode only after a valid `206` response proves safe range behavior; otherwise it uses a safe single stream or reports an explicit error.

### Transfer bytes

1. The scheduler subtracts durable coverage and lazily creates large disjoint assignments over only missing bytes.
2. A fixed worker requests exactly its assignment using `Accept-Encoding: identity` and an eligible `If-Range` validator.
3. Before storage ownership, the helper validates status, `Content-Range`, total size, resource validators, encoding, and exact bounded body length.
4. The storage layer independently rejects short, overlapping, duplicate, and out-of-bounds completion claims.
5. The validated assignment is written directly to its offset in the `.part` file; one optional duplicate fetch of the sole slow tail has only one storage winner.
6. Crash-safe metadata records verified completed coverage at a bounded cadence.
7. A safe one-worker fallback uses the same storage/result model; unknown-length interruption restarts from zero.
8. The task controller samples absolute counters at a bounded cadence, publishes coalescible progress, and keeps a complete latest-value snapshot independently.

### Complete a download

1. The helper stops assigning work and verifies exact, gap-free, non-overlapping coverage.
2. It verifies the expected byte count and an optional user-provided SHA-256 digest.
3. It flushes file data and durable coverage metadata as required by the storage policy.
4. It selects a non-colliding final name and atomically publishes the partial's complete bytes where the filesystem permits.
5. It critically checkpoints both hard-link names, removes and checkpoints the redundant partial name, and only then persists and emits `completed`.

### Recover state

Persisted metadata, not the extension, describes recoverable work. On startup, the helper validates schema version, task transitions, paths, resource identity, partial/final lengths, publication same-file identity, completed ranges, and the fixed worker selection. Formats v1/v2/v3 have explicit dedicated migrations to format v4, which requires the session marker and nullable immutable expected-checksum key; unknown future versions are not interpreted. Valid interrupted `downloading` tasks become paused, incomplete probe/validation phases fail safely, and a recorded promoted final link can complete recovery. Corrupt, incompatible, or identity-conflicting state is preserved and diagnosed; it is never resumed optimistically.

## Task lifecycle

```text
queued → probing → downloading ⇄ paused
                    │    │
                    │    ├→ cancelled
                    │    └→ failed → queued (explicit retry)
                    └→ validating → promoting → completed
```

Transitions are explicit and persisted where they affect recovery. `completed` and `cancelled` are terminal; `failed` is inactive until an explicit retry requeues the same task and revalidates any retained resource identity. Protocol v2 uses the `resume` command as that explicit retry action for a failed task. Authenticated tasks that lost context instead require an explicit fresh Add; new credentials never mutate existing retained bytes (see [AUTHENTICATION.md](AUTHENTICATION.md)). Pause/cancel acknowledgement occurs only after cancellation-aware probes, retry sleeps, requests, and workers stop; the controller then performs a bytes-first critical checkpoint. Cancellation has an explicit keep/delete-partial choice and never deletes final output.

## Retry and progress policy

One retry budget spans probing and transfer attempts for a task run. Retryable request failures and HTTP `408`, `425`, `429`, `500`, `502`, `503`, and `504` use bounded exponential equal jitter. Server `Retry-After` is a minimum delay; guidance above the configured safety bound fails rather than retrying early. The default budget is five retries after the initial attempt, protocol settings cannot exceed 20, and explicit user retry starts a fresh budget. Protocol/range identity violations and storage failures are fatal for that run.

Scheduler updates use latest-value channels. The task controller emits absolute progress no more often than its bounded interval (250 ms by default), while per-task subscriptions and list/get snapshots always retain a complete current replacement value. Speed uses a bounded sliding window over monotonic time. ETA is absent for unknown sizes, regressions, stalls, zero rates, and unstable interval rates; exact completion reports zero. Slow consumers may lose intermediate progress but not authority: event-buffer overflow explicitly requires a fresh full snapshot.

## Concurrency and ownership

- Supported per-task worker counts are 1, 2, 4, and 8.
- Four is the default; eight is the initial per-task cap; the resolved per-task choice is persisted for restart.
- Per-host and global limits independently bound aggregate transfer-request pressure; broader adaptive throttling remains issue #24.
- Each active byte belongs to one assignment and one writer.
- Tail assistance may duplicate only the sole remaining bounded request; first validated completion wins one storage assignment and the loser cannot write.
- An exclusive state-store ownership lock prevents two helper instances from managing the same task set and partial files.

## Storage model

Application-owned state and logs live beneath a user-scoped application-data directory. Download bodies live in the user-selected destination as collision-safe partial files. Metadata uses a versioned format and crash-safe replace sequence. It may store the original and final URL because recovery requires them, but never stores cookies or authorization values by default. Sensitive query data is redacted from logs even when it must remain in task state for a signed URL.

Final files are never opened as active download targets. The helper creates a unique partial file, validates it, then promotes it to a non-existing final pathname. Cross-volume or non-atomic behavior must be detected and handled explicitly rather than described as atomic.

See [ADR-0004](decisions/0004-storage-and-recovery.md), the concrete [partial-file storage policy](STORAGE.md), and the [persistent task-state policy](STATE.md).

## Protocol boundary

Native Messaging carries control and state, never file bodies. The host reads partial prefixes/bodies exactly, caps bodies at one MiB before allocation, rejects duplicate JSON members and unknown fields, and distinguishes clean EOF from truncation. Every envelope has a protocol version and correlation identifier. `hello` is mandatory before operations. Commands receive one terminal response; asynchronous events identify their task and use a separate connection-local sequence. Engine retry bookkeeping is filtered without consuming a wire sequence. Event overflow clears the uncertain generation and emits a new authoritative snapshot. Stable machine-readable error codes are separate from localized/display text.

The detailed contract is defined in [PROTOCOL.md](PROTOCOL.md) and must preserve this boundary.

## Decisions and reversibility

| Decision | Record | Reversibility |
| --- | --- | --- |
| WebExtension UI plus Rust-owned engine | [ADR-0001](decisions/0001-component-boundaries.md) | Expensive to reverse after protocol and persistence ship |
| Manifest V3 Firefox event page | [ADR-0002](decisions/0002-firefox-manifest-v3.md) | Moderate; review per Firefox release |
| On-demand user process, no daemon/service | [ADR-0003](decisions/0003-helper-lifecycle.md) | Moderate; daemonization would require a new authenticated local boundary |
| Versioned metadata plus unique partial and promotion | [ADR-0004](decisions/0004-storage-and-recovery.md) | Format is evolvable; final-file safety is not negotiable |
| Probe with ranged GET and validate every range | [ADR-0005](decisions/0005-http-segmentation.md) | Tuning is reversible; response validation is not |
| Original implementation with audited dependencies | [ADR-0006](decisions/0006-third-party-code.md) | Dependencies are replaceable; provenance obligations remain |
| Cooperative task controls, bounded retries, and latest-value progress | [ADR-0007](decisions/0007-task-controls-retries-progress.md) | Timing can be tuned within bounds; safe-stop and boundedness semantics remain |

## Deferred choices

The following remain deliberately reversible and belong to later issues:

- UI framework or framework-free implementation;
- empirically tuned progress cadence within the implemented 100 ms–60 second bound;
- measured tail-hedge and retry-delay tuning within implemented safety bounds;
- release packaging/upgrade technology beyond the current-user registration scripts;
- future cross-platform packaging.

No deferred choice may weaken the correctness and security invariants above.

## Shared request admission (#24)

Probe bytes, redirected probe hops, and transfer workers share one admission domain. It combines configured global/origin caps, retained server cooldown, bounded origin state, and future-width reduction. A dropped response/permit relinquishes local ownership; remote observation may lag as established in #32. Settings reconfiguration preserves outstanding pressure. Worker `416` permits one fresh identity revalidation, not blind retries or body merging. Experimental tail duplication is opt-in and off by default. [RELIABILITY.md](RELIABILITY.md) records exact behavior, measurements, and limits.

### Integrity completion (#25)

[INTEGRITY.md](INTEGRITY.md) describes always-on exact size/coverage checks and optional SHA-256. Hashing streams the owned file through a 256 KiB buffer on a joined blocking task; a non-cloneable validation lease freezes helper writes through create-new promotion. Mismatch uses the explicit failure-retention setting and never emits success. Expected digests survive retry/recovery in internal format v4; the wire stays v2. Validation Cancel is cooperative, and last download throughput is not projected as hashing ETA.

### Pre-packaging security checkpoint (#26)

[SECURITY_REVIEW.md](SECURITY_REVIEW.md) inventories every permission and trust boundary. Selected-site permission construction now rejects wildcard hosts; remote ETags/If-Range are debug-sensitive; explicit CSP, no-private-window behavior and minimum Firefox 156 are guarded. No broader API compatibility or final-browser qualification is inferred from the earlier authentication slice. Development-installer root/ownership/upgrade findings explicitly block #27 delivery until fixed; #28 still qualifies final artifacts.
