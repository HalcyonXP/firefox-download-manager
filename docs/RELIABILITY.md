# Polite admission, retry, and fallback (#24)

## Goal and established gap

Transfer workers already validated assignments and used concurrency semaphores, but probes and redirected probe hops were outside those caps. Task-local backoff also did not suppress peer tasks to the same origin. Shared `Admission` now covers **every** probe/transfer request and retains ownership until the response body is consumed or dropped. Cookie eligibility is checked after admission waits, immediately before send.

**Admission caps** mean locally admitted HTTP requests by exact scheme/host/port origin and globally across the helper. This preserves the existing code/docs model, not a new promise about OS TIME_WAIT entries, idle sockets, or one TCP socket per HTTP/2 stream. Remote handlers may finish bookkeeping after local cancellation; the #32 local-ownership boundary still applies.

## Server pressure

- A `429` or `503` halves the origin's future admission width down to one and establishes a shared not-before deadline. Requests already admitted may finish; no new origin admission is granted while the cooldown applies or its reduced width is full.
- Valid `Retry-After` seconds/HTTP dates remain minimum delays; fractional remaining HTTP-date seconds round **up** so retries cannot precede the date. Missing/invalid guidance uses a one-second shared cooldown plus task backoff. Explicit zero permits immediate admission subject to the reduced width and task backoff.
- Guidance above one hour blocks that origin for the current admission-domain lifetime instead of retrying early. The task's retry policy may reject smaller excessive waits (five minutes by default); peers still retain known guidance.
- Origin state is capped at 1,024 entries. Penalties remain for at least 60 seconds after their cooldown and while referenced; unpenalized idle entries can be reclaimed. Protected/blocked entries are not evicted to bypass guidance. Exhaustion fails safely.
- Unrelated origins can proceed without a cooled origin reserving a global slot. Pause/cancel drops pending admission futures and releases owned request slots.
- Inactive settings reconfiguration copies outstanding pressure into the new limiter; changing a destination or cap does not accidentally erase known server guidance. Cooldowns are memory-only, not a durable global rate-limit service across helper restarts.

The configured per-task worker selection remains 1/2/4/8 (default four), persisted and displayed as before. Each transient **transfer** retry reduces its effective width `8 → 4 → 2 → 1`, without mutating that selection or increasing the existing shared retry budget. Engine-local `RetryScheduled.next_workers` records the effective next width; initial-probe retries have no width. This is not a new wire-v2 field. Task backoff delay is a minimum; shared admission can add waiting.

## Range rejection and identity

A worker `416` causes at most one fresh probe revalidation per run. Its body is never consumed as file data. Original/final URL, size, validators, and mode must still match the accepted resource identity. If unchanged, a reduced-width transfer retry may consume the ordinary shared retry budget. Changed identity fails immediately; a second worker `416` fails rather than looping. A failing initial probe already is revalidation and is never blindly retried merely because its status is `416`.

Malformed ranges, ignored worker ranges (`200` after segmented probing), conflicting validators, and encoded bodies remain fatal before storage ownership. No retained segmented output is relabeled as a successful single stream. Safe single fallback remains a fresh initial-probe decision. Existing exact coverage, byte count, missing-only resume, no-overwrite promotion, and strong-ETag requirements are unchanged.

## Optional tail duplication

Production defaults no longer duplicate tail work. `SchedulerOptions::with_tail_hedging(true)` is an explicit experimental engine opt-in, not a new extension setting. The ordinary shared missing-work queue remains enabled. A **tail hedge** is one duplicate of the sole slow assignment, not arbitrary worker restart or permission to overlap committed ranges. Tests retain winner-only validated ownership and cancellation of the losing request.

### Bounded local measurement — Windows 11, 2026-09-08

Run:

```powershell
cargo test -p download-manager-engine --test scheduler measure_optional --locked -- --ignored --nocapture
```

An 8 MiB loopback fixture, four workers, one first-attempt tail stalled for 600 ms, and a 50 ms hedge threshold produced:

| Policy | Transfer milliseconds, five trials | Transfer requests | Duplicate requests |
| --- | --- | --- | --- |
| Default, off | 614, 614, 614, 614, 614 | 8 per trial | 0 |
| Explicit hedge | 68, 70, 72, 65, 70 | 9 per trial | 1 |

Every promoted output matched exact fixture bytes. This measures transfer wall time/request cost on that deliberately favorable fixture, **not** CPU, memory, general internet throughput, TLS performance, or a justification for enabling a restart/duplication policy by default. The measurement is opt-in rather than a timing-dependent ordinary CI gate; the separate correctness regression runs in normal CI. Multi-gigabyte release performance remains #28.

## Regression evidence

Tests cover shared probe/redirect admission, independent origin/global limits, cancelled waiters, peer cooldown and halving, excessive guidance, bounded origin retention, settings-pressure preservation, effective retry widths under one budget, unchanged/changed/repeated worker `416`, all original hostile range/storage tests, and the deterministic #32 cancellation pair. Actual remote CI is required before merge; code presence and local timings do not qualify an installable release.
