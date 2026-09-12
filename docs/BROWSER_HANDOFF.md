# Browser handoff coordination

Status: recovery, pending UI and conservative eligibility components implemented; **production interception remains unselected**. The default/manual build does not cancel ordinary browser requests. A separate [automatic-capture candidate](CAPTURE_CANDIDATE.md) now selects these components with independently verified site/API authority; it is not release-qualified. These components are not persistent unsigned-XPI or installed Firefox-to-companion qualification. Native transaction semantics remain in [NATIVE_HANDOFF.md](NATIVE_HANDOFF.md).

## Decision and storage boundaries

`BrowserHandoff` obtains companion readiness and `prepared_handoff` support before recording a new ID. A two-second monotonic authorization budget includes readiness, storage, native preparation and cancellation intent. A delayed callback cannot authorize cancellation after that budget; JavaScript scheduling is not a real-time response guarantee. Late native/storage work stays retained and drainable, rather than being mistaken for cancelled work.

The extension-owned `pending-handoffs-v1` journal contains only canonical UUIDv4 IDs, stages and creation timestamps, with at most 32 entries. Changes are serialized, strictly decoded and independently read back. Corrupt or uncertain storage pauses capture without clearing existing records. It contains no filenames, URLs or HTTP context; the engine's separate existing task-storage contract still applies. Browser storage readback is not proof of sudden-power-loss durability or resistance to external same-user modification.

| Browser journal stage | Authority and recovery |
| --- | --- |
| `preparing` | Written before native prepare. No browser cancellation authorized. Recovery only attempts abort for this ID. |
| `intent` | Written and read back before returning `{cancel:true}`. This does **not** prove Firefox applied cancellation. Restart never automatically commits this stage. |
| `cancelled` | Matching live request yielded `NS_ERROR_ABORT` after cancellation was issued. Persist this observation before commit. Recover by status and the same ID; never replay prepare/Add. |
| `confirmed` | Explicit UI confirmation that Firefox stopped, not an observed cancellation event. Persist confirmation before commit; same-ID recovery only. |
| `fallback` | Firefox retained control; abort the unused native reservation. |

Only validated, ID-correlated Committed/Aborted receipts settle a native-attempted record. A lost acknowledgement stays pending. Committed status settles recovery without starting another transfer; Prepared may receive a commit only for `cancelled`/`confirmed`. Native-aborted or unknown IDs that cannot resolve the recorded decision remain visible. There is no unsafe forget/recreate or automatic expiry path.

Missing terminal observation becomes uncertain after five seconds. A mismatched terminal event does not authorize commit. A repeated request key invalidates its prior live ticket. A fresh-ID collision cannot abort or erase an older journal record. Recovery operations serialize per ID and skip live preparation/terminal work.

## Pending presentation

The manager port publishes pending records and a blocked-state warning; the extension badge indicates attention is needed. Labels join opaque IDs to the existing native task-name projection, without adding names to journal storage. Unknown details or any phase other than a known Prepared reservation do not offer Manager confirmation. `intent` offers explicit Manager/Firefox decisions and recheck; other stages offer recheck. Confirmation warns against competing Firefox output. Rendering is presentation, not native commitment or independent evidence of browser cancellation.

## Eligibility adapter and build selection

`capture-click.ts`, `capture-registration.ts` and `CapturePolicy` are selected by the explicit capture-candidate entry/manifest, not the default/manual build. The default manifest grants own storage but no mandatory sites, webRequest or blocking permission. Candidate permission/readiness/compatibility gates and UI are described in CAPTURE_CANDIDATE.md; broad authority is not inherited by the manual package.

The default conservative policy requires (the explicit cross-origin option is described below):

- A trusted, unmodified primary click on an ordinary same-tab HTML link. The message must arrive before request creation; late delivery falls back rather than borrowing another request's authority.
- One top-frame GET in an explicitly non-private default cookie store. Bind one click to one request ID; require the initiating document and initial link URL to match observed request data. Rapid clicks invalidate authority instead of guessing. Click/request counts and lifetimes are bounded.
- A redirect chain confined to the initial target origin. Each hop needs fresh sent-header observation; earlier credentials or unknown headers permanently invalidate the chain. At most eight subsequent request-creation callbacks are accepted.
- Only the closed ordinary anonymous request-header name list. Values are not retained or replayed. Cookie, Authorization, proxy credentials and custom headers refuse capture.
- A final 200 attachment with a Windows-safe filename. Session-setting/challenge/range responses, ambiguous representation headers, nonidentity Content-Encoding and unsupported Vary refuse capture.
- A still-live eligibility predicate through native preparation. Terminal proof must match the request ID, tab, immutable URL offered to Manager, final sent URL and supported context. Later request metadata cannot replace the URL already prepared.

