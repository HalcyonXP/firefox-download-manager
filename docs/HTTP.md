# HTTP probing and range-validation policy

The Rust engine's `network` module determines whether a direct HTTP(S) resource can be segmented. No worker may write response bytes until this policy validates its status and metadata.

## URL and redirect boundary

- Only absolute `http:` and `https:` URLs are accepted.
- URL user-info is rejected; fragments are removed because they are not HTTP request data.
- TLS certificate verification uses maintained Rustls platform-verifier defaults and is never disabled.
- Ordinary initial probes follow redirects at most ten times.
- Redirect targets must remain HTTP(S).
- HTTPS-to-HTTP downgrade is rejected.
- Unauthenticated cross-origin redirects are allowed in the MVP.
- Optional [session handoff](AUTHENTICATION.md) is origin-confined. Context-bearing cross-origin redirects are rejected before contact; same-origin redirects re-evaluate cookie eligibility. Transfer requests never redirect.
- Redirect loops, excessive depth, unsupported targets, and stopped redirects fail explicitly.

Neither user-provided URLs nor redirect locations are included in ordinary error display text or the custom `Debug` representation of a probe.

## Opt-in anonymous no-redirect probe

`ProbeClient::probe_anonymous_without_redirects` refuses redirects from the initial request as well as the final-byte verification, before requesting the redirect target. It accepts no session argument. This shares the ordinary URL/TLS/admission/status/header/body/validator checks; ordinary probes retain their existing bounded redirect policy and transfer requests already refuse redirects.

This unselected interface supports a future protection binding starting from the browser's captured final request URL: an empty native redirect chain must be enforced, not invented from a final-URL-only receipt. No task, retry/reprobe or recovery path selects this mode yet; all such paths must preserve the required policy before that inference becomes valid. It does not establish browser provenance, reputation or publication authority. Three owned loopback cases independently check no target contact at initial/final-boundary redirects, exact direct boundaries, malformed-range refusal and unchanged ordinary following; three executed/rejected mutations cover both refusal flags and ordinary compatibility. Fixture listeners are retired before final assertions.

## Probe sequence

The client sends a small `GET` with:

```http
Range: bytes=0-0
Accept-Encoding: identity
```

A `206` is considered evidence of range support only after exact status/header/body validation. For resources larger than one byte, the client then requests the declared final byte. This second one-byte request confirms the advertised total exists and that accepted validators remain stable. Thus a successful probe transfers two body bytes at most.

A correct `416` with `Content-Range: bytes */0` represents an empty resource. Other non-success statuses become bounded machine data, including numeric `Retry-After` guidance where valid.

## Parsed metadata

A successful probe records:

- final URL after redirects;
- known total size, or unknown size for a sequential fallback;
- valid ETag and Last-Modified values;
- an untrusted filename candidate from UTF-8 `filename*`, then `filename`, then the final URL path; and
- proven segmented, safe single-stream, or empty mode.

Singleton headers are rejected when duplicated. Numeric fields accept decimal ASCII only with checked `u64` parsing; a task rejects a resource size above the protocol's JavaScript exact-integer bound before creating storage. Metadata values are bounded and control characters are rejected. Last-Modified must parse as an HTTP date. ETags retain weak/strong identity and opaque contents rather than being normalized.

Filename parsing is not filename sanitization. The storage boundary independently rejects traversal, device names, alternate streams, and other Windows hazards.

## Exact ranged-response acceptance

For assignment `[start, end]` within a known `total`, every worker response must satisfy all of these before any body byte is writable:

1. Status is exactly `206`.
2. There is exactly one syntactically valid `Content-Range`.
3. Its start and inclusive end exactly match the assignment.
4. Its total exactly matches the accepted resource size.
5. `Content-Length`, when present, equals `end - start + 1`.
6. `Content-Encoding` is absent or exactly `identity`.
7. Every previously accepted ETag or Last-Modified validator is present and unchanged.
8. The streaming layer receives exactly the assigned number of bytes—never fewer or more.

Unknown totals, malformed numbers, duplicate singleton headers, missing known validators, unexpected transforms, and inconsistent boundaries fail closed. A `200` is never sliced or merged as a range.

## Configured concurrency and optional tail work

For a proven known-size resource with a strong ETag, the scheduler subtracts durable completed coverage and lazily divides only the missing gaps into large requests. Ordinary assignments are between 1 MiB and 8 MiB, subject to a smaller final fragment. This limits request overhead while bounding each response buffer. The request plan is rejected if it would exceed 1,000,000 assignments.

Supported task worker counts are exactly 1, 2, 4, and 8. Four is the default and eight is the initial cap. Workers pull from one synchronized gap queue, so faster workers continue with unassigned bytes without creating overlap. Tail duplication is off by default. With an explicit experimental engine opt-in, after all other work is complete one idle worker may duplicate the sole stalled tail request after a bounded delay. Both responses are validated and buffered independently, but only the first complete response can claim the storage assignment; the loser is cancelled and can never write.

