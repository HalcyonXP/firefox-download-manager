# ADR 0013: Install, restart, click — visible companion and persistent Firefox integration

- Status: Accepted product direction; browser/IPC mechanisms require the linked feasibility work
- Date: 2026-09-10
- Work: #48–#53, M5 — Install, restart, click
- Supersedes for the next release: manual-only capture, temporary-XPI user installation, and ADR0003's browser-owned engine lifetime. The v0.1.0 tag/assets and historical evidence stay unchanged.

## Confirmed owner intent and observed mismatch

The owner reports installing the XPI, restarting Firefox, checking it was running and clicking a GGUF download. Firefox's built-in downloader handled it. They explicitly require a simpler workflow:

1. Run `setup.exe` to install the companion app.
2. The running companion has a system-tray icon, not an invisible background presence.
3. Install the XPI once.
4. Restart Firefox.
5. Click a download link normally; it is added to Manager and starts downloading.

The owner did not specify an interception API, IPC transport, signing account, Windows-logon startup, or GGUF provider/URL. We have not inspected their normal profile or verified their installation method/native registration. Do not infer an absent extension from the restart report: the released background script has no ordinary-download interceptor, regardless of connection state.

v0.1.0 intentionally implemented manual toolbar/context-menu capture, Firefox-owned stdio networking and a temporary unsigned-XPI workflow. Its actual Firefox tests proved those paths, not the owner's newly confirmed ordinary-click/install-once path. Calling that release ready did not mean this desired experience was delivered. Better wording alone cannot supply the missing behavior; this is a product correction, not advice to keep using the rejected workaround.

## Decisions

### User experience is the acceptance contract

Use the short target workflow in `USER_WORKFLOW.md`. No terminal commands, `about:debugging`, repeated XPI loading, context-menu action or per-click Add form may substitute for that acceptance test. Ordinary Firefox installation/permission approvals are still legitimate user actions, not security settings to bypass.

A new release must have an ordinary `setup.exe` entry point with clear install/launch/error UI. Setup starts the visible companion; users can launch it normally and deliberately quit it. A running companion must register a tray icon, handle shell restart and expose status/Open Manager/Quit behavior. Windows may place registered icons in tray overflow; that is not permission for the app to omit its icon. Do not run a hidden continuing engine if tray startup fails.

### Companion owns the engine; browser bridge does not

A per-user Rust companion with one state/engine owner persists across Firefox restart. This lifetime is our engineering interpretation of the requested flow, not an additional owner request for a service or unattended Windows startup. No service, elevation, silent logon autorun or scheduled task is authorized.

The Firefox Native Messaging process becomes a bounded bridge to the companion. Specify authenticated/local-user-confined IPC, version pairing, singleton ownership, command correlation/idempotency and graceful quit/checkpoint behavior before implementation. Do not detach today's helper on EOF, run a second engine over the same state, or introduce an unauthenticated loopback HTTP control endpoint. Current helper lifecycle protections explain why simply hiding/detaching it was previously rejected in ADR0003.

Keep Firefox capture, controls and display, and Rust networking/scheduling/storage/validation/recovery. Native tray/install UI supplements these boundaries; it is not a new network engine in JavaScript.

### Ordinary download clicks, not every request

Automatic capture means a supported HTTP(S) file download initiated by an ordinary click is added and started using saved settings. It is enabled after ordinary installation/permission approval unless the user turns it off; fresh-install acceptance must not quietly add a manual enable/configuration step. It does not mean hijacking page navigation, replaying POST bodies as GETs, cancelling all download events, or collecting cookies/authorization implicitly. GGUF is an opaque downloadable file type, not a media-extraction feature; its actual provider/URL is unknown.

#49 must prove the browser classification, permission and handoff mechanism before #51 selects it. Firefox response-time blocking promises are a candidate, not an already proven product design. A native prepare/accept/browser-cancel/commit or equivalent contract needs bounded waits and stable receipts so failure, restart, duplicate events and lost acknowledgements do not create duplicate tasks/files or silently lose the click. Existing v2 Add acknowledgement alone does not establish that cross-process contract.

Unsupported/unsafe cases remain in Firefox with clear feedback. Test GET downloads with redirects, query/signed targets and attachment responses without filename extensions, as well as ordinary navigation. Private/container/POST/blob/session-bound cases must not be guessed into the supported set. Preserve the existing explicit opt-in session path; automatic capture is not authorization to harvest or persist credentials. Already transmitted network bytes cannot be retracted; claim one owned task/output, not zero probe/overlapping in-flight network traffic.

