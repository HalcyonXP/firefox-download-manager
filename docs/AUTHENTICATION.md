# Minimal session handoff (#23)

## Goal and boundary

Support an explicitly selected direct HTTP(S) download using the user's ordinary Firefox session, without cloning arbitrary browser requests or circumventing access controls. The concrete default-store-only scope below is an implementation decision under the existing issue, not a separately confirmed user preference. Wire protocol stays **v2**; the helper advertises `authenticated_requests`. An older helper must never receive session fields.

## Using it

1. Sign in normally in a non-private, default-store Firefox tab. Open the manager from a link or paste the exact direct URL.
2. Expand **Optional session handoff** and check the per-download option. It starts unchecked and resets after submission.
3. Optionally enter an exact **same-origin referrer** or an **HTTPS-only Basic/Bearer Authorization** value. No browser Authorization harvesting, content scripts, request cloning, or login extraction occurs.
4. Submit and approve Firefox's optional cookies/site permission. Only eligible cookies for the target URL are read. HttpOnly cookies come from the privileged cookies API, not the document.
5. On `AUTH_REQUIRED` or `AUTH_EXPIRED`, sign in again and explicitly add a **fresh task** from the original direct URL. Remove the old task separately if its retained partial is no longer wanted. Protocol v2 cannot refresh credentials on `resume`; new credentials never mutate an existing partial's identity.

Firefox host permission patterns grant access to a **scheme/host across all its ports**, not one path or TCP port. The extension requests only the selected scheme/host, never a wildcard subdomain. The helper independently confines context to the exact scheme/host/port origin. The revoke button removes the extension's optional cookie/site grants. Grants otherwise persist, but collection still requires opt-in on each download. Revocation does not retract an already submitted helper context: cancel that task separately.

## Supported semantics and deliberate limits

- Only the normal `firefox-default` store is supported. Source-tab context is retained for link capture; an unavailable/ambiguous source, private tab, or container is rejected, never substituted with another session.
- First-party-isolated and partitioned cookies are excluded. This is not general browser-session replication; Firefox Total Cookie Protection can make some authenticated sites unsupported. IPv6 site-permission handoff is conservatively unsupported by the UI; ordinary unauthenticated HTTP(S) capture is unchanged.
- Both peers enforce host/domain, path-boundary, Secure, and expiry eligibility. The reserved cookie domain field uses a leading dot for Domain cookies and no leading dot for host-only cookies. The helper also enforces HTTP cookie/header grammar and an aggregate 8 KiB outgoing context-header bound. The extension bounds serialized context to 64 KiB and both peers cap cookies at 256.
- Cookie expiry is checked before **every** request, not just creation. Expiry fails the task rather than silently sending a reduced session. `time` provides reviewed RFC 3339/UTC parsing; offsets are respected.
- Non-Secure cookies may be sent over an explicitly selected HTTP target; the UI warns that the network can observe them. Authorization is HTTPS-only. TLS validation is never disabled.
- Every authenticated origin-changing redirect is rejected **before contacting the next origin**, including a port change. Same-origin redirects re-evaluate path and expiry. Downgrades are forbidden for all probes; transfer requests do not follow redirects at all. Automatic referrer generation is disabled.
- There is no Set-Cookie jar or response-session update. Only the initially handed-off eligible cookies are available. SameSite/browser navigation semantics are not cloned.
- Signed query encoding, ordering, and duplicate keys are not reconstructed. Tests assert exact fixture request targets through both probes and all transfer modes.

## Memory and recovery

Cookie/Authorization/referrer values are not serializable engine state, are redacted from Debug, and never enter ordinary task snapshots, diagnostics, errors, or completion events. Context persists in memory during pause and automatic retry. Terminal completion, cancellation, or failure releases managed context/probe ownership; helper shutdown loses all secrets. This is memory-only handling, **not a secure-erasure guarantee** against process inspection or operating-system dumps.

Internal task format **v3** requires a non-secret `needs_session` marker. Strict v1/v2 migrations set it false because those formats never accepted session data. A recovered marked task cannot start a network probe or reuse retained bytes without its lost context. Missing v3 markers fail validation. Exact original/final URLs remain protected recovery metadata, as before; this can include signed query data.

With context supplied, HTTP 401/403 means the supplied session is no longer accepted (`AUTH_EXPIRED`). Without context, 401 maps to `AUTH_REQUIRED`; an ordinary unauthenticated 403 remains `HTTP_STATUS` rather than asserting that an absent session expired. Existing partial-retention settings govern terminal failures.

## Evidence and remaining qualification

Rust tests exercise header grammar, expiry/UTC boundaries, redacted Debug, scoped redirects with a second origin that observes zero requests, signed probes, native-host dispatch, 1/2/4/8-worker output, safe single fallback, failed-session fresh-task flow, and paused helper recovery without secrets. The test server records only boolean session assertions against fixed synthetic values, never received header values.

Extension tests cover explicit opt-in, permission denial, default-store enforcement, HttpOnly/host-only/Domain/Secure/path/expiry eligibility, unsupported contexts, same-origin referrers, HTTPS Authorization, and capability gating. These tests alone do **not** prove Firefox's permission prompt, Native Messaging transport, or release installation. Browser qualification evidence is recorded separately as it is obtained; #26–#28 still gate a release.
