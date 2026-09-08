# Retry cancellation boundaries (#41)

## Failure and objective

PR #40 CI [34283625146](https://github.com/HalcyonXP/firefox-download-manager/actions/runs/34283625146) at `9c9b81d24c21027baf41d37aa0ad5537f4feabf8` passed both #39 progress cases but failed `cancel_interrupts_probe_retry_sleep` in the LLVM/MinGW release-target lifecycle suite. Its one-second timeout expired; the other 30 cases passed. The run remains failed. The precise scheduler, wakeup and filesystem contribution is not available from that log; slow I/O/runner contention is possible, not established as its cause.

#41 is a separate correction from main `9928058`, not folded into the #39 PR. Its goal is to distinguish retry-wakeup correctness from durable control acknowledgement before either that baseline or #28 qualification is approved.

## Contract and chosen test boundary

`wait_retry` must become ready for a cancellation signal rather than wait for the retry timer. The existing architecture also requires a safe ownership stop and critical checkpoint before a control acknowledgement. Individual blocking filesystem operations are not forcibly interruptible. Neither the issue acceptance nor that architecture establishes a one-second durable-ack SLO.

The old integration timeout mixed these boundaries. This correction does **not** merely increase it until CI passes:

- The existing `tokio::select!` is factored into private `wait_retry_until(timer, cancellation)`. Production still supplies the same Tokio sleep and retains the same branch results, including cancellation winning semantically when both branches are ready. No new dependency, timer policy or public API is introduced.
- A manually polled unit test supplies a **never-ready timer**. It requires `Pending` before cancellation and `Ready(false)` on the next poll after cancellation, and covers a signal supplied before the first poll. A separately ready timer permits retry only without cancellation. No scheduler delay, filesystem performance, actual sleep or virtual-time auto-advance can explain these results.
- Real probe-cancel, transfer-pause/resume and probe-shutdown tests retain their network request counts, lifecycle and recovery assertions. They now read the acknowledged critical checkpoint back from disk as well as checking the in-memory snapshot.
- Their 15-second test deadline is bounded deadlock containment, **not** a new product latency guarantee or the proof of retry interruption. In particular, it can exceed the fixture's retry delay; the never-ready-timer assertion now proves the interruption independently.
- CI repeats the actual release-target probe-cancellation integration ten times. Worker counts, test-case parallelism and other gates are unchanged.

## Mutation and local evidence

Temporarily removing the cancellation branch made the new unit test fail with `Pending` instead of `Ready(false)`. Temporarily bypassing the stop path's critical checkpoint made the real probe-cancel test fail with on-disk `probing` rather than `cancelled`. Both source mutations were restored byte-for-byte before normal tests resumed. An initial mutation harness failed its text-match preflight on CRLF input, before changing source; normalized matching corrected the harness, not the product.

The normal readiness unit, all three control integrations, twenty consecutive local debug probe-cancellation repetitions, full workspace formatting/Clippy/tests/build, npm checks/audit/license gate, Cargo deny and pure packaging-policy tests passed at the initial implementation checkpoint. Local release-target tests and final-tip public CI remain pending; later revision-specific records belong in [issue #41](https://github.com/HalcyonXP/firefox-download-manager/issues/41) and its linked PR.

This is not native Windows 11 Firefox qualification, a disk-latency benchmark, or release approval. #28 remains blocked. After this focused correction is verified, #39 / PR #40 must be updated to the resulting main and pass its own complete gate before qualification resumes.
