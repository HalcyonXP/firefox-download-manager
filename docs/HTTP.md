# HTTP probing and range-validation policy

The Rust engine's `network` module determines whether a direct HTTP(S) resource can be segmented. No worker may write response bytes until this policy validates its status and metadata.

## URL and redirect boundary

- Only absolute `http:` and `https:` URLs are accepted.
- URL user-info is rejected; fragments are removed because they are not HTTP request data.
- TLS certificate verification uses maintained Rustls platform-verifier defaults and is never disabled.
- Redirects are followed at most ten times.
- Redirect targets must remain HTTP(S).
- HTTPS-to-HTTP downgrade is rejected.
- Unauthenticated cross-origin redirects are allowed in the MVP.
- Credentials are not accepted by this implementation. The later authenticated-download policy must strip them on unrelated origins before enabling its protocol capability.
- Redirect loops, excessive depth, unsupported targets, and stopped redirects fail explicitly.

Neither user-provided URLs nor redirect locations are included in ordinary error display text or the custom `Debug` representation of a probe.

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

Singleton headers are rejected when duplicated. Numeric fields accept decimal ASCII only with checked `u64` parsing. Metadata values are bounded and control characters are rejected. Last-Modified must parse as an HTTP date. ETags retain weak/strong identity and opaque contents rather than being normalized.

Filename parsing is not filename sanitization. The storage boundary in issue #6 must still reject traversal, device names, alternate streams, and other Windows hazards.

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

## Fixed-concurrency transfer

For a proven known-size resource, the scheduler subtracts durable completed coverage and lazily divides only the missing gaps into large requests. Ordinary assignments are between 1 MiB and 8 MiB, subject to a smaller final fragment. This limits request overhead while bounding each response buffer. The request plan is rejected if it would exceed 1,000,000 assignments.

Supported task worker counts are exactly 1, 2, 4, and 8. Four is the default and eight is the initial cap. Workers pull from one synchronized gap queue, so faster workers continue with unassigned bytes without creating overlap. After all other work is complete, one idle worker may duplicate the sole stalled tail request after a bounded delay. Both responses are validated and buffered independently, but only the first complete response can claim the storage assignment; the loser is cancelled and can never write.

Every worker sends `Accept-Encoding: identity`. It also sends `If-Range` with a probed strong ETag, or with Last-Modified when no strong ETag is available. Redirect following is disabled for transfer requests: the response URL must remain the exact probed final URL. Status, range, total, declared length, encoding, validators, and exact EOF are revalidated before the buffered assignment is passed to storage.

A shared global semaphore and one semaphore per URL origin independently limit concurrent requests across tasks. The defaults are 16 globally and 8 per origin; validated configuration caps them at 32 and 8 respectively. Per-task workers remain capped at eight regardless of those aggregate limits.

## Safe fallback

If the initial ranged request is ignored with `200`, the probe drops that response without consuming the full body and selects the single-stream path. The scheduler issues a fresh request with one worker and uses the same partial-file lifecycle and transfer summary as segmented work. That response must remain at the exact probed URL, return `200`, use identity encoding, preserve accepted validators, and reach exact EOF at any declared or previously known length.

A stream with no declared length grows only through a sole sequential writer under a configured byte limit (64 GiB by default and never above 16 TiB). Clean EOF seals its actual size and exact coverage. A request failure or restart before EOF retains no completed range and the next attempt truncates the managed partial before restarting at byte zero. The helper never infers a resumable total from interrupted bytes.

A malformed `206`, transformed response, contradictory total, unstable validator, premature body, or resource mutation is an error—not a fallback opportunity—because accepting it could mix inconsistent bytes.

## Resource and server impact

Client connect and whole-request timeouts are bounded. The HTTP library's automatic content decompression features are disabled. Probe responses are streamed and capped by their one-byte assignment; ranged bodies are capped by their 8 MiB assignment; undeclared sequential bodies are capped explicitly. One tail assignment can have at most two active attempts. HTTP status and bounded `Retry-After` data are preserved for the retry policy in issue #9, but issue #8 does not retry failed requests automatically.

The deterministic fixtures and integration matrix are documented in [TEST_SERVER.md](TEST_SERVER.md).
