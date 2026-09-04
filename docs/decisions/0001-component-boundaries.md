# ADR-0001: Separate Firefox UI from the Rust download engine

- Status: Accepted
- Date: 2026-09-04
- Reversibility: Low after protocol and persisted state are released

## Context

Firefox WebExtensions provide browser integration but are a poor boundary for multi-gigabyte random-access writes, durable recovery, and long-running segmented networking. Native code can perform those operations but must not receive ambient browser authority or page access.

## Decision

Use two components joined by Firefox Native Messaging:

- The WebExtension captures explicit user intent and owns all browser UI.
- The Rust helper owns HTTP, scheduling, validation, persistence, direct disk I/O, and recovery.
- Native Messaging carries bounded versioned control/state JSON, not downloaded file bodies.
- The helper is authoritative; extension views rebuild from snapshots.

## Rejected alternatives

- **Extension-only Blob/download assembly:** rejected because it duplicates large data in browser memory and cannot provide the required random-access durability.
- **Native UI application without an extension:** rejected because it cannot provide the scoped Firefox context-menu/session workflow cleanly.
- **Local HTTP control API:** rejected because it creates a listening network endpoint, origin/authentication problems, and unnecessary attack surface.
- **Browser download interception:** rejected for the first release because explicit activation is clearer and avoids surprising cancellation of Firefox downloads.

## Consequences

The protocol must be specified before the components couple. Installation must register a least-privilege native host. UI lifecycle loss is normal, so no correctness decision can depend on extension memory.
