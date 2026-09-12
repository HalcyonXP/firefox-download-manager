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

Native process wait, exit-code observation, pipe closure and I/O settlement remain required before browser shutdown. Collector removal alone is not SDK retirement. Failed-but-joined native observations can support only the existing failure-cleanup contract. Diagnostic version2 records original and cleanup facts separately; it is not native wire2 or a successful lifecycle receipt.

## Keep the original setup authority

On a browser cleanup failure, the installed-parent driver independently requests owned Manager quiescence but keeps the **original setup owner/window** alive. The unchanged `BrowserPeer` permits that parent's explicit Manager-joined observation, but still needs the live original setup witness and unchanged installation binding. Closing setup prematurely would also discard the ability to verify removal through that owner.

The setup process reference cannot be replaced, and reinitialization cannot discard its plan or ownership. Once browser/fixture retirement is established, cleanup either makes the first Uninstall dispatch or reads back the one already attempted. Readback requires matching completed setup state, exact removal status, fresh closed-app/all-view checks and absence of the retained binding's program/receipt/journal paths. Only then is setup retired. An uncertain Uninstall is never replayed. Missing resource ownership remains held; unconfirmed removal does not authorize closing the live original setup owner.

Cleanup attempts independent resource/Manager/record stages and preserves interruption over later ordinary errors. The supervisor's final cleanup permits one failure-only continuation; further `continue_retirement()` calls require a consumed failed-cleanup path. They cannot restart the run or repair its qualification. No timeout/concurrency or protection setting changes are introduced.

## Evidence and remaining work

Two fileless counterexamples against the earlier driver reproduced rejected removal confirmation after a context-restore error, and premature setup retirement while browser cleanup remained unresolved. Current models cover those cases, original/cleanup separation, uncertain-removal readback, corruption quarantine, cancellation, exact setup identity, Uninstall readback and the unchanged BrowserPeer after Manager join. Review additionally reproduced a complete invalid return envelope being misclassified as a read gap after a later restoration error; pre-restoration validation now rejects that combination, including cancellation. All291 Python cases, unchanged npm gates and13 targeted rejected/restored mutations pass. These are modeled controller observations, not new browser/SDK execution or identification of the earlier installed attempt's historical error.

A new live controller still needs usable running-versus-failed-held presentation, a retained control channel during holds, deliberate safe continuation and protection against accidental window closure discarding owners. No existing consumed controller/domain may be rerun. Persistent unsigned installation, protected capture and exact-final-package acceptance remain separate gates.
