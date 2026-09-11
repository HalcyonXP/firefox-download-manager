# Native ordinary-click handoff — implementation contract

Status: engine transaction and opt-in companion bridge commands implemented; production automatic capture remains disabled. Browser API observations are in [FIREFOX_CAPTURE_API.md](FIREFOX_CAPTURE_API.md). Distribution remains persistent unsigned personal XPI under ADR0016.

## Selected transaction boundary

A handoff identifier is a fresh canonical UUIDv4 chosen before the first prepare request. It identifies one immutable URL/destination/name/worker/checksum tuple, not a reusable Add correlation ID. The engine must exclusively create and flush a preparation record before returning Prepared. Preparation creates no download file, probes or workers. Existing manual Add remains create-and-start.

- **Prepared**: durable intent awaiting a positive browser cancellation observation. Generic Start/Resume/Retry/Cancel cannot start or dispose of it. Repeated identical prepare returns the existing phase; a changed tuple or a normal-task collision is refused.
- **Committed**: commit atomically records the committed phase and probing state before starting owned work. Duplicate commits return current state, including paused/failed/completed, without starting or retrying again. A lost commit reply is resolved by status for the same ID, never a fresh Add.
- **Aborted**: a durable cancellation of Prepared, without any network work. Repeated abort is idempotent. Commit after abort and abort after commit are refused.

The initial implementation retains handoff records, including aborted ones, within the existing 10,000-record limit. Generic Remove refuses them until a bounded non-sensitive tombstone/cleanup policy is implemented; silently forgetting an ID would reopen replay. Unknown or corrupt records are preserved/refused, never interpreted as permission to recreate an uncertain task. No automatic expiry deletes a preparation that may correspond to a cancelled browser request.

Use task envelope 5 for handoff-bearing records only: the strictly validated task4 payload plus a required handoff phase at the envelope level. Ordinary records remain task4 and existing v1–v3 migrations remain unchanged. Older engines refuse version5 as incompatible rather than treating Prepared as runnable Queued. This format is independent of wire2. The opt-in companion bridge advertises `prepared_handoff`; the legacy stdio-owned engine neither advertises nor dispatches it. Browser pending-state handling and UI are now implemented as components in [BROWSER_HANDOFF.md](BROWSER_HANDOFF.md), but must be qualified before selecting automatic capture.

## Browser coordination boundary

Browser and native storage cannot commit atomically. The extension must durably retain its pending ID and cancellation intent before giving up the browser request. Only a positively correlated browser cancellation event may authorize automatic native commit. If a browser/companion crash makes cancellation or commit uncertain, retain and surface the pending handoff; do not guess, silently lose the click, or create competing output. Missing eligibility, permissions, preparation or deadline must leave Firefox untouched. Prepared is not a receipt that the browser was cancelled.

Browser journal/recovery, pending presentation and conservative click/eligibility components now exist. Live integration, cross-origin behavior and end-to-end exact-artifact acceptance remain open; see BROWSER_HANDOFF.md. These engine APIs alone do not deliver automatic capture or qualify persistent unsigned installation.

## Opt-in wire2 commands

All commands require negotiated `prepared_handoff`. `prepare_handoff` takes `{task_id, download}`; `download` uses Add fields but forbids any `request_context` member, including null. Destination/name/workers resolve once using validated input or current settings. Those resolved values form the immutable tuple: changed defaults may refuse a repeated omitted-field prepare; use status for the same ID instead of inventing a replacement ID. `commit_handoff`, `abort_handoff` and `get_handoff` each take `{task_id}`. Successful results are `{phase, task}`, where task is the sanitized full task projection, including matching `handoff_phase` when `task_handoff_phase` is advertised. Unknown IDs are errors, not empty successful receipts. Frames and typed payloads retain existing size/shape limits.

Preparation is authorization to wait, not proof of browser eligibility, cancellation, credentials, redirect safety or native output. Only an explicit commit can start engine work. The extension can recover existing pending IDs through these commands; it does not register an ordinary-download interceptor.

## Component verification and limits

