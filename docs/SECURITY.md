# Security and privacy model

Status: accepted baseline for the initial local release

Last updated: 2026-09-08

## Security goals

The manager must deliver only validated bytes to a collision-safe destination, recover conservatively, expose minimal browser authority to the native helper, and avoid leaking credentials or sensitive URLs. It is local software running with the interactive user's privileges; it is not a sandbox or malware scanner.

## Protected assets

- correctness and identity of downloaded bytes;
- existing user files and filesystem locations;
- browser cookies, authorization values, referrers, and signed URLs;
- local task history, paths, and diagnostics;
- availability of the browser, helper, network, and disk; and
- integrity of the extension, native host registration, and helper binary.

## Threat assumptions

Potentially hostile inputs include web-page URLs and metadata, redirects and DNS results, HTTP headers and bodies, filenames, native messages, extension settings, persisted state, and filesystem conditions. A remote server may ignore ranges, lie about offsets or sizes, mutate a resource, disconnect, stall, compress unexpectedly, redirect repeatedly, or rate-limit requests.

The operating system account and installed extension/helper are trusted to the degree required to run local software. An attacker already able to replace the helper binary, alter the native-host registry under the user account, or arbitrarily modify the process has capabilities outside this product's defensive boundary. The manager still validates persisted state and fails safely after ordinary corruption or tampering.

## Trust boundaries

### Web content to extension

A page may supply deceptive URLs, suggested names, or link text. Only an explicit user action creates a task. The extension accepts only `http:` and `https:` targets, does not execute page-provided code, and treats displayed metadata as untrusted. Unsupported schemes and malformed URLs are rejected before Native Messaging and again by the helper.

### Extension to native helper

Possession of the extension ID is not treated as proof that a message is safe. The native-host manifest allows only the fixed extension ID, while the helper independently enforces frame limits, JSON shape, protocol version, command type, field bounds, URL scheme, path policy, and state transitions. File bytes never cross Native Messaging.

Malformed JSON, truncated framing, unexpected EOF, duplicate correlations, unknown commands/versions, and oversized messages produce bounded failures without panics or partial commands.

### Helper to network

All remote responses are untrusted. The helper uses maintained TLS defaults, does not disable certificate validation, and does not implement custom VPN or route behavior. Redirect count and schemes are bounded. Context-bearing origin-changing redirects are rejected before contacting the next origin; same-origin hops re-evaluate cookie path and expiry.

A range is writable only when status is `206` and a parsed `Content-Range` exactly matches the assignment and known total. Each response is buffered only to its bounded assignment and revalidates encoding, body length, validators, total size, and exact final URL before acquiring storage ownership. A malformed worker response fails; fallback is selected only by the initial safe probe, never after segmented bytes are accepted. Already mixed output is never reclassified as safe.

### Helper to filesystem

All paths and filenames are untrusted. The helper:

- accepts a configured destination directory, not an arbitrary final-file write command;
- validates and normalizes paths using Windows-aware APIs;
- rejects traversal, absolute filename components, device paths, alternate data streams, control characters, trailing dots/spaces, and reserved device names;
- confines temporary metadata to the application state root and partial bodies to the selected destination;
- uses create-new semantics and collision-safe names;
- prevents links/reparse points and path replacement from silently redirecting critical writes where practical;
- bounds every random-access write to its assignment and expected file size;
- reports disk-full, access-denied, sharing, and lock failures; and
- never overwrites an existing final file silently.

Promotion occurs only after validation. If the filesystem cannot provide an atomic, create-new, same-volume publication primitive, the helper must use a documented safe alternative or fail; it must not expose a partially copied final file as completed. The initial implementation publishes a flushed complete file through a same-directory hard link, checkpoints both names, and only then removes and checkpoints the redundant partial name.

### Persisted state to restarted helper

Metadata is versioned but untrusted. Recovery validates identifiers, enum values, the persisted 1/2/4/8 worker selection, size arithmetic, range ordering and coverage, paths, partial/final file type and length, same-file publication identity, and resource validators. Formats v1/v2/v3 are accepted only through dedicated migration into strict v4, preserving session requirements and supplying no checksum only for historical formats that could not accept one; unknown future formats and corrupt state fail closed. Secret headers and cookies are not persisted by default. Concrete schema, checkpoint ordering, recovery bounds, and cleanup behavior are documented in [STATE.md](STATE.md).

## Sensitive-data policy

### Classification

| Data | Persistence | Ordinary logs | Protocol snapshots |
| --- | --- | --- | --- |
| Cookies / authorization values | Memory only | Never | Never |
| URL user-info | Rejected | Never | Never |
| Signed or sensitive query values | Only when required to resume the exact task | Redacted | Redacted by default |
| Original/final URL without sensitive rendering | Required task state | Origin plus opaque task ID at most | Available only where UI function requires it |
| Referrer | Memory only by default; persistence requires a later explicit decision | Redacted | Omitted by default |
| Destination path / filename | Required task state | Minimized or redacted in ordinary mode | Included where user control requires it |
| ETag / Last-Modified / size | Required task state | Safe structured values with bounds | Included in detailed task data as needed |

URL user-info (`https://user:pass@host/`) is rejected rather than normalized. Logs must use structured redaction before formatting; redaction after a string has entered a log pipeline is insufficient. Panic/error chains and HTTP-client tracing must be reviewed so they cannot bypass redaction. Verbose diagnostics are opt-in, local, bounded, and still exclude credentials.

