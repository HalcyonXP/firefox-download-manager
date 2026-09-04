# Persistent task-state and recovery policy

Status: implemented baseline for issue #7

Internal format version: `1`

Last updated: 2026-09-04

## Authority and ownership

The Rust helper's persisted task records, reconciled with their partial files, are authoritative after restart. Extension memory and Native Messaging events are reconstructible views, not recovery truth.

A `TaskStore` owns one user-scoped state root at a time. It accepts only an ordinary non-reparse `.task-store.lock`, confirms that the opened handle still matches that path, and holds an operating-system exclusive lock; a second helper fails with an explicit store-locked error rather than writing concurrently. The lock file may remain after a crash, but the lock itself is released by the operating system when the process exits.

Each task has a random RFC 4122 version-4 UUID rendered as canonical lowercase hyphenated text. The task filename is:

```text
tasks/<task-id>.task.json
```

Task IDs are opaque non-secret identifiers. A persisted task also has a monotonically increasing revision; an older in-memory revision cannot replace a newer durable revision.

## Explicit lifecycle

The persisted state names match protocol v1:

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

## Version 1 record

Every file is one strict UTF-8 JSON object with this conceptual shape:

```json
{
  "format": "firefox-download-manager-task",
  "version": 1,
  "task": {
    "task_id": "7b1c7182-37e9-4a3a-89bd-f9e4e2d6f376",
    "revision": 6,
    "state": "downloading",
    "original_url": "https://downloads.example.test/archive.bin",
    "final_url": "https://cdn.example.test/archive.bin",
    "expected_size": 10485760,
    "validators": {
      "etag": "\"fixture-v1\"",
      "last_modified": "Thu, 04 Sep 2025 10:00:00 GMT"
    },
    "transfer_mode": "segmented",
    "destination": "C:\\Users\\Example\\Downloads",
    "display_name": "archive.bin",
    "partial_path": "C:\\Users\\Example\\Downloads\\archive.bin.dm-opaque.part",
    "final_path": null,
    "completed_ranges": [{ "start": 0, "end": 5242880 }],
    "created_at_ms": 1788512400000,
    "updated_at_ms": 1788512403000
  }
}
```

The concrete serializer emits compact JSON. All fields shown are required; absent optional data is `null`. Unknown or duplicate fields, unknown enum values, malformed types, noncanonical UUIDs/URLs/paths/ranges, and inconsistent state are rejected. The persisted format version is independent of Native Messaging protocol version 1.

Completed ranges are half-open, ordered, merged, non-overlapping, bounded by the known resource size, and capped at 8,192 entries. Adjacent ranges are noncanonical because storage merges them. Byte totals use checked arithmetic.

The original and final exact URLs may include sensitive query values because the same resource may require them for revalidation or retry. URLs are normalized once, restricted to HTTP(S), and reject user-info. Destination, partial, and final paths are absolute, bounded, reject raw Windows device namespaces, and are confined to the destination where applicable.

## Data intentionally not persisted

Version 1 has no fields for:

- cookies or cookie attributes;
- authorization schemes or values;
- arbitrary request headers;
- referrers; or
- response bodies.

These values cannot enter serialization accidentally through a generic header map because no such map exists in the persisted type. Authentication remains deferred to issue #15 and credentials stay memory-only by default. Exact URLs and local paths are persisted only because recovery needs them, and custom `Debug` implementations redact URLs, destinations, partial paths, final paths, and filenames.

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

Unknown future versions and corrupt records remain untouched for diagnosis or deliberate local cleanup and are returned as safe failure classifications, not resumable tasks. Version 1 has no predecessor to migrate; adding a later format requires an explicit tested migration rather than interpreting future fields as version 1. A missing, truncated, linked, or wrong-length partial excludes active prepublication work from recovery; a `promoting` or `completed` task may instead prove its recorded final file.

A validated task can reopen its partial file with only the durable completed ranges. Active assignments never survive restart. Reopened storage rejects assignments over completed coverage and permits only missing ranges, so uncheckpointed bytes are safely overwritten rather than trusted.

## Completed and abandoned cleanup

There is no automatic age-based deletion in the initial release.

- A completed task with no retained partial can have its history record removed explicitly; its final file is never deleted by task cleanup.
- If publication left a redundant `.part` hard link, `keep` retains both metadata and the partial until explicit cleanup.
- A failed or cancelled task with a partial remains recorded when `keep` is selected.
- Explicit `delete` validates that the partial is an ordinary confined `.part` file, removes it, and only then removes metadata.
- A nonterminal task cannot use terminal cleanup.

If a crash occurs between partial deletion and metadata deletion, recovery retains terminal metadata with its now-missing partial and a repeated explicit cleanup can finish safely. Unknown files are never swept as abandoned downloads.

## Deferred integration

Issues #8 and #9 connect recovered records to scheduling, pause/resume, retry, and progress events. A loaded `downloading` value describes the last durable phase, not a claim that a worker survived process exit; lifecycle integration must pause or safely reconstruct work before emitting a live snapshot. The issue #7 baseline can persist a probed unknown-length single-stream identity, but it deliberately refuses to claim a partial or completed coverage without an unknown-length storage implementation; issue #8 must add that storage/progress behavior or restart such a stream from byte zero rather than inventing a size. Later work may tune when routine checkpoint requests occur, but it may not reverse bytes-first ordering, weaken state/range validation, persist credentials by default, or permit stale revisions to overwrite newer state.
