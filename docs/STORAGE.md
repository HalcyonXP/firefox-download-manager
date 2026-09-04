# Partial-file storage policy

Status: implemented baseline for issue #6

Last updated: 2026-09-04

## Safety model

The native engine writes response bodies only to a unique file ending in `.part` in the selected destination directory. The storage layer never opens a final pathname as a download target and never accepts an arbitrary path from a segment worker.

Each worker receives one non-empty, half-open assignment (`start..end`). Active and completed assignments must be disjoint and within the expected file length. A writer advances sequentially from its assignment's start and rejects a chunk before I/O if that chunk would cross the exclusive end. An assignment becomes completed coverage only after every assigned byte was written. Dropping an unfinished writer releases ownership without claiming its partial bytes, allowing a later retry to overwrite the whole assignment safely.

Completed coverage is maintained as ordered, merged, non-overlapping ranges. Publication requires no active writers and exact `0..expected_length` coverage (or no ranges for an empty file). The on-disk file length is independently rechecked before publication.

## Creation and preallocation

`PartialFile::create`:

1. requires an existing ordinary destination directory and rejects a final-component symbolic link or Windows reparse point;
2. canonicalizes the directory once for this storage object;
3. sanitizes untrusted filename metadata into one bounded component;
4. chooses a process/time/counter-based `.part` candidate; and
5. opens it with create-new semantics, sets its logical length to the expected resource size, and flushes creation metadata before returning it.

`File::set_len` establishes the complete logical extent up front and lets the filesystem decide physical allocation. A filesystem that rejects this operation produces an explicit preallocation error; the engine does not silently switch to a differently sized file.

Partial-name uniqueness does not depend on checking and then opening. Every candidate is opened atomically with create-new semantics, so a collision is retried without replacing anything.

## Windows filename rules

The sanitizer replaces controls and these Win32-invalid or path-significant characters with `_`:

```text
< > : " / \ | ? *
```

It also:

- removes trailing dots and spaces;
- converts empty, `.`-only, and `..`-only results to `download`;
- prefixes reserved device stems such as `CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, and `LPT1`–`LPT9`, including documented superscript aliases;
- neutralizes drive letters, UNC/device prefixes, alternate data streams, and traversal separators by treating all input as one component; and
- limits the result to 180 UTF-16 code units without splitting a Unicode scalar value.

Sanitization does not imply that server metadata is trustworthy; the UI may still let the user edit the proposed name before task creation.

## Random-access writes

The public API is assignment-oriented:

- `assign(range)` rejects overlap with active or completed ownership;
- `SegmentWriter::write(bytes)` seeks to the assignment's current offset while holding the file lock and writes the entire chunk or returns a classified failure;
- `finish()` rejects short assignments and is the only operation that adds completed coverage; and
- dropping an unfinished writer removes only its ownership record, not bytes or durable completed state.

Separate segment writers are thread-safe. File seek/write operations are serialized internally so the portable implementation cannot race through a shared file cursor; network workers can still fetch concurrently. This can later be replaced by platform positional-I/O calls without changing assignment semantics.

## Error classification

Filesystem errors are converted immediately to bounded, path-free categories. Routine display text never embeds the destination or partial path. The storage API distinguishes:

- disk full or quota exhausted;
- access denied;
- Windows sharing/lock violations;
- existing-name collisions;
- missing paths;
- unsupported filesystem operations; and
- other I/O failures.

The failed operation and optional numeric OS code remain available for structured local diagnosis. Range overlap, out-of-bounds chunks, short assignments, incomplete coverage, changed file length, and active-writer publication are separate non-I/O errors.

## Collision-safe atomic publication

After exact coverage and file-length checks, the engine flushes all file data with `sync_all`. It then publishes a final name by creating a hard link from the complete `.part` file to a non-existing candidate in the same directory. Hard-link creation is one create-new filesystem operation: it cannot overwrite an existing entry and makes the complete bytes visible under the final name atomically.

Candidates are tried as:

```text
report.txt
report (1).txt
report (2).txt
...
```

The extension is preserved where practical and each candidate remains within the filename bound. An existing file or directory is left untouched even if it appears during publication.

After successful publication, the writable handle is closed and the `.part` name is removed. Failure to remove that now-redundant hard link is returned as a non-fatal cleanup classification because the complete final name is already visible and rolling it back would be unsafe. Recovery can remove the redundant partial name later.

Atomic namespace visibility and power-loss durability are distinct. Rust's portable file API does not provide a Windows destination-directory flush; restart recovery therefore must reconcile the safe outcomes around publication (only the partial name, only the final name, or both hard links) rather than assuming an acknowledgement survived sudden power loss.

Filesystems that do not support same-directory hard links fail publication explicitly. The implementation does not fall back to a copy, an overwrite-capable rename, or a check-then-rename sequence that could expose partial content or race an existing final file.

## Deferred recovery work

Issue #7 adds versioned task metadata, restart recovery, and cleanup policy. This baseline deliberately keeps partial files after ordinary object drop and exposes the partial path only for trusted persistence code. Issue #17 will extend adversarial filesystem tests and path-identity hardening; those later changes may strengthen checks but may not weaken assignment bounds, exact coverage, or create-new final publication.
