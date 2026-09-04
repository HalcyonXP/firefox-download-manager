# Security and privacy model

Status: accepted baseline for the initial local release

Last updated: 2026-09-04

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

All remote responses are untrusted. The helper uses maintained TLS defaults, does not disable certificate validation, and does not implement custom VPN or route behavior. Redirect count and schemes are bounded. Credentials are not forwarded across an origin change unless a later, explicit policy proves the destination is eligible.

A range is writable only when status is `206` and a parsed `Content-Range` exactly matches the assignment and known total. Unexpected encoding, body length, validators, total size, or final URL causes revalidation, safe fallback before segmented bytes are accepted, or failure. Already mixed output is never reclassified as safe.

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

Promotion occurs only after validation. If the filesystem cannot provide an atomic same-volume rename, the helper must use a documented safe alternative or fail; it must not expose a partially copied final file as completed.

### Persisted state to restarted helper

Metadata is versioned but untrusted. Recovery validates identifiers, enum values, size arithmetic, range ordering and coverage, paths, partial-file identity/length, and resource validators. Unknown future formats and corrupt state fail closed. Secret headers and cookies are not persisted by default.

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

Authenticated downloads are intentionally deferred until issue #15. Only cookies applicable to the exact target URL may be transferred, honoring domain, path, expiry, `Secure`, and `HttpOnly` semantics. Authorization data remains memory-only. Redirects to unrelated origins strip credentials and require explicit reauthorization.

## Resource identity and corruption resistance

The helper records the expected size, final URL, and available strong validators. `If-Range` is used where appropriate. A conflicting ETag, Last-Modified value, total size, or redirect identity blocks unsafe resume. Weak or absent validators follow a documented conservative policy and never justify combining bytes known to differ.

Completed coverage is represented canonically as ordered, non-overlapping, in-bounds ranges. Arithmetic is checked. Storage completion is recorded only after the corresponding bytes have been written and required flush policy succeeds. Final validation independently checks coverage and byte count; an optional user-provided SHA-256 digest is streamed from disk.

## Availability and server-impact controls

- Per-task worker count is restricted to 1, 2, 4, or 8.
- Per-host and global concurrency caps are mandatory.
- Retries are bounded and use exponential backoff with jitter.
- `Retry-After` is honored for applicable responses.
- Repeated failures reduce pressure rather than creating worker storms.
- Response bodies, headers, protocol frames, metadata, logs, event frequency, redirect depth, and timeouts are bounded.
- Cancellation and shutdown stop new assignments before waiting for writers to checkpoint.

These controls reduce accidental denial of service but do not promise availability against an adversarial server or exhausted local disk.

## Native installation and update boundary

The native-host manifest names one absolute helper path and allows only this extension's stable ID. Installation and removal modify only required user-scoped registration where possible, quote paths containing spaces correctly, and embed no secrets. The project has no remote updater. Release artifacts and checksums come from GitHub; update behavior is explicit and local.

Task-state migrations are versioned and tested. An incompatible upgrade preserves data for diagnosis or explicit cleanup rather than guessing.

## Third-party and supply-chain policy

Only dependencies needed for scoped behavior may be introduced. Versions and lockfiles are committed where appropriate. CI must support license/notices and vulnerability auditing. Source copied from another project requires prior license compatibility review, attribution, and a documented reason; design inspiration alone is recorded without copying implementation. See [ADR-0006](decisions/0006-third-party-code.md).

## Required security tests

Later issues must provide regression coverage for:

- malformed, oversized, and truncated Native Messaging frames;
- unknown protocol versions and commands;
- malformed/ignored ranges, changing validators, and resource mutation;
- short, overlapping, duplicate, and out-of-bounds writes;
- traversal, Windows reserved names, alternate streams, links, collisions, and paths with spaces;
- redirect loops and cross-origin credential stripping;
- corrupt/incompatible recovery metadata and killed-helper recovery;
- bounded concurrency, retries, logs, and progress events; and
- inspection of logs/state for credentials and sensitive query values.

## Explicit exclusions

The manager does not claim to hide network activity from the operating system, VPN provider, ISP, or destination server. It does not scan downloaded content, bypass endpoint security, enforce download licensing, or protect against a fully compromised Windows account. These exclusions do not relax byte-integrity, least-privilege, or secret-handling requirements.