### Signing versus listing

No public AMO listing is required. Mozilla signing and marketplace listing are separate operations: unlisted signing requires submission to Mozilla but does not create a publicly discoverable/installable AMO listing. Developer Edition can already be configured to accept persistent unsigned installations. In that case, accepting an unsigned XPI need not require a new settings change. Existing configuration, changes to configuration, artifact signing and observed persistence are separate facts; do not infer one from another or from the `.xpi` file extension. This compatibility distinction does not remove the signed-artifact requirement in this project's current release gates.

### Persistent installation requires a real distribution path

Use Mozilla-signed self-distribution or another deliberately reviewed Mozilla-supported persistent path with signing protection unchanged. Do not use temporary-addon reloads, profile injection, enterprise-policy bypasses or `xpinstall.signatures.required` changes as the solution.

Mozilla's documentation describes AMO signing for self-distributed extensions, publisher account/submission requirements and possible review. Where Developer Edition currently enforces signatures, enabling its unsigned-install exception would require a settings change, which remains disallowed here. That conditional statement must not be generalized to an installation already configured to accept unsigned extensions. An AMO account, credentials, submission approval and a signed artifact are **not established** by the owner's feedback. No account creation/submission/login/profile access is performed by this decision. Surface the smallest necessary owner action when needed; never request secrets in issues/logs.

The actual returned signed XPI is a new artifact input. Review signing-added metadata, source/payload correspondence, package validators/notices and signatures, then test that exact XPI through normal persistent installation and restart. Reproducible pre-sign payload builds are distinct from reproducibility of a third-party signing service. Do not substitute unsigned temporary-XPI evidence.

## Execution and release gates

- #48: reconcile intent, history, vocabulary, plan and acceptance contract (this decision).
- #49: actual Firefox capture/persistent-install feasibility and signing prerequisites.
- #50: visible companion, bounded authenticated bridge and ordinary setup UI; may proceed independently of #49 after #48.
- #51: production automatic capture, dependent on #49/#50.
- #52: exact signed distribution and concise install instructions, dependent on #49/#50/#51.
- #53: exact final-main qualification and new version/tag/publication, dependent on merged implementation and successful main CI, following ADR0012's ordering.

Final qualification must execute setup → real tray → normal signed XPI install → Firefox restart → ordinary GGUF click → one automatically started native task → independent correct output. No temporary-addon API, manual Add/context-menu action or mocked transport may stand in for this path. Recheck setup/IPC lifecycle, shell restart, failure/uncertain handoff, existing adversaries, privacy and resource boundaries. A new release (working target0.2.0, our versioning choice) is not approved until these gates pass.

The owner is now using Firefox. Earlier closed-browser permission was consumed by completed v0.1.0 qualification; it is not current mutation authority. At this checkpoint fresh consent was required; the owner's later explicit [standing authority in ADR0014](0014-autonomous-completion.md) supersedes that approval requirement. Actual browser/registration tests still require closed-app/ownership preflights and isolated state. No normal profile inspection/modification, unowned process termination, security downgrade, VPN behavior, telemetry, remote updater, private-archive import or manufactured clean-machine evidence is permitted.

## Sources and knowledge status

Reviewed2026-09-10:

- [Mozilla temporary installation](https://extensionworkshop.com/documentation/develop/temporary-installation-in-firefox/): development loading is not the requested persistent user installation.
- [Mozilla signing/distribution](https://extensionworkshop.com/documentation/publish/signing-and-distribution-overview/): signed self-distribution, AMO account/submission/review, and the unsigned-install preference exception we will not use.
- [Firefox onHeadersReceived](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/API/webRequest/onHeadersReceived): Firefox can return a blocking response promise with the required permission. This documents API availability, not proof of safe request/download classification or end-to-end handoff.

Open: signing authority/approval, actual capture correlation and minimal permissions, exact tray/IPC dependency and license choices, API/protocol migration and the owner's specific GGUF site. Settled: the requested five-step experience and unchanged correctness/privacy/protection boundaries. Administrative issue sequencing and independent companion lifetime are our implementation choices. The direction can be revised explicitly after feasibility evidence; unsafe fallbacks cannot be silently used to claim acceptance.