POST/blob, iframe, private/container, new-tab attribution and ambiguous/expired/overflow cases stay with Firefox. Cross-origin redirects also stay with Firefox under the default policy. Header screening does not independently establish server resource identity, output correctness or browser event ordering. Real supported-click acceptance must verify those separately, including safe Firefox fallback.

## Evidence and remaining gates

The TypeScript tests exercise the journal, deadline/write races, discarded replies, restart recovery, same-ID receipts, closed capability negotiation, click consumption, redirect/session exclusions, terminal correlation and the mocked browser registration adapter. UI tests verify text projection, not actual browser interaction. Mutation checks target the intent checkpoint, cancellation classification, forbidden uncertain-intent replay, readback equality, click consumption, redirect-session retention and terminal correlation.

Integration found an actual compatibility defect: the companion advertised `prepared_handoff`, but the previous extension and JSON schema rejected it. Both closed capability lists now accept the capability, with an explicit companion Hello example. A matching updated XPI is necessary; native component evidence did not establish extension compatibility. The storage permission also required updating both independent manifest-policy validators. A modeled later-request counterexample initially let mutable sent-header metadata redefine terminal URL correlation; retaining the immutable decision URL now rejects that case, including both original-URL and later-URL terminal events.

An owned temporary loopback XPI has now exercised the real coordinator/native/UI nominal path and nine cleanup checkpoints against an identified older clean package; see [INSTALLED_BROWSER_SLICE.md](INSTALLED_BROWSER_SLICE.md). This does not qualify production interception or persistence.

Still required before production selection: remaining live recovery/fault cases, orphan/tombstone/history reconciliation, activation/permission/off-on behavior, persistent unsigned exact-XPI restart/click acceptance and final artifact qualification. Existing API-probe and installed-native reports remain separate source-specific observations. No current component result closes #49/#50 or unblocks #51 acceptance.


