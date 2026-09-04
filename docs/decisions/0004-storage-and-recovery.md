# ADR-0004: Use unique partial files and versioned recoverable metadata

- Status: Accepted
- Date: 2026-09-04
- Reversibility: The encoding is reversible; safety semantics are not

## Context

Concurrent workers must write large files without browser memory assembly. Crashes can occur between body writes and metadata updates. User destinations may contain existing files, hostile names, links, sharing locks, or filesystems with different rename guarantees.

## Decision

Create a unique `.part` file with create-new semantics in the selected destination and preallocate it for a known size. Every ranged write carries an assignment and is bounds-checked before random-access I/O. An undeclared-length fallback instead grants one bounded sequential writer; only clean EOF seals its actual size and coverage, and interruption requires truncation and restart from zero. Keep versioned task metadata in the user-scoped application state directory and update it through write/flush/replace at a bounded cadence.

On recovery, distrust both metadata and the partial file and validate their agreement. On completion, independently verify coverage and size, flush, select a non-existing final pathname, and use an atomic same-filesystem create-new publication primitive. The initial Windows implementation creates a hard link for the complete partial file, checkpoints both names, then removes and checkpoints the redundant partial name; a filesystem without that primitive fails explicitly. Never overwrite an existing final file and never mark success before promotion.

## Rejected alternatives

- **One temporary file per segment plus concatenation:** doubles I/O and creates a separate error-prone merge step.
- **Write directly to the final name:** exposes incomplete content and collision risk.
- **In-memory progress only:** cannot recover after process failure.
- **Database as an initial requirement:** adds migration and locking complexity before the state volume warrants it; the concrete encoding remains deferred.

## Consequences

The storage API must model assignment ownership, durability points, and promotion explicitly. Metadata schema versions and migrations are mandatory. Cleanup of completed, cancelled, corrupt, and abandoned state must be a deliberate policy. The implemented format, bytes-first checkpoint ordering, conservative loader, and explicit cleanup baseline are documented in [STATE.md](../STATE.md).