Authenticated handoff is implemented in #23 with per-download opt-in, optional cookie/site grants, normal default-store scope, memory-only secrets, and explicit fresh-task retry after expiry/restart. Private/container sources and unsupported partitioned/first-party-isolated contexts are not silently borrowed. See [AUTHENTICATION.md](AUTHENTICATION.md) and [ADR 0010](decisions/0010-minimal-session-handoff.md) for supported semantics, permission granularity, recovery marker, and known limits.

## Resource identity and corruption resistance

The helper records the expected size, final URL, and available strong validators. `If-Range` is used where appropriate. A conflicting ETag, Last-Modified value, total size, or redirect identity blocks unsafe resume. Weak or absent validators follow a documented conservative policy and never justify combining bytes known to differ.

Completed coverage is represented canonically as ordered, non-overlapping, in-bounds ranges. Arithmetic is checked. Storage completion is recorded only after the corresponding bytes have been written and required flush policy succeeds. Final validation independently checks coverage and byte count; an optional user-provided SHA-256 digest is streamed from disk.

## Availability and server-impact controls

- Per-task worker count is restricted to 1, 2, 4, or 8, with four default and eight maximum.
- HTTP admission limits are independent and shared across all probes, redirected probe hops, and transfer requests. #24 adds shared origin cooldown and adaptive pressure; see [RELIABILITY.md](RELIABILITY.md).
- Ranged bodies are limited to 8 MiB per assignment and plans to 1,000,000 requests.
- Tail duplication is off by default. Explicit engine opt-in can hedge only the sole remaining tail, at most once; only one validated response can own its range.
- Undeclared-length streams have an explicit byte cap and restart from zero after interruption.
- One run has zero through 20 retries after its initial attempt; retries use capped exponential equal jitter.
- Only transport failures and HTTP `408`, `425`, `429`, `500`, `502`, `503`, and `504` are automatic retry candidates.
- `Retry-After` is a minimum delay; guidance above the accepted bound fails instead of retrying early.
- Protocol, resource-identity, and storage failures stop the run rather than creating worker storms.
- Response bodies, headers, protocol frames, metadata, logs, event frequency, redirect depth, and timeouts are bounded.
- Pause/cancel interrupts probe, semaphore, request, body, tail, and backoff waits, joins workers, then checkpoints bytes before state.
- Cancellation's explicit `delete` policy can remove only its validated managed partial; completed final output is never eligible.
- Progress events are interval-limited and coalescible; a bounded overflow flag requires an authoritative snapshot.

These controls reduce accidental denial of service but do not promise availability against an adversarial server or exhausted local disk.

## Native installation and update boundary

The source native-host manifest allows only `download-manager@halcyonxp.local`; installation substitutes one JSON-escaped absolute executable path and retains only Firefox's supported manifest fields. The extension requests `nativeMessaging` and `menus`, with no host origins. `install-native-host.ps1` copies a verified executable beneath the current user's local application data and writes only `HKCU\Software\Mozilla\NativeMessagingHosts\com.halcyonxp.firefox_download_manager`; it creates no service, listener, scheduled task, firewall rule, VPN setting, or route. The companion uninstaller refuses to delete a registration pointing at a different manifest, removes only known generated host files, and never recursively removes task state or destination content. Windows CI installs into a path containing spaces, launches the registered binary, negotiates, receives a snapshot, and removes the registration.

The project has no remote updater. Release artifacts and checksums come from GitHub; update behavior is explicit and local. A packaging/upgrade design beyond these development registration scripts remains release work.

Task-state migrations are versioned and tested. The v1-to-v2 worker-default migration uses a separate strict decoder and atomic replacement. An incompatible upgrade preserves data for diagnosis or explicit cleanup rather than guessing.

## Third-party and supply-chain policy

Only dependencies needed for scoped behavior may be introduced. Versions and lockfiles are committed where appropriate. CI must support license/notices and vulnerability auditing. Source copied from another project requires prior license compatibility review, attribution, and a documented reason; design inspiration alone is recorded without copying implementation. See [ADR-0006](decisions/0006-third-party-code.md).

## Required security tests

Current regression coverage includes partial, malformed, duplicate-member, oversized, and truncated Native Messaging frames; unknown versions/commands; mandatory hello negotiation; path-with-spaces HKCU launch; initial/reconnect snapshots; clean-EOF cooperative shutdown; and the HTTP/storage/state cases implemented to date. Remaining issues extend coverage for:

- full browser UI command/reconnect behavior and helper-unavailable presentation;
- malformed/ignored ranges, changing validators, and resource mutation;
- short, overlapping, duplicate, and out-of-bounds writes;
- traversal, Windows reserved names, alternate streams, links, collisions, and paths with spaces;
- redirect loops and cross-origin credential stripping;
- corrupt/incompatible recovery metadata and killed-helper recovery;
- bounded concurrency, retries, logs, and progress events; and
- inspection of logs/state for credentials and sensitive query values.

## Explicit exclusions

The manager does not claim to hide network activity from the operating system, VPN provider, ISP, or destination server. It does not scan downloaded content, bypass endpoint security, enforce download licensing, or protect against a fully compromised Windows account. These exclusions do not relax byte-integrity, least-privilege, or secret-handling requirements.

## Optional checksum boundary (#25)

Supplied SHA-256 expectations are immutable task inputs, validated before networking and required in v4 recovery shape. Streaming validation reads the owned complete partial, then retains a non-cloneable lease through no-overwrite promotion; mismatch cannot publish output or success. The failure-retention setting applies explicitly. Cancellation joins hashing before acknowledgement. Windows file locks resist ordinary competing I/O, not malicious same-user or memory-mapped mutation/all namespace races. Published files are not continuously rehashed. These limits and test boundaries are explicit in [INTEGRITY.md](INTEGRITY.md); #26 still requires the overall security review.