- Nine engine tests cover preparation with no network/download files, generic-control refusal, immutable inputs, concurrent duplicate commits, independent 128 KiB output, reopen, aborted-ID retention, unavailable runtime, corrupt/colliding records and refused commit/abort replacement under an owned Windows file lease.
- The real authenticated companion pipe test discards prepare and commit replies, reconnects by the same ID and produces one independently verified 64 KiB output plus Completed. It observes joined worker/peer retirement and reopens the engine without replay. This is not Firefox or installed-artifact qualification.
- Three targeted mutations were rejected: nonexclusive initial creation, forgetting an aborted ID, and skipping the commit checkpoint. Sources were restored before the complete gates.
- Wire tests cover all four commands, strict IDs/unknown fields/context exclusion and legacy-host refusal. A targeted null-context regression first failed: generic Option decoding treated an explicitly null member as absent. Handoff decoding now rejects the member's presence before typed Add decoding; no context is silently stripped.
- Recovery from the commit-before-dispatch boundary is a metadata fixture, not an observed abrupt process crash. The implementation uses the existing flushed-file/atomic-replacement store contract. Process-reopen observations do not prove sudden power-loss durability, durable directory-entry ordering or resistance to external same-user state modification. These limits must not become an exactly-once claim across arbitrary machine/storage failures.
- Local workspace formatting/Clippy/tests/build, dependency policy, npm checks and privacy screening passed. The nine handoff engine tests and all eight bridge tests also passed the reviewed LLVM/UCRT release target. The first complete workspace run exposed an obsolete future-version fixture: it used ordinary version4 + 1, which is now the supported handoff envelope. The fixture now uses handoff version5 + 1; malformed version5 remains independently rejected.
- Records are retained, not automatically expired or garbage-collected. Replay-safe tombstones/expiry and exact unsigned-artifact acceptance remain open. Browser deadline/recovery/UI components require live integration qualification. No browser protection or normal profile was changed by these component tests.


## Completion-deadline diagnostic boundary

CI34558329318 at6ff1843 passed debug quality but failed the release-target `lost_prepare_and_commit_replies_recover_by_id_without_a_second_transfer` at the unchanged five-second durable-completion observation. Seven other bridge tests passed. The original failure recorded no last task state, progress or pending I/O phase, so it does not distinguish slow transfer, terminal failure or delayed response. No shared cause with private PowerShell startups is established.

The test now retains response count, last reported closed-enum task state, byte count, allowlisted failure code and a fixed pending-operation label. It never formats raw responses, identifiers, URLs, paths or failure context. Deadline failure drops its peer, explicitly joins the exact worker, retires the fixture and preserves the failed domain before refusing. Production behavior, deadlines, concurrency and dependency graph are unchanged.

Nine bridge tests pass locally in debug and reviewed LLVM/UCRT release configurations, including a fileless observation-filter control. A deliberately retained HTTP response gate rejected completion after the existing deadline and reached explicit worker join/fixture retirement; this diagnoses the test boundary, not the hosted failure. The initial diagnostic edit exceeded the function-length lint; extracting an ownership-preserving completion/retirement helper resolved it without a lint exception.


## Phase-aware controls

Full task snapshots now carry durable handoff phase through `task_handoff_phase`; it is not inferred from queued/cancelled state. The extension hides ordinary controls for Prepared, Aborted and unknown older-companion snapshots, retaining Open folder. Committed tasks retain normal transfer controls but not Remove: engine/store history retention still forbids forgetting their IDs. Both dashboard selection and the NativeConnection dispatch guard share these restrictions, so a stale UI command cannot bypass them. Engine enforcement is unchanged. Waiting/discarded reservations have explicit status text rather than apparent queued transfer progress.

This does not implement replay-safe expiry, orphan reconciliation or automatic capture. The earlier installed-browser diagnostic used older native bytes. A subsequent clean2df904b temporary-XPI run observed this pairing, including missing-terminal recovery; [INSTALLED_BROWSER_SLICE.md](INSTALLED_BROWSER_SLICE.md) preserves both identities and limits.


Local phase/recovery checkpoint checks pass workspace formatting/Clippy/tests/build, nine engine and nine actual bridge tests on debug and reviewed LLVM/UCRT release flags, plus TypeScript and Python driver models. Targeted mutations reject omitted snapshot phase, dashboard phase input, dispatch restriction, intent auto-commit and aborted-intent protection. These are component results, not new installed/browser qualification. Initial snapshot decoding must use the already validated Hello, not the connected-only support accessor; connection readiness itself depends on decoding that snapshot.
