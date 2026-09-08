# Persistent task-state and recovery policy

Status: implemented for issues #15 through #17 and #22–#25

Internal format version: `4`

Last updated: 2026-09-08

## Authority and ownership

The Rust helper's persisted task records, reconciled with their partial files, are authoritative after restart. Extension memory and Native Messaging events are reconstructible views, not recovery truth.

A `TaskStore` owns one user-scoped state root at a time. It accepts only an ordinary non-reparse `.task-store.lock`, confirms that the opened handle still matches that path, and holds an operating-system exclusive lock; a second helper fails with an explicit store-locked error rather than writing concurrently. The lock file may remain after a crash, but the lock itself is released by the operating system when the process exits.

Each task has a random RFC 4122 version-4 UUID rendered as canonical lowercase hyphenated text. The task filename is:

```text
tasks/<task-id>.task.json
```

Task IDs are opaque non-secret identifiers. A persisted task also has a monotonically increasing revision; an older in-memory revision cannot replace a newer durable revision.

## Explicit lifecycle

The persisted state names match protocol v2:

| Current state | Allowed next states |
| --- | --- |
| `queued` | `probing`, `failed`, `cancelled` |
| `probing` | `downloading`, `failed`, `cancelled` |
| `downloading` | `paused`, `validating`, `failed`, `cancelled` |
| `paused` | `downloading`, `failed`, `cancelled` |
| `validating` | `promoting`, `failed`, `cancelled` |
| `promoting` | `completed`, `failed` |
| `failed` | `queued` through explicit retry |
| `completed` | none |
| `cancelled` | none |

A transition also validates its required data and rolls back entirely on failure. For example, `downloading` requires accepted resource identity and a confined partial path; `validating` and `promoting` require exact completed coverage; `completed` requires exact coverage and a published final path. A failed task may retain its partial, but retry must reprobe and apply the same accepted resource identity before those bytes can resume. Timestamps cannot move backwards, and every semantic mutation advances the revision.

## Version 4 record

Every file is one strict UTF-8 JSON object with this conceptual shape:

```json
{
  "format": "firefox-download-manager-task",
  "version": 4,
  "task": {
    "task_id": "7b1c7182-37e9-4a3a-89bd-f9e4e2d6f376",
    "revision": 6,
    "state": "downloading",
    "original_url": "https://downloads.example.test/archive.bin",
    "needs_session": false,
    "expected_sha256": null,
    "final_url": "https://cdn.example.test/archive.bin",
    "expected_size": 10485760,
    "validators": {
      "etag": "\"fixture-v1\"",
      "last_modified": "Thu, 04 Sep 2025 10:00:00 GMT"
    },
    "transfer_mode": "segmented",
    "destination": "C:\\Users\\Example\\Downloads",
    "display_name": "archive.bin",
    "workers": 4,
    "partial_path": "C:\\Users\\Example\\Downloads\\archive.bin.dm-opaque.part",
    "final_path": null,
    "completed_ranges": [{ "start": 0, "end": 5242880 }],
    "created_at_ms": 1788512400000,
    "updated_at_ms": 1788512403000
  }
}
```

The concrete serializer emits compact JSON. All fields shown are required; absent optional data is `null`. Unknown or duplicate fields, unknown enum values, malformed types, noncanonical UUIDs/URLs/paths/ranges, unsupported worker counts, and inconsistent state are rejected. `workers` is exactly 1, 2, 4, or 8. The persisted format version is independent of Native Messaging protocol version 2.

Completed ranges are half-open, ordered, merged, non-overlapping, bounded by the known resource size, and capped at 8,192 entries. Adjacent ranges are noncanonical because storage merges them. Byte totals use checked arithmetic, and resource sizes exposed through protocol-facing snapshots cannot exceed JavaScript's exact-integer bound (`9,007,199,254,740,991`).

An active single-stream record may temporarily have `expected_size: null`, a confined partial path, and no completed ranges. Bytes written before clean EOF are intentionally not resumable metadata. Once the streaming writer validates EOF and flushes data, `refresh_completed` atomically advances the in-memory resource identity to the discovered size and captures exact coverage for the next checkpoint. Validating, promoting, and completing still require a known size and exact coverage.

