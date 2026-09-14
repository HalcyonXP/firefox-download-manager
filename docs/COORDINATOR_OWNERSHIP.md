# Engine coordinator retirement

## Distinctions

- **Transfer workers** perform scheduled network/file work; blocking validation is also awaited by the transfer coordinator.
- **Coordinator** is the Tokio task executing one engine run, retaining engine/store ownership through its final state and event operations.
- **Inactive** is a published task-state boundary. It does not by itself prove coordinator return or release of its engine/store references.
- **Joined shutdown** closes new-run admission and awaits every retained coordinator before reporting success. It is not message-delivery acknowledgement or proof of arbitrary crash/power-loss durability.

`wait_until_inactive` remains a state observation. A caller retiring an engine must await `shutdown`, then drop all remaining engine owners before expecting another engine to acquire its state store.

## Retention and admission

`task/coordinators.rs` owns coordinator `JoinHandle`s behind shared, cancellation-safe owners. Awaiting uses a borrowed handle inside an async mutex; cancelling a shutdown waiter does not take/drop that handle. Later shutdown calls can finish retirement. Join outcomes are memoized, and failed outcomes cannot become successful through repeated calls or housekeeping.

A consuming admission guard spans task activation, durable handoff commit where applicable, spawn and handle retention. Shutdown closes admission under the same lock; it cannot overlook work in the activation-to-registration interval. An in-flight synchronous activation/checkpoint may therefore finish before shutdown acquires that lock. No blocking filesystem operation is forcibly interrupted.

Start, retry, paused resume and new handoff commit refuse after admission closes. A repeated already-committed handoff still returns status without dispatch. Read-only state observations and metadata-only preparation are not newly authorized runs. Reopening run admission requires a newly opened engine; shutdown does not release a live engine owner's store lock automatically.

The registry uses the existing managed-task bound for retained coordinator slots. Admission harvests only actual completed join results, using a nonblocking poll of the retained handle; pending owners remain retained. Failed joins remain a sticky failure. Shutdown attempts all retained joins even if an earlier one fails, and refuses success for failed joins or inconsistent active state. There is no new retry sleep, relaxed deadline, increased concurrency or dependency.

## Reproduction and evidence boundary

CI34631705857 atf0850d3 failed the checksum-retention/restart test when reopening the state store after `shutdown` and engine drop: `Persistence(StoreLocked)`. Quality/dependency checks passed; package failed and emulation skipped. This was a different failure from the private-directory startup cases.

Source review found that the previous `spawn_run` discarded its coordinator handle, while shutdown waited only for `running:false`. A test-only gate in the real checksum-failure coordinator reproduced premature shutdown success and `StoreLocked` after acknowledgement. The fixture independently retained its coordinator, released the gate, joined it, and verified store reopening before asserting the failure. This demonstrates the ownership defect; it does not prove every historical store-lock or startup failure shares that cause.

With retained joins, the same gated shutdown stays pending, including when its inactive task has already been removed. Dropping that pending waiter leaves ownership intact; retirement and store reopening then succeed. Additional tests cover cancellation-safe/memoized joins, failed-result retention, bounded ownership slots, and closed start/retry/resume/commit admission with replay-free committed status. Paused-state admission uses a valid metadata fixture, not an observed interrupted transfer.

These are component observations, not installed-browser or exact-final-package qualification. Earlier independently checked bytes, task states and outer process lifetimes remain source-specific; none should be relabelled as explicit inner-coordinator joins merely because an inactive flag was observed.

Local workspace fmt/Clippy/tests/build/dependency gates pass. The reviewed LLVM/UCRT recipe passes five coordinator cases, all34 lifecycle cases (including checksum-retention/restart),28 storage cases, one stream-lease case, ten handoff cases and nine real bridge cases. Seven targeted mutations reject omitted retention/join, reopened admission, failed-join success, lost failure summary, missing slot bound and lost committed-status recovery; sources were restored before the complete gates. These do not qualify a new installed package.
