# ADR 0013: Install, restart, click — visible companion and persistent Firefox integration

- Status: Accepted product direction; browser/IPC mechanisms require the linked feasibility work
- Date: 2026-09-10
- Work: #48–#53, M5 — Install, restart, click
- Distribution scope: [ADR0016](0016-unsigned-personal-xpi.md) supersedes the earlier signed-XPI requirement.
- Supersedes for the next release: manual-only capture, temporary-XPI user installation, and ADR0003's browser-owned engine lifetime. The v0.1.0 tag/assets and historical evidence stay unchanged.

## Product requirement and baseline

The next release must provide:

1. Run `setup.exe` to install the companion app.
2. Observe a visible system-tray companion.
3. Install the unsigned personal XPI once through ordinary Firefox installation prompts.
4. Restart Firefox.
5. Click a supported download link normally; one Manager task starts and produces independently correct output.

v0.1.0 implements manual toolbar/context-menu capture, Firefox-owned stdio networking and a temporary unsigned-XPI workflow. Its tests do not establish persistent installation or ordinary-click capture. The absence of an interceptor is a product limitation, not evidence of a failed XPI installation.

## Decisions

### User experience is the acceptance contract

Use the short target workflow in `USER_WORKFLOW.md`. No terminal commands, `about:debugging`, repeated XPI loading, context-menu action or per-click Add form may substitute for that acceptance test. Ordinary Firefox installation/permission approvals are still legitimate user actions, not security settings to bypass.

A new release must have an ordinary `setup.exe` entry point with clear install/launch/error UI. Setup starts the visible companion; users can launch it normally and deliberately quit it. A running companion must register a tray icon, handle shell restart and expose status/Open Manager/Quit behavior. Windows may place registered icons in tray overflow; that is not permission for the app to omit its icon. Do not run a hidden continuing engine if tray startup fails.

### Companion owns the engine; browser bridge does not

A per-user Rust companion with one state/engine owner persists across Firefox restart. Independent companion lifetime is the selected architecture; it does not imply a service or unattended Windows startup. No service, elevation, silent logon autorun or scheduled task is authorized.

The Firefox Native Messaging process becomes a bounded bridge to the companion. Specify authenticated/local-user-confined IPC, version pairing, singleton ownership, command correlation/idempotency and graceful quit/checkpoint behavior before implementation. Do not detach today's helper on EOF, run a second engine over the same state, or introduce an unauthenticated loopback HTTP control endpoint. Current helper lifecycle protections explain why simply hiding/detaching it was previously rejected in ADR0003.

Keep Firefox capture, controls and display, and Rust networking/scheduling/storage/validation/recovery. Native tray/install UI supplements these boundaries; it is not a new network engine in JavaScript.

### Ordinary download clicks, not every request

Automatic capture means a supported HTTP(S) file download initiated by an ordinary click is added and started using saved settings. It is enabled after ordinary installation/permission approval unless the user turns it off; fresh-install acceptance must not quietly add a manual enable/configuration step. It does not mean hijacking page navigation, replaying POST bodies as GETs, cancelling all download events, or collecting cookies/authorization implicitly. GGUF is an opaque downloadable file type, not a media-extraction feature. The public Hugging Face acceptance case and bounded observations are recorded in #49.

#49 must prove the browser classification, permission and handoff mechanism before #51 selects it. Firefox response-time blocking promises are a candidate, not an already proven product design. A native prepare/accept/browser-cancel/commit or equivalent contract needs bounded waits and stable receipts so failure, restart, duplicate events and lost acknowledgements do not create duplicate tasks/files or silently lose the click. Existing v2 Add acknowledgement alone does not establish that cross-process contract.

Unsupported/unsafe cases remain in Firefox with clear feedback. Test GET downloads with redirects, query/signed targets and attachment responses without filename extensions, as well as ordinary navigation. Private/container/POST/blob/session-bound cases must not be guessed into the supported set. Preserve the existing explicit opt-in session path; automatic capture is not authorization to harvest or persist credentials. Already transmitted network bytes cannot be retracted; claim one owned task/output, not zero probe/overlapping in-flight network traffic.

### Persistent unsigned personal installation

Use a **persistent unsigned personal XPI** for Firefox Developer Edition on Windows 11. Signed XPIs, Mozilla signing/submission, signing credentials, publisher accounts and public AMO listing are **not requirements or release gates**. This replaces the earlier signed-distribution requirement; it does not relax browser protections or declare persistent installation tested.

Existing Developer Edition configuration can already permit persistent unsigned installation without a new settings change. Existing configuration, configuration changes, artifact signing and observed persistence are separate facts. Do not infer one from another or from the `.xpi` extension. Normal Firefox profiles and settings must not be inspected or changed for testing. No signature-enforcement preference change, enterprise-policy bypass, profile injection or temporary-addon reload may substitute for persistent-install acceptance.

Bind the reviewed source, exact unsigned XPI, stable add-on identity, native payloads, manifests, hashes and notices. Verify normal installation, restart survival and actual native/browser behavior against those bytes in owned isolated state. Existing extension compatibility and older temporary-XPI evidence do not qualify Manager's persistent workflow. If an isolated environment cannot accept the unsigned artifact unchanged, record that environment limitation without introducing a signing/account prerequisite or weakening protections.

## Execution and release gates

- #48: reconcile baseline, vocabulary, plan and acceptance contract (this decision).
- #49: actual Firefox capture and persistent unsigned-install feasibility.
- #50: visible companion, bounded authenticated bridge and ordinary setup UI; may proceed independently of #49 after #48.
- #51: production automatic capture, dependent on #49/#50.
- #52: exact unsigned personal packaging and concise install instructions, dependent on #49/#50/#51.
- #53: exact final-main qualification and new version/tag/publication, dependent on merged implementation and successful main CI, following ADR0012's ordering.

Final qualification must execute setup → real tray → normal unsigned XPI install → Firefox restart → ordinary GGUF click → one automatically started native task → independent correct output. No temporary-addon API, manual Add/context-menu action or mocked transport may stand in for this path. Recheck setup/IPC lifecycle, shell restart, failure/uncertain handoff, existing adversaries, privacy and resource boundaries. A new release (working target0.2.0, our versioning choice) is not approved until these gates pass.

Actual browser/registration tests retain closed-app/ownership preflights and isolated state; see [ADR0014](0014-autonomous-completion.md). No normal-profile inspection/modification, unowned process termination, security downgrade, VPN behavior, telemetry, remote updater, private-archive import or manufactured clean-machine evidence is permitted.

## Sources and knowledge status

Reviewed2026-09-10:

- [Mozilla temporary installation](https://extensionworkshop.com/documentation/develop/temporary-installation-in-firefox/): development loading is not the requested persistent user installation.
- [Mozilla signing/distribution](https://extensionworkshop.com/documentation/publish/signing-and-distribution-overview/): platform distribution background, not a signing/submission requirement for this unsigned personal project. No signature-enforcement settings are changed.
- [Firefox onHeadersReceived](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/API/webRequest/onHeadersReceived): Firefox can return a blocking response promise with the required permission. This documents API availability, not proof of safe request/download classification or end-to-end handoff.

Open: exact Manager unsigned-XPI persistence, actual capture correlation and minimal permissions, complete installed tray/IPC/setup acceptance, API/protocol integration and final-artifact qualification. The public GGUF acceptance case is recorded in #49. Settled: unsigned personal distribution, the five-step experience and unchanged correctness/privacy/protection boundaries. Component evidence does not close browser acceptance.
