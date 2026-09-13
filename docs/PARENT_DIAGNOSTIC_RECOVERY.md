# Installed-parent diagnostic recovery

This is failure cleanup for the owned, temporary-XPI installed-parent diagnostic. It is not a production capture fallback, a persistent-install observation or authority to adopt processes. The original fileless observer/client and ordinary installed driver remain unchanged.

## Separate observations

| Observation | Meaning |
| --- | --- |
| Original `evidence.failed` | Sticky qualification failure; later cleanup never clears it. |
| Cleanup tracker | The same collector's strictly decoded, monotonic retirement facts. It cannot issue success receipts. |
| Automation read gap | A rejected automation command or interruption prevented a complete observation. Fresh cleanup-only readback may be attempted against the original process/sandbox/collector. |
| Parsed corruption or changed identity | Malformed/open envelopes, collector/prefix replacement or other non-automation failures quarantine cleanup too. No reset or replacement tracker is allowed. |
| `removal_returned` | The single observer-removal operation returned a valid removed observation. |
| Removed after readback | A fresh snapshot confirmed the original collector is removed after an uncertain return. This does not relabel the earlier operation as returned or repeat its inverse. |

The selected `CleanupObserverClient` retains both trackers before collector installation. The normal snapshot interface stays refused after its qualification evidence fails. Cleanup readback requires an already-failed browser owner and uses the same retained process object, named sandbox, nonce/collector and closed envelopes. Context restoration is independent; a later restore error cannot replace an earlier interruption. Pure envelope/history validation runs before restoration, so known corruption remains quarantined even when restoration also fails or is interrupted. Validation alone does not accept records or issue receipts.

Native process wait, exit-code observation, pipe closure and I/O settlement remain required before browser shutdown. Collector removal alone is not SDK retirement. Failed-but-joined native observations can support only the existing failure-cleanup contract. Browser diagnostic version4 records original and cleanup facts separately; it is not native wire2 or a successful lifecycle receipt. The enclosing private failure/cleanup file uses version2.

## Keep the original setup authority

On a browser cleanup failure, the installed-parent driver independently requests owned Manager quiescence but keeps the **original setup owner/window** alive. The unchanged `BrowserPeer` permits that parent's explicit Manager-joined observation, but still needs the live original setup witness and unchanged installation binding. Closing setup prematurely would also discard the ability to verify removal through that owner.

The setup process reference cannot be replaced, and reinitialization cannot discard its plan or ownership. Once browser/fixture retirement is established, cleanup either makes the first Uninstall dispatch or reads back the one already attempted. Readback requires matching completed setup state, exact removal status, fresh closed-app/all-view checks and absence of the retained binding's program/receipt/journal paths. Only then is setup retired. An uncertain Uninstall is never replayed. Missing resource ownership remains held; unconfirmed removal does not authorize closing the live original setup owner.

Cleanup attempts independent resource/Manager/record stages and preserves interruption over later ordinary errors. The supervisor's final cleanup permits one failure-only continuation; further `continue_retirement()` calls require a consumed failed-cleanup path. They cannot restart the run or repair its qualification. No timeout/concurrency or protection setting changes are introduced.

## Refusal evidence before a held state

Manager-tab creation uses separate fixed stages for response shape, tab type,
handle validity, distinction from the retained control tab, and selection. The
validation and one-shot creation rules are unchanged. A refusal at the former
compound guard does not identify which predicate failed. Diagnostics retain no
returned handles or unknown response fields; flat Marionette and single
`value`-wrapped replies remain supported.

After an ordinary tab-creation/validation/selection failure, one optional
`parent_tab_state.js` sample records only five boolean-or-null fields from the
original Marionette chrome window: its window-modal marker, navigation-toolbox
collapse, selected-tab/browser consistency, and whether the selected browser
matches the retained control identity and `about:blank`. No tab enumeration,
selection, prompt dismissal, preference write or returned URL/handle is added.
The sample is later than the failure; it cannot reconstruct transient state or
establish that all kinds of warnings are absent. A modal marker is not a cause
diagnosis. Existing WebDriver creation and prompt handling remain unchanged.

The first failure is retained before this sample. Observation intent is consumed
before commands; malformed or unavailable observations remain `unavailable` and
cannot be retried by cleanup. Original interruptions skip and consume the sample
without issuing new diagnostic commands. Script/context-restoration interruptions
retain priority over ordinary failures. Version4 adds `tab_failure` to the existing
pre-hold record: null (not applicable), `skipped-interruption`, `unavailable`, or an
`observed` closed version1 flag set. These states never authorize recovery or a
successful qualification. The enclosing private-file version2 is unchanged.

`qualification.failure_location.failure_location` copies only allowlisted source tags and line numbers from an exception traceback: at most eight recognized locations within a 32-frame observation. Unknown frames consume the budget without exposing filenames or line numbers; `trace_truncated` reports a remaining tail. No exception text, arguments, frame locals, source text, or raw paths are serialized. Interpret locations against the run's separately pinned source revision. They identify propagation sites, not a proven cause or recovery authority.

