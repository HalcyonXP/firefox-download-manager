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

The initial extension uses Manifest V3 with a Firefox event-page background script. It must tolerate that background context, dashboard pages, and Native Messaging connections can disappear and be recreated. See [ADR-0002](decisions/0002-firefox-manifest-v3.md).

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

The helper runs with the interactive user's privileges and is not a Windows service. A browser/helper disconnect causes network work to stop at a safe checkpoint; persisted state permits recovery after reconnection or restart. Only one writer may own a task at a time. See [ADR-0003](decisions/0003-helper-lifecycle.md).

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

1. The scheduler creates disjoint, bounded assignments covering the expected resource.
2. A worker requests exactly its assignment using `Accept-Encoding: identity`.
3. Before writing, the helper validates status, `Content-Range`, total size, resource validators, and body bounds.
4. The storage layer rejects short, overlapping, duplicate, and out-of-bounds completion claims.
5. Workers write directly to assigned offsets in the `.part` file.
6. Crash-safe metadata records verified completed coverage at a bounded cadence.
7. Progress events are rate-limited; a complete snapshot remains available.

### Complete a download

1. The helper stops assigning work and verifies exact, gap-free, non-overlapping coverage.
2. It verifies the expected byte count and an optional user-provided SHA-256 digest.
3. It flushes file data and durable coverage metadata as required by the storage policy.
4. It selects a non-colliding final name and atomically publishes the partial's complete bytes where the filesystem permits.
5. It critically checkpoints both hard-link names, removes and checkpoints the redundant partial name, and only then persists and emits `completed`.

### Recover state

Persisted metadata, not the extension, describes recoverable work. On startup, the helper validates schema version, task transitions, paths, resource identity, partial/final lengths, publication same-file identity, and completed ranges. Corrupt, incompatible, or identity-conflicting state is preserved and failed with a useful diagnosis; it is never resumed optimistically.

## Task lifecycle

```text
queued → probing → downloading ⇄ paused
                    │    │
                    │    ├→ cancelled
                    │    └→ failed → queued (explicit retry)
                    └→ validating → promoting → completed
```

Transitions are explicit and persisted where they affect recovery. `completed` and `cancelled` are terminal; `failed` is inactive until an explicit retry requeues the same task and revalidates any retained resource identity. Pause/cancel acknowledgement occurs only after active workers stop making writes. Cancellation has an explicit keep/delete-partial choice.

## Concurrency and ownership

- Supported per-task worker counts are 1, 2, 4, and 8.
- Four is the default; eight is the initial per-task cap.
- Per-host and global limits independently bound aggregate pressure.
- Each active byte belongs to one assignment and one writer.
- Tail splitting may only create new disjoint assignments from bytes not yet written.
- A task-level ownership guard prevents two helper instances from writing the same partial file.

## Storage model

Application-owned state and logs live beneath a user-scoped application-data directory. Download bodies live in the user-selected destination as collision-safe partial files. Metadata uses a versioned format and crash-safe replace sequence. It may store the original and final URL because recovery requires them, but never stores cookies or authorization values by default. Sensitive query data is redacted from logs even when it must remain in task state for a signed URL.

Final files are never opened as active download targets. The helper creates a unique partial file, validates it, then promotes it to a non-existing final pathname. Cross-volume or non-atomic behavior must be detected and handled explicitly rather than described as atomic.

See [ADR-0004](decisions/0004-storage-and-recovery.md), the concrete [partial-file storage policy](STORAGE.md), and the [persistent task-state policy](STATE.md).

## Protocol boundary

Native Messaging carries control and state, never file bodies. Every envelope has a protocol version and correlation identifier. Commands receive one terminal response; asynchronous events identify their task. Unknown versions, commands, fields where forbidden, oversized frames, malformed JSON, and invalid state transitions fail safely. Stable machine-readable error codes are separate from localized/display text.

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

## Deferred choices

The following remain deliberately reversible and belong to later issues:

- concrete Rust HTTP/runtime, persistence serialization, and JavaScript build dependencies;
- UI framework or framework-free implementation;
- exact progress-event cadence and metadata checkpoint interval;
- measured tail-splitting and retry tuning;
- installer technology;
- optional checksum UX; and
- future cross-platform packaging.

No deferred choice may weaken the correctness and security invariants above.
