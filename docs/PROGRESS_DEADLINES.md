# Progress deadline investigation (#44)

## Goal and established facts

Restore a trustworthy release-target gate before #28. This independent correction starts from main `01a49d0`; `crates/engine/src` and `crates/engine/tests` are identical to the failing qualification tip `26e302b`. The latter adds MIT licensing, packaging notices and the owner's existing-machine qualification decision. Those decisions remain in PR #43 and are not revoked here.

[CI 34321346203](https://github.com/HalcyonXP/firefox-download-manager/actions/runs/34321346203) failed the LLVM/MinGW release-target suite:

- `progress_events_are_rate_limited_while_snapshots_remain_complete`: `event completion timeout: Elapsed(())` at former line 777.
- `progress_events_coalesce_for_a_late_consumer_without_losing_final_state`: `task timeout: Elapsed(())` at former line 843.
- Other 29 lifecycle cases passed; total 51.37 s. The preceding passing CI `34298068858` took 57.07 s for this suite, so aggregate duration does not identify the cause.
- Rust dependency policy passed; Windows quality failed; dependent Windows 11 ARM64 x64-emulation lifecycle was skipped. No candidate was produced by this failed run.
- Local debug and clean LLVM full suites, twelve native-artifact cases and the 2 GiB run passed on `26e302b`. They do not override CI failure.

The original output lacks phase/gate/observation timing. Scheduler, network and filesystem explanations are hypotheses, not an established reconstruction. #39's minimum-spacing/coalescing contract and #41's readiness-versus-durable-acknowledgement distinction remain; neither explains these two timeouts by itself. See [PROGRESS_REGRESSION.md](PROGRESS_REGRESSION.md) and [RETRY_CANCELLATION_REGRESSION.md](RETRY_CANCELLATION_REGRESSION.md).

## First intervention: diagnostics without changing the contract

The first patch retains the 10 s completion and 5 s drain deadlines, worker/test parallelism, 8 MiB fixtures, explicit response gate and every spacing/monotonicity/count/terminal/exact-byte assertion. It does not change production behavior.

- Snapshot diagnostics read the separately published watch value, not the managed-state mutex that a slow critical filesystem checkpoint may hold. They report fixed enums and bounded numeric fields only: elapsed time, published state, bytes/size, workers, rate and failure kind. No task ID, URL, origin, filename, path, validator or credentials are formatted. A synthetic canary regression checks that boundary without starting networking.
- Cadence diagnostics add event/held-prefix counts, first-prefix and gate-release times, current gate-arrival predicate and last observed transition. They are observations of coalesced channels, not a complete execution trace.
- A `Failed` event now fails immediately with its stable failure kind instead of being ignored until the `Completed` timeout. This strengthens error attribution, not product success criteria.
- Late-consumer diagnostics consume no events before inactivity, preserving the actual coalescing boundary.
- CI runs a fixed three additional **full lifecycle suites at default parallelism**, recording every outcome even if the preceding release-target build/test step failed after creating the executable. An absent/ambiguous executable refuses the experiment; earlier failures stay failures. This is bounded diagnostic sampling, not retry-until-green. The original full workspace and target-filtered repetitions remain. Only these known test executables emit the new bounded observations.

The initial lint check rejected a redundant closure and the enlarged test's line count. Equivalent fixture/sample-assertion helpers satisfy linting; no lint rule or assertion was disabled. Local formatting/Clippy and all 32 lifecycle tests then passed. This is a diagnostics checkpoint, not #44 acceptance or a causal explanation.

## Remaining acceptance

Establish a causal reproduction/intervention for the actual observation boundary, correct the demonstrated defect without arbitrary deadline extension or assertion/parallelism reductions, and add regression/mutation coverage. Preserve any unavailable original trace explicitly. A later passing run alone is not evidence that the original failure has been explained. #44 and #28 remain unaccepted until their respective gates are satisfied.

## Causal counterexample and corrected boundary

Diagnostics-only `b61959c` passed all three jobs in CI `34325455523`. Its initial debug/release lifecycle suites were 10.42/10.32 s; the three additional full suites were 10.29/10.86/17.51 s. The additional late-consumer observations were 99/89/205 ms and cadence 791/795/848 ms. These values **do not establish a cold-start penalty or reconstruct the original failure**. A preliminary comparison with the older 51.37/57.07 s suites was insufficient; machine/load/cache/phase causes remain unclassified.

Inspection establishes a concrete boundary mismatch: the real probe client permits **30 s per connected request**, while both tests used **10 s for preparation, transfer observations, joined disk validation and promotion together**. Neither the progress policy nor coalescing promises ten-second end-to-end completion. Raising a deadline without separating those contracts would not be sufficient.

Two real-network/disk counterexamples now hold a fully received probe before ledger/response handling across the **actual old ten-second deadline**. At expiry they assert `Probing`, zero bytes and no failure. Releasing the owned barrier permits the same 8 MiB task to finish with all cadence or unread-event coalescing/exact-output assertions intact. Initial local totals were 10.795 s (cadence) and 10.140 s (coalescing); both were demonstrably valid tasks that outlasted the old whole-task budget. This is a controlled counterexample to the test premise, **not a claim that the original CI probe was delayed this way**.

The corrected tests separate:

1. **Preparation readiness:** independently published watch prefix of 2 MiB **and** selected second assignment at the response barrier. No ordinary events are consumed yet. This does not assert a durable task checkpoint; retained-range recovery has its own tests.
2. **Active cadence:** the original **10 s** missing-observation watchdog begins after that readiness. Observe at least four held-prefix samples and an actual rate estimate; only then release the response. Spacing, monotonicity and upper-count assertions remain. Preparation time cannot inflate the event-count upper bound.
3. **Workflow containment:** one **60 s** deadlock-containment budget includes preparation/observation/completion. It is not a measured/product SLO or a calculated worst-case Windows/filesystem bound. It allows the client's existing 30 s request contract plus joined local work rather than declaring all that work a progress-frequency failure. It does not restart on heartbeat events. Synchronous filesystem calls still cannot be forcibly interrupted by a cooperative Tokio timeout.
4. **Late consumer:** retain an unread event channel until inactivity under the workflow budget; then retain the original **5 s** drain watchdog, at-most-one ordinary sample, bounded sample fields, no overflow, exact terminal snapshot and exact 8 MiB output. The controlled delayed-preparation variant exercises the same assertions.

No production timeout, retry, persistence, protocol, worker width or test parallelism changed. Full-suite CI observations retain default parallelism. Diagnostics remain fixed-enum/numeric and early failure still refuses success.

### Mutation evidence and current gate

- HTTP410 fixture mutation at the diagnostic checkpoint failed immediately with fixed `HttpStatus`, not a masked completion timeout.
- Starting the active-cadence clock at workflow start, **while retaining the new 60 s workflow budget**, makes the delayed-preparation cadence counterexample fail its intended `active cadence observation deadline`. This distinguishes the correction from a blind global deadline extension.
- Suppressing ordinary events fails that same active-cadence watchdog after independent readiness (`held_samples=0`).
- Removing keyed coalescing fails the existing latest-sample buffer regression's equality assertion.

All mutated source was restored byte-for-byte before normal gates. Formatting, Clippy and the full local debug workspace passed; 34 lifecycle tests now include both controlled counterexamples and the diagnostic privacy canary. Final clean LLVM, repeated cases, final-tip CI and publication checks remain required. The original failed CI trace is permanently unavailable beyond its recorded timeout output; that uncertainty must remain in the accepted result rather than being replaced with a presumed infrastructure diagnosis.
