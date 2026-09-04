# ADR-0007: Use cooperative task controls, bounded retries, and latest-value progress

- Status: Accepted
- Date: 2026-09-04
- Reversibility: Timing constants are tunable within bounds; safe-stop ordering, bounded retries, and snapshot authority are not

## Context

The Firefox UI needs pause, resume, cancellation, speed, and ETA without becoming authoritative for network or disk state; the engine also needs observable retry scheduling for deterministic policy tests and later adapters. A command can race probing, semaphore acquisition, response streaming, a tail hedge, storage completion, or retry backoff. Unbounded retries can amplify server/load failures, while unbounded progress queues can exhaust helper memory. Conversely, dropping deltas must not leave the UI unable to reconstruct a task.

## Decision

Give each task run one cooperative cancellation signal shared by probing, retry sleeps, scheduler waits, requests, response reads, and range workers. A pause or cancellation response is sent only after workers join (or finish an already-entered synchronous storage commit), storage flushes completed bytes, metadata critically checkpoints those ranges, and the requested state/policy is durable. Cancellation requires an explicit `keep` or `delete` partial policy; deletion can target only the validated managed partial and never final output.

Resume reprobes resource identity and reopens only durable coverage. Protocol v1's `resume` command is also the explicit retry action for `failed`: it persists `queued` and `probing`, starts a fresh bounded budget, and rejects retained bytes when final URL, size, validators, or transfer mode changed.

Use one retry budget across probe and transfer attempts. Retry transport failures and selected transient HTTP statuses (`408`, `425`, `429`, `500`, `502`, `503`, and `504`) with capped exponential equal jitter. Treat valid `Retry-After` as a minimum and fail rather than retry early when it exceeds the accepted bound. Never automatically retry protocol identity/range violations or storage failures. The default budget is five retries after the initial attempt and protocol-aligned configuration is capped at 20. Record retry scheduling as best-effort typed engine-local bookkeeping; protocol v1 has no matching event fields, so its connection adapter consumes this internally without inventing a wire shape or advancing the wire sequence.

Publish scheduler counters through a `watch` latest-value channel. Maintain a complete per-task snapshot separately from a bounded event queue. Emit absolute progress at a validated cadence (250 ms by default), coalesce unsent progress per task, and mark event continuity uncertain if critical queue capacity is exhausted so the connection layer reconstructs from snapshots. Compute speed over bounded monotonic samples and expose ETA only for known-size, positive, stable progress; suppress it for unknown, regressed, stalled, or unstable samples.

## Rejected alternatives

- **Abort worker tasks immediately:** cancellation during storage commit could acknowledge before ownership and durability settle.
- **Pause by retaining arbitrary in-flight body bytes:** those bytes have not completed an assignment and are not safe resume coverage.
- **Retry every error indefinitely:** this can hammer servers, hide fatal corruption/storage conditions, and prevent user control.
- **Ignore or cap `Retry-After` downward:** retrying earlier than server guidance violates the policy; excessive guidance instead ends automatic retry.
- **Send every byte delta through Native Messaging:** slow/disconnected consumers would create unbounded memory pressure and fragile UI state.
- **Derive ETA from one instantaneous sample:** stalls and bursty segmented commits would present misleading precision.

## Consequences

Commands are serialized by per-task state and may complete after a short cooperative-stop delay. Known/unknown single streams can visibly regress to durable zero on interruption because incomplete stream bytes are never represented as resumable coverage. Intermediate progress can be skipped safely because every sample is absolute and complete snapshots remain authoritative. Protocol v1 surfaces terminal retry exhaustion but not the richer engine-local retry number and actual-delay bookkeeping. Retry and event timing can be tuned after measurement only within the documented resource bounds and without changing fatal/transient classification silently.
