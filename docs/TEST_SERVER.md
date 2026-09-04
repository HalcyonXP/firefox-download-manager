# Deterministic adversarial HTTP server

The `download-manager-test-server` crate provides local HTTP fixtures for downloader integration tests. It binds only to an ephemeral `127.0.0.1` port, has no production dependency, performs no outbound request, and needs no internet access.

## Reproducible payload

A fixture is defined by a byte length and one-byte seed. Generation `g` at offset `o` is:

```text
(o * 31 + (o >> 8) * 17 + seed + g * 13) mod 251
```

All arithmetic is wrapping unsigned 64-bit arithmetic before the final modulus. Tests can independently compute any byte without storing a golden body. Generation zero is the stable default; mutation routes increment the generation deterministically by request ordinal.

## Built-in routes

All body routes support a single explicit `Range: bytes=start-end` unless the scenario intentionally violates that behavior.

| Path | Deterministic behavior |
| --- | --- |
| `/fixture` | Correct `200`, or `206` and exact `Content-Range` |
| `/ignore-range` | Ignores a range and returns the full body with `200` |
| `/bad-range/start` | Advertises a start one byte too high |
| `/bad-range/end` | Advertises an end one byte too high |
| `/bad-range/total` | Advertises a total one byte too high |
| `/validators/missing` | Omits ETag and Last-Modified |
| `/validators/changing` | Changes validators per request while retaining body bytes |
| `/mutating` | Changes validators and body generation per request |
| `/redirect/once` | Redirects to `/fixture` |
| `/redirect/loop-a` | Redirects to `/redirect/loop-b` |
| `/redirect/loop-b` | Redirects to `/redirect/loop-a` |
| `/disconnect` | Declares the full length, writes 17 bytes, then closes |
| `/stall` | Waits 100 ms before writing the body |
| `/status/403` | Returns `403` |
| `/status/404` | Returns `404` |
| `/status/416` | Returns `416` with `Content-Range: bytes */total` |
| `/status/429` | Returns `429` with `Retry-After: 2` |
| `/status/503` | Returns `503` with `Retry-After: 1` |
| `/unknown-length` | Omits `Content-Length` and delimits the body by close |
| `/encoded` | Unexpectedly labels raw fixture bytes as gzip encoded |

Unknown paths return `404`. Out-of-bounds ranges return a correct `416`. Multiple, suffix, open-ended, duplicate, or malformed Range headers are rejected instead of being guessed.

## Targeted faults

Tests can add ordered `FaultRule` values to `ServerConfig`. `RequestSelector` can match any combination of:

- exact route path;
- one-based request ordinal for that path; and
- exact inclusive byte range.

The first matching custom rule overrides a built-in route. Available faults cover ignored/malformed ranges, omitted/changing validators, body mutation, redirects, selected statuses and retry guidance, premature disconnects, bounded stalls, unexpected encoding, and unknown lengths. `TestServer::requests` returns non-sensitive observed path/ordinal/range metadata so a test can verify which request triggered a fault.

## Usage

```rust
use download_manager_test_server::{ByteRange, ServerConfig, TestServer};

let server = TestServer::start(ServerConfig::default())?;
let url = server.url("/fixture");
let assignment = ByteRange::new(0, 1023)?;
```

The server stops when its guard is dropped. Each instance has isolated counters and configuration, so tests do not rely on execution order or shared ports.

Run all fixture conformance tests with:

```powershell
cargo test -p download-manager-test-server --all-features --locked
```

The workspace CI command runs these tests automatically on Windows. Later engine integration tests should instantiate this crate directly rather than depend on public servers.