The first browser failure includes these locations. The first cleanup refusal records its fixed cleanup-step label and locations. Existing exclusive `parent-failure.private.json` and `parent-cleanup.private.json` writes occur before the controller returns a failed-held status; they do not wait for final GUI shutdown. Failed diagnostic observation preserves the original failure and cannot skip independent cleanup stages or replace an earlier interruption. A partial observation may retain only the failure stage/step. File creation/closure is not a sudden-power-loss durability guarantee.

These records neither relax the original binding/process-inventory checks nor replay Uninstall. A failed-held controller is stopped pending recovery, not still making download progress. External interruption can destroy retained process authority; later process absence or a separately owned removal does not reconstruct the missing joins.

## Retained control channel

`qualification.parent_retirement_control.RetirementControl` retains one original SDK diagnostic owner before effects. This optional local-host library supplies no browser launch, package selection or GUI. The original fileless `parent_supervisor.serve` interface is unchanged.

The closed ASCII request is `{version:1, sequence, command}` followed by a newline, at most256 bytes. Sequences are exact integers1–32, consumed before dispatch; commands are `start`, `status`, `continue`, and `finish`. Start is once. Continue requires the original started, failed, cleanup-attempted, held owner and calls only its guarded retirement continuation. Finish requires fresh retirement, records intent before output, and preserves cancellation. As the first request, Finish stops an initialized but undispatched controller: it marks failure and attempts only the original one-shot cleanup, without starting the test. This pre-dispatch stop is distinct from an interruption reported by `cancelled`. Its successful retirement response has `started:false`, `failed:true`, `cleanup_attempted:true`, and `held:false`; later Start is refused. An unresolved owner still prevents Finish. Failed qualification never becomes success.

Replies are bounded newline-delimited `{version:1, sequence, kind:"status", status}` or `kind:"phase", phase` objects. Phases are fixed names, synchronous on the original thread, before that request's terminal status: at most32 per command and96 overall, leaving32 response slots within128 total frames. A single-flight reader must hold a full legal33-frame burst without requiring concurrent consumption. These control sequences/version1 are not native wire2, admission IDs or SDK authority.

Failed-held status keeps input open for deliberate continuation. Unknown writes are not repeated. EOF or sink/notice failure invokes only existing one-shot cleanup and keeps the original host alive while resources remain unresolved. Substituted owners/thread authority are not adopted. The caller must also retain the control across GUI/entry/exit-record failures; a host exit alone is not an SDK retirement receipt.

## Evidence and remaining work

Two fileless counterexamples against the earlier driver reproduced rejected removal confirmation after a context-restore error, and premature setup retirement while browser cleanup remained unresolved. Current models cover those cases, original/cleanup separation, uncertain-removal readback, corruption quarantine, cancellation, exact setup identity, Uninstall readback and the unchanged BrowserPeer after Manager join. Review additionally reproduced a complete invalid return envelope being misclassified as a read gap after a later restoration error; pre-restoration validation now rejects that combination, including cancellation. All291 Python cases, unchanged npm gates and13 targeted rejected/restored mutations pass. These are modeled controller observations, not new browser/SDK execution or identification of the earlier installed attempt's historical error.

Fourteen selected-control models cover the actual installed driver's failure continuation with a modeled setup, sticky cancellation, exact owner/thread binding, malformed/duplicate/sequence refusal, output uncertainty, bounded progress and retained EOF holds. Ten targeted mutations reject with restored sources. An additional redundant late-check mutation survives because the underlying supervisor independently rechecks retirement; it is not counted as a rejected mutation. A fileless Python-host/withdrawn-GUI interop exercises held input, deliberate continuation, failed-but-retired exit and actual host/reader/controller joins; its stub owner is not SDK execution. Integration reproduced a legal32-phase-plus-status burst overflowing a32-slot prototype reader; its bound is now33 without a timeout/concurrency change.

A further request-handler model reproduced initial Finish being refused before dispatch, leaving no acknowledged pre-dispatch cleanup operation. The existing failure/EOF cleanup path is distinct from such an acknowledgment. Two additional control models and three targeted rejected/restored mutations cover undispatched cleanup, no execution, sticky failure and held-owner refusal; all307 Python cases and npm gates pass. A fileless Python helper observed first-Finish `started:false`, failed retirement, exit1, an actual process wait and reader join. This does not observe a browser or SDK shutdown.

Ten source-location models cover bounded trace observation, redaction, a modeled BrowserPeer refusal before profile creation, first-failure retention, pre-hold file persistence, independent cleanup and interruption priority. Nine targeted mutations reject with restored source/test identities and original child waits; all317 Python cases and npm checks pass. These tests do not identify a historical refusal or establish new browser/installed acceptance. The engine, native protocol, package selection, preflight rules and deadlines are unchanged.

A new live controller still needs verified running-versus-failed-held presentation, original GUI/outer-process lifetime handling and protection against accidental window closure discarding owners. No existing consumed controller/domain may be rerun. Persistent unsigned installation, protected capture and exact-final-package acceptance remain separate gates.
