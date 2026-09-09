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