Every worker sends `Accept-Encoding: identity`. It also sends `If-Range` with the probed strong ETag. Last-Modified is still compared when present, but no longer substitutes for strong byte identity. Redirect following is disabled for transfer requests: the response URL must remain the exact probed final URL. Status, range, total, declared length, encoding, validators, and exact EOF are revalidated before the buffered assignment is passed to storage.

Shared admission independently limits all concurrent HTTP requests across tasks, including both probe bytes and every redirected probe hop. Origin pressure can further reduce effective admission width. The defaults are 16 globally and 8 per origin; validated configuration caps them at 32 and 8 respectively. Per-task workers remain capped at eight regardless of those aggregate limits; transient transfer retries reduce effective width without changing the persisted selection. See [RELIABILITY.md](RELIABILITY.md) for shared 429/503 cooldown, minimum Retry-After deadlines, bounded worker-416 revalidation, and measured optional-tail policy.

## Strong resource identity and recovery (#22)

Weak or absent ETags (even with Last-Modified and a matching length) do not prove byte identity across requests. Such probes choose a fresh single stream. Nonempty completed coverage is never reused without a strong ETag, in both the task controller and scheduler boundary. Resume/retry compares final URL, size, transfer mode, ETag, and Last-Modified exactly. Known conflict or insufficient identity returns an explicit failure; remove the retained task/partial deliberately and create a new download rather than mixing generations. Empty interrupted single streams may restart from byte zero.

This is deliberately stricter than the original #13 policy. A strong ETag is an HTTP server promise, not a cryptographic checksum; a server lying consistently about its validator is outside what HTTP identity checks alone can detect. Optional user-supplied checksums remain #25.

## Safe fallback

If the initial ranged request is ignored with `200`, the probe drops that response without consuming the full body and selects the single-stream path. The scheduler issues a fresh request with one worker and uses the same partial-file lifecycle and transfer summary as segmented work. That response must remain at the exact probed URL, return `200`, use identity encoding, preserve accepted validators, and reach exact EOF at any declared or previously known length.

A stream with no declared length grows only through a sole sequential writer under a configured byte limit (64 GiB by default and never above 16 TiB). Clean EOF seals its actual size and exact coverage. A request failure or restart before EOF retains no completed range and the next attempt truncates the managed partial before restarting at byte zero. The helper never infers a resumable total from interrupted bytes.

A malformed `206`, transformed response, contradictory total, unstable validator, premature body, or resource mutation is an error—not a fallback opportunity—because accepting it could mix inconsistent bytes.

## Cooperative controls and retries

Each task run owns one cooperative cancellation signal covering probe requests, semaphore waits, request sends, response-body reads, tail coordination, and retry sleeps. A range worker checks the signal again before claiming storage. Pause/cancel acknowledgement waits until all workers join or finish an already-entered synchronous storage commit; the task controller then flushes and critically checkpoints completed coverage. Unfinished known-size assignments remain unclaimed, and interrupted unknown-length streams retain no resumable range.

A single retry budget spans probe and transfer attempts. Request/transport failures and HTTP `408`, `425`, `429`, `500`, `502`, `503`, and `504` are transient candidates. Each retry uses capped exponential equal jitter (a random delay from one-half through the current exponential ceiling). The default is five retries after the initial attempt; configuration permits zero through 20. Base delay is constrained to 10 ms–60 seconds, the exponential ceiling to at most ten minutes, and accepted `Retry-After` guidance to at most one hour.

A parsed delta-seconds or HTTP-date `Retry-After` is a minimum, even when it exceeds the jittered delay. Guidance above the configured bound exhausts the automatic decision instead of retrying early. Protocol/range metadata violations, resource identity changes, storage failures, and non-transient statuses are fatal for the current run. Exhaustion stops with a stable failure; only an explicit user resume/retry starts a fresh budget. Automatic attempts remain within the same accepted probe identity and every response revalidates it. Work resumed after pause/failure or restart first requires a fresh probe proving the same complete resource identity.

## Progress reporting

Scheduler metrics are absolute latest values: safe in-process bytes, expected size when known, and active requests. A bounded watch channel coalesces fast producer updates. The task controller samples with monotonic time and emits progress no more frequently than the configured 100 ms–60 second interval (250 ms by default), while complete current snapshots remain independently available.

Speed uses a bounded sliding window (five seconds by default, configurable from one through 60 seconds). ETA appears only for a known remaining size, a positive rate, and interval rates stable within a conservative 4× ratio. Unknown size, counter regression, zero/stalled rate, or instability suppresses ETA; exact completion reports zero. Pausing or entering retry backoff clears stale rate/ETA state.

## Resource and server impact

Client connect and whole-request timeouts are bounded. The HTTP library's automatic content decompression features are disabled. Probe responses are streamed and capped by their one-byte assignment; ranged bodies are capped by their 8 MiB assignment; undeclared sequential bodies are capped explicitly. One tail assignment can have at most two active attempts. HTTP status and bounded `Retry-After` data are preserved as path- and URL-free machine data for the implemented task retry policy.

The deterministic fixtures and integration matrix are documented in [TEST_SERVER.md](TEST_SERVER.md).
