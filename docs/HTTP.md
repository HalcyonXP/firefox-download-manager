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

## Safe fallback

If the initial ranged request is ignored with `200`, the probe drops that response without consuming the full body and selects the single-stream path. It retains a trustworthy Content-Length when present or explicitly records unknown length. The eventual single-stream implementation must issue a fresh request and use the same storage/progress model.

A malformed `206`, transformed response, contradictory total, unstable validator, premature probe body, or resource mutation is an error—not a fallback opportunity—because accepting it could mix inconsistent bytes.

## Resource and server impact

Client connect and whole-request timeouts are bounded. The HTTP library's automatic content decompression features are disabled. Probe responses are streamed and capped by their one-byte assignment rather than buffered without limit. Scheduling, retries, and global/per-host concurrency are implemented by later issues without weakening these checks.

The deterministic fixtures and integration matrix are documented in [TEST_SERVER.md](TEST_SERVER.md).
