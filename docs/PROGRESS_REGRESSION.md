# Progress-event observation regression (#39)

## Objective and baseline

Restore a trustworthy main baseline before #28 qualification. This is test-boundary work, not a new user preference or a change to progress behavior.

Merged-main CI [34277988932](https://github.com/HalcyonXP/firefox-download-manager/actions/runs/34277988932), source `9928058dc77652561727aceb646b0e6a6ec2da18`, failed the LLVM/MinGW x64 release-target lifecycle test `progress_events_are_rate_limited_while_snapshots_remain_complete`: the consumer observed fewer than four progress events. The other 29 lifecycle tests passed; the dependent Windows 11 ARM64 lifecycle job was skipped. Both preceding #27 PR CI runs passed. This failure is retained, not dismissed as the earlier artifact-finalization 403 or retried until green.

## Established contract versus unresolved timing

- `ProgressPolicy::event_interval()` is a **minimum spacing**, not a promise to deliver every timer tick or four samples during a particular transfer.
- `run_transfer_attempt()` skips missed ticks. `emit_progress()` throttles ordinary samples using monotonic elapsed time.
- `EventBuffer::emit()` replaces an unread ordinary progress sample for the same task with the latest one. Consumer scheduling therefore affects the observed count independently of producer scheduling.
- Ordinary events may predate completion, including a zero-byte event on a fast transfer. The authoritative terminal snapshot carries joined final metrics. A terminal snapshot is not a promise of a separate final ordinary progress event.

The old test's four 130-ms assignment stalls did not establish four consumer observations. The CI log contains the failing count predicate, not a scheduler trace, so the precise mix of missed ticks, I/O delay and consumer coalescing on that run is unknown. The invalid minimum-count premise has a deterministic counterexample below. No transfer/output defect was identified in this scoped investigation; product behavior was not changed to satisfy the test.

## Replacement evidence

1. **Selected response barrier:** `TestServer::pause_responses(selector)` holds a matching request after ledger insertion and before response headers/body. It is distinct from #32's pre-ledger observation barrier. Unmatched requests proceed; overlapping pauses are refused; dropping the guard or server releases waiters. A real HTTP regression checks these boundaries and exact bytes.
2. **Observed cadence, then release:** the lifecycle test withholds the second 2-MiB assignment of an 8-MiB transfer. It consumes at least four samples at the retained 2-MiB prefix and observes a speed-estimator window. Only after the selected request is confirmed at the barrier does it release the response. Existing spacing, monotonicity, event-count upper bound and complete snapshot assertions remain; final output is additionally checked byte-for-byte. A bounded deadline detects loss of progress rather than guessing readiness from a fixed sleep.
3. **Late consumer:** a second actual network/disk test consumes no events until the task is inactive. At most one ordinary progress sample remains, without overflow, while the terminal event/snapshot and all 8-MiB output bytes remain complete. This deliberately reproduces the invalid minimum-count premise without relying on a busy runner.
4. **Keyed buffer unit regression:** eight synthetic samples for two task IDs leave exactly the latest sample for each ID. This proves coalescing independently of producer timing; it is not substituted for the real integration cases.

An initial late-consumer assertion incorrectly expected the last ordinary event to contain the final byte count. The real test returned zero ordinary bytes alongside a complete 8-MiB terminal snapshot. Inspection confirmed the distinction above; the test now checks ordinary bounds and exact terminal state separately. No completion behavior was weakened or added.

## Validation checkpoint

Twenty consecutive local MSVC debug runs of the two `progress_events_` integration cases passed. The keyed buffer and selected-response HTTP tests, full workspace formatting/Clippy/tests/build, npm checks/audit/license review, Cargo dependency policy and four pure packaging tests passed. LLVM release-target gates and final-tip public CI are still pending at this implementation checkpoint; subsequent revision-specific results belong in [issue #39](https://github.com/HalcyonXP/firefox-download-manager/issues/39) and its linked PR.

CI now repeats the two integration cases ten times using the actual LLVM/MinGW release-target lifecycle executable produced by the preceding clean build, before the second clean build removes test outputs. Existing cancellation repetitions and all workspace gates remain enabled. “Deterministic” describes the controlled causal boundary, not guaranteed execution under arbitrary scheduler starvation or a timing benchmark.

#28 is paused in Backlog behind #39. Its separate draft harness is preserved on `issue-28-release-qualification`; it is not mixed into this fix or represented as passing qualification. A passing #39 baseline will permit qualification work to resume, not approve a release or resolve native Windows 11 x64 Firefox coverage.