The dashboard now distinguishes durable native handoff phase from transfer state using `task_handoff_phase`; see [PROTOCOL.md](PROTOCOL.md#task-phase-metadata). Prepared/aborted/unknown snapshots do not offer ordinary mutating controls; committed history remains retained. A regression first demonstrated the former queued projection offered Pause/Start/Cancel for Prepared. The separate clean2df904b live diagnostic observed the updated-pair controls; component checks alone do not establish that result.


Recheck now queries uncertain `intent` by the same ID: already-Committed status settles it without sending commit; Prepared/Aborted stays pending. Explicit Manager continuation independently rechecks status before recording `confirmed`. An Aborted result refuses while preserving `intent`, allowing the separate Firefox/unused-reservation choice instead of stranding it as Confirmed. A modeled regression demonstrated both the previous no-op recheck and the stranded-Aborted stage. No new transfer is authorized from intent on restart, and no journal contents/limits/deadlines change. Clean2df904b live missing-terminal confirmation now passed in the temporary loopback diagnostic; see [INSTALLED_BROWSER_SLICE.md](INSTALLED_BROWSER_SLICE.md#clean-phaserecovery-observations-2df904b). This does not qualify subsequent changed code or persistent installation.


## Opt-in cross-origin chain binding

The default remains same-origin. `CaptureOptions.crossOriginRedirects` permits explicitly observed cross-origin transitions while preserving the other eligibility requirements; `originAllowed` can impose a narrower per-hop authority boundary. An origin is scheme, host and effective port, not a registrable domain.

`onBeforeRedirect` now supplies status and read-only response headers. Only301/302/303/307/308 with a single resolvable Location matching Firefox's target can authorize the next same-ID request. Missing/different transitions, changed tab/context, a post-decision redirect, session-setting/challenge/range redirect responses, missing or non-anonymous sent headers, more than eight transitions and any HTTPS→HTTP step invalidate the chain permanently. Each destination needs fresh sent-header evidence. Native preparation still receives only the immutable final URL/safe filename; no cookies, authorization or redirect-header values are replayed or added to ordinary observations.

A modeled regression first showed the old same-origin policy accepting a later URL different from its observed redirect target. Explicit target consumption now rejects it. Cross-origin positive/refusal, registered response-header wiring and diagnostic origin-scope tests cover the new logic; eight targeted mutations reject target/default/TLS/session/scope/header-wiring/hop-bound omissions. These are not browser API ordering or provider-compatibility proof. The new temporary diagnostic is bounded to at most two exact loopback origins; production build/manifest selection is unchanged.


Clean1cde75b subsequently passed the actual two-origin installed-browser diagnostic, plus nominal and missing-terminal regressions, against paired2df904b. [Source-specific observations](INSTALLED_BROWSER_SLICE.md#clean-cross-origin-observations-1cde75b--paired2df904b) include temporary reload, independent native/Firefox outputs and joined retirement. This establishes those owned loopback callbacks and controls, not general event ordering, distinct-host/TLS/provider behavior or persistent installation. The nine earlier cleanup faults remain scoped to2df904b; no production activation follows from these reports alone.


## Explicit stopped/unlinked reservation recovery

Journal **loaded** means its initial strict decode finished successfully; it is distinct from native connection readiness and from unblocked storage. Preparation now publishes its journal entry before the native prepare call, so a newly projected reservation is not presented as unlinked merely because the view lagged the native response.

- Cancelled/Confirmed records paired with a known native Aborted task offer an explicit acknowledgement. The warning explains that Manager cannot finish it and the button does not start Firefox. The coordinator rechecks the same ID's Aborted receipt before strict journal settlement. Recheck alone still preserves this notice; Prepared/Committed/unknown/foreign receipts and failed storage do not dismiss it.
- **Unlinked reservation** means a native Prepared task absent from successfully loaded, unblocked journal history; it does not prove Firefox was never cancelled. At most32 candidates are shown. Explicit discard rechecks history/activity before and after the asynchronous native status read, aborts only Prepared, and validates the matching Aborted receipt. An already-Aborted status can confirm an earlier lost abort reply without replay. Committed/unknown identities refuse; no journal entry or native history is erased.
- Recovery serializes by ID and refuses active/recorded work. New preparation also refuses an ID held by recovery, preventing a concurrent same-ID capture from acquiring cancellation authority during cleanup. The background accepts only the closed cleanup message and delegates to the coordinator, never generic Cancel/Remove/Add.

The fixed UI warnings and confirmation-result dispatch, real renderer wiring, coordinator state/receipt/storage/race boundaries and diagnostic scope are covered by226 TypeScript tests; eight targeted mutations reject after restoration.92 Python tests cover the separate opt-in recovery driver and existing boundaries. These additions do not alter native lifecycle/persistence, journal record format, dependencies, production capture selection, deadlines or protections. Clean6f8b209 nominal and clean23bfa69 recovery observations now exist against paired2df904b; see the source-scoped [recovery observations](INSTALLED_BROWSER_SLICE.md#clean-recovery-observations-6f8b209-and23bfa69--paired2df904b). They do not qualify later capture-control changes or persistent installation.


## Persistent capture preference and activation boundary

`capture-control.ts` separates **available** (one reviewed listener registration completed), **ready** (strict saved-preference decode completed), saved **enabled**, and effective authorization. Missing preference defaults on, but cannot authorize anything before readiness and explicit activation. The default/manual background does not activate an interceptor and its manifest grants no new site/webRequest authority, so its manager labels capture unavailable. The separate candidate requires verified website/API authority and compatible native readiness in addition to this preference. The owned diagnostic explicitly activates its existing narrowly scoped registration; its separate arming gate still applies.

The closed `{version:1, enabled:boolean}` value lives under `automatic-capture-v1`, separate from handoff history. Off immediately revokes new authorization in the background, then serializes the write and independently reads it back. On remains paused until verification; overlapping writes cannot expose stale authorization. Corrupt/future data, failed writes or uncertain readback pause capture without repairing unknown data or touching pending handoffs. Failed-write pausing is scoped to this background instance: no durable Off is claimed without verified storage, and a later instance rechecks the stored value, which may still be On. The UI states this explicitly. A failed partial registration stays unavailable and cannot be blindly repeated. This preference does not pause/cancel existing Manager transfers or erase recovery records.

Only the owned manager port accepts the closed boolean preference action. It uses an independent queue so an outstanding native action does not delay Off. The checkbox displays unconfirmed/disabled state while delivery or verification is uncertain; a disconnected interface cannot claim that a queued change took effect. Renderer, storage, routing, restart and diagnostic-eligibility models cover these distinctions. The clean4f5c006 [six-case batch](INSTALLED_BROWSER_SLICE.md#clean-capture-control-regression-batch-4f5c006--paired2df904b) now observes live toggle behavior and related recovery paths against paired2df904b. Production permission approval/activation and exact-XPI persistence remain separate acceptance requirements.