The original and final exact URLs may include sensitive query values because the same resource may require them for revalidation or retry. URLs are normalized once, restricted to HTTP(S), and reject user-info. Destination, partial, and final paths are absolute, bounded, reject raw Windows device namespaces, and are confined to the destination where applicable.

## Data intentionally not persisted

Version 2 has no fields for:

- cookies or cookie attributes;
- authorization schemes or values;
- arbitrary request headers;
- referrers; or
- response bodies;
- live speed samples, active-worker counters, retry budgets, or event queues; or
- detailed terminal failure data (the `failed` lifecycle state persists, but restart uses a bounded generic recovery error).

These values cannot enter serialization accidentally through a generic header map because no such map exists in the persisted type. Session handoff is implemented in #23; its secrets remain memory-only. Only the non-secret `needs_session` marker is persisted, and a recovered marked task cannot send without its lost context (see [AUTHENTICATION.md](AUTHENTICATION.md)). Exact URLs and local paths are persisted only because recovery needs them, and custom `Debug` implementations redact URLs, destinations, partial paths, final paths, and filenames.

## Bytes-first crash-safe checkpoints

Persistent completed coverage follows this ordering:

1. Storage holds assignment state so no worker can finish concurrently.
2. Storage calls `sync_data` on the partial file.
3. Storage returns the completed-range snapshot covered by that durability point.
4. Task metadata advances its revision with those ranges.
5. The store serializes the complete record into a create-new temporary file in `tasks/`.
6. The temporary file is fully written and `sync_all` succeeds.
7. A same-directory atomic rename replaces the previous record.

A write/flush/replace failure leaves the previous complete record authoritative. A crash before replacement can leave a temporary file, which startup removes only when its name contains a canonical task ID and the exact temporary marker. Unknown files and links are not followed or deleted.

Rust's portable Windows API does not expose destination-directory flushing, so power loss can yield the old or new complete record rather than a promised directory-journal ordering. Recovery validates either record against the partial file and never trusts a torn or mismatched combination.

Final publication follows [integrity validation](INTEGRITY.md), including a streamed optional SHA-256 pass over the owned partial. The non-cloneable validation lease retains helper-write exclusion and its file lock across the promoting checkpoint.

Final publication has a second checkpoint boundary. Storage creates the collision-safe final hard link but retains the `.part` link. The task records both paths in `promoting` state and critically checkpoints them before explicit partial-link cleanup. It then records and checkpoints the cleanup before completion. A crash therefore leaves at least one metadata-identified complete link; recovery never has to guess which pre-existing final filename belongs to the task.

## Bounded write frequency and input

Routine progress checkpoints are coalesced per task. The default minimum interval is one second; configured intervals are restricted to 100 milliseconds through 60 seconds. A dirty progress revision requested before the interval is deferred. Creation, lifecycle changes, pause/shutdown boundaries, and completion use critical checkpoints and bypass progress delay.

Additional recovery bounds are:

- 256 KiB per task-state file;
- 8,192 completed ranges per task;
- 10,000 task records and 20,000 total directory entries per scan;
- 16 KiB per exact URL; and
- Windows path and timestamp bounds before allocation or use.

State errors and load diagnoses contain no JSON text, URL, filename, or path.

## Conservative recovery

Startup acquires the store lock and removes narrowly matched stale temporary files. `load_all` evaluates each final task record independently so one corrupt task cannot hide valid tasks. It checks:

- ordinary non-link/non-reparse state files and bounded reads;
- format marker and version before interpreting the full task;
- strict JSON structure and semantic task invariants;
- filename/task-ID agreement and monotonically valid revisions;
- URL schemes/user-info, validators, timestamps, path confinement, and range arithmetic;
- destination identity and availability;
- partial and published-final file types and exact known lengths; and
- same-file identity when a recoverable publication records both hard links.

Unknown future versions and corrupt records remain untouched for diagnosis or deliberate local cleanup and are returned as safe failure classifications, not resumable tasks. Format v4 has dedicated strict shapes for v1/v2/v3 migration: v1 receives the historical four-worker default; v1/v2 receive `needs_session: false`; v3 retains its required session marker. All three receive `expected_sha256: null` because they never accepted checksums, undergo semantic/filesystem validation, and are atomically rewritten as v4 before being returned. Missing required fields in the current version, or newer fields in older shapes, are malformed rather than guessed. Migration failure excludes only that task and preserves its prior complete record. A missing v3/v4 session marker never authorizes an unauthenticated resume. A missing v4 checksum key never silently disables validation. The v4 parser makes all nullable task/validator keys explicitly required, correcting the older discrepancy between the documented shape and Serde Option omission behavior. Legacy task shapes retain their dedicated migrations.

A missing, truncated, linked, or wrong-length partial excludes active prepublication work from recovery; a `promoting` or `completed` task may instead prove its recorded final file. At task-engine startup, a valid interrupted `downloading` record becomes `paused`; interrupted `probing` or `validating` becomes `failed`; `promoting` becomes `completed` only when its recorded final path already passed recovery validation, otherwise it becomes `failed`. A failed/cancelled record whose partial deletion completed before its metadata checkpoint durably forgets the now-missing path and coverage. Each normalization is a critical checkpoint before the snapshot is exposed.

Before reuse, #22 additionally requires a fresh equal resource identity and a strong ETag for any nonempty completed coverage. Old weak/absent-validator records remain readable but cannot optimistically resume.

A validated known-size task can reopen its partial file with only the durable completed ranges. Active assignments never survive restart. Reopened storage rejects assignments over completed coverage and permits only missing ranges, so uncheckpointed bytes are safely overwritten rather than trusted. An unknown-length single stream reopens with no coverage regardless of the partial's current length; its next bounded streaming writer truncates to zero before receiving a fresh response.

## Completed and abandoned cleanup

There is no automatic age-based deletion in the initial release.

- A completed task with no retained partial can have its history record removed explicitly; its final file is never deleted by task cleanup.
- If publication left a redundant `.part` hard link, `keep` retains both metadata and the partial until explicit cleanup.
- A failed or cancelled task with a partial remains recorded when `keep` is selected.
- Cancellation `delete` validates and removes only the managed partial, then critically retains terminal history without its partial path or completed ranges.
- Explicit history cleanup can subsequently remove eligible terminal metadata; it never removes final output.
- A nonterminal task cannot use terminal cleanup.

If interruption occurs between cancellation's partial deletion and metadata replacement, task-engine startup clears the now-missing partial reference and coverage. If interruption instead occurs during explicit history cleanup, recovery retains terminal metadata and a repeated cleanup can finish safely. Unknown files are never swept as abandoned downloads.

## Task-controller integration

The issue-#17 task controller persists the resolved worker count, serializes control per task, and drives probe, scheduler, checkpoint, validation, promotion, and terminal transitions. Pause and cancellation first signal all asynchronous network waits, then wait for worker joins, then take a critical bytes-first completed-range checkpoint before changing state. Resume reprobes and opens only validated retained storage; an explicit retry of `failed` moves through `queued` and `probing` and cannot reuse a partial after identity change. Unknown-length interruption still restarts from zero.

Routine progress checkpoint requests remain subject to the store's cadence, while pause, cancel, failure, retry, promotion, and completion boundaries are critical. Native Messaging stdin EOF invokes the same cooperative stop path across every managed active run: transfer cancellation/backoff wakes, workers join, eligible partial bytes/ranges receive a critical boundary checkpoint, `downloading` becomes `paused`, and incomplete probe/validation becomes durably `failed`. Promotion is synchronous and is allowed to finish its collision-safe publication. The host exits only after these active ownership boundaries complete; the next process exclusively locks the state root, performs normal recovery, and sends snapshots after protocol negotiation.

Terminal protocol `remove` now calls transactional store cleanup. Keeping a retained partial rejects removal explicitly; `delete_partial: true` validates and removes only that managed partial before deleting history. Completed final output is never removed. Pending in-memory events for successfully removed history are purged, while a later reconnect remains authoritative through its snapshot.

Later work may tune timing within validated bounds, but it may not reverse bytes-first ordering, weaken state/range validation, persist credentials by default, permit stale revisions to overwrite newer state, or treat interrupted unknown-length bytes as resumable.

## Process-kill evidence

`crates/native-host/tests/crash_recovery.rs` launches the actual native-host binary against a loopback 8 MiB fixture in isolated paths containing spaces, waits for persisted completed ranges while a tail is stalled, forcibly kills the process (not cooperative EOF), launches a second helper, asserts paused recovery with retained bytes, resumes with the persisted expected SHA-256 still required, and byte-compares final output. This does not claim a real power-loss or filesystem hardware-fault test.
