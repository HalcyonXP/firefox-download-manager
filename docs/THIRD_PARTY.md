# Third-party dependency review

This record complements automated lockfile, license, advisory, ban, and source checks. It documents deliberate direct dependency choices and exceptional transitive licenses; generated release notices remain a packaging requirement.

## Runtime Rust dependencies

| Dependency | Purpose | Version policy | License | Rationale |
| --- | --- | --- | --- | --- |
| `bytes` | Reference-counted bounded chunks returned by the HTTP streaming API | Exact workspace pin | MIT | Lets cancellation-aware scheduler code consume Reqwest body chunks without copying at the API boundary; assignment and stream limits still bound retained data |
| `reqwest` | Bounded async HTTP client and maintained protocol/TLS integration | Exact workspace pin | MIT OR Apache-2.0 | Avoids custom HTTP/TLS parsing; default features and automatic decompression are disabled |
| `httpdate` | Strict HTTP date and Retry-After parsing | Exact workspace pin | MIT OR Apache-2.0 | Small standards-focused parser avoids locale/date ambiguity |
| `time` | RFC 3339 cookie-expiry parsing and UTC comparison | Exact 0.3.55 pin, std/parsing only | MIT OR Apache-2.0 | Reviewed metadata before incorporation; avoids a custom date parser, local-time lookup, or credential serialization |
| `sha2` | Streamed user-supplied SHA-256 validation | Exact 0.11.0 pin, default features disabled | MIT OR Apache-2.0 | Reviewed package metadata before incorporation; maintained RustCrypto digest implementation, no custom cryptographic code, OID, or allocation feature needed |
| `same-file` | Prove that retained partial and published final names are hard links to one file | Exact workspace pin | Unlicense OR MIT | Uses stable device/file identity rather than comparing paths, timestamps, or contents heuristically; required for conservative crash recovery on Windows |
| `serde` | Strict protocol and internal task-state encoding/decoding | Exact workspace pin with derive support | MIT OR Apache-2.0 | Maintained typed serialization avoids ad hoc field conversion; protocol payloads and persisted version-specific records deny unknown fields |
| `serde_json` | Bounded Native Messaging and task-state JSON | Exact workspace pin with default `std` only | MIT OR Apache-2.0 | Human-inspectable JSON with project size limits; protocol input additionally uses a duplicate-member-rejecting visitor before typed decoding |
| `thiserror` | Typed internal errors with safe project-controlled display text | Exact workspace pin | MIT OR Apache-2.0 | Keeps stable classifications separate from untrusted source text |
| `tokio` | Host runtime, async worker tasks, cancellation notifications, timers, channels, and concurrency semaphores | Exact workspace pin; direct crates use only the workspace's `macros`, `rt-multi-thread`, `sync`, and `time` feature set | MIT | Provides maintained bounded scheduling/session primitives; Native Messaging framing itself remains synchronous stdio and Reqwest supplies its required network features transitively |
| `uuid` | Opaque stable task identifiers | Exact workspace pin with v4 generation only | MIT OR Apache-2.0 | Produces canonical random RFC 4122 UUIDs from the operating system randomness source |

Reqwest is configured with its Rustls/platform-verifier path rather than disabling certificate checks or introducing native OpenSSL installation. This transitively includes `webpki-root-certs`, whose certificate-data package uses **CDLA-Permissive-2.0**. That permissive data license was reviewed and explicitly added to `deny.toml`; its required license/attribution material must be retained in generated release notices.

No dependency enables cookie storage, transparent gzip/Brotli/deflate/Zstandard decoding, HTTP/3, SOCKS, or Reqwest request JSON features. UUID generation uses the existing audited `getrandom` operating-system integration. `serde_json` uses the MIT-licensed `zmij` number-formatting implementation transitively; persisted state accepts integer counters and does not expose an unbounded generic JSON API. On Windows, `same-file` uses the Unlicense OR MIT `winapi-util` wrapper and its MIT OR Apache-2.0 `windows-sys` bindings to query volume and file identifiers from file handles; it does not read or modify file contents.

The #25 `sha2` addition resolves `digest` 0.11.3, `block-buffer` 0.12.1, `crypto-common` 0.2.2, `hybrid-array` 0.4.14, and `typenum` 1.20.1; all declare MIT OR Apache-2.0. Their locked package metadata and dependency-policy results were reviewed; no new license exception was needed.

## JavaScript development dependencies

The extension build/lint/test dependencies are development-only and listed exactly in `package.json`/`package-lock.json`. `npm run licenses:js` inventories all resolved packages. CC-BY-3.0 data currently comes from SPDX metadata used by the license-audit tool and is not extension runtime code.

Mozilla `web-ext` 10.6.0 was evaluated during scaffolding and deliberately removed because its then-current `addons-linter` → `image-size` tree had three high-severity denial-of-service advisories. A local bounded manifest/asset check is used until that dependency tree is safe to reconsider.

## Review commands

```powershell
npm audit
npm run licenses:js
cargo deny check
```

Any direct dependency addition must update this record with purpose and feature choices. Lockfile-only transitive changes still require CI results and inspection for new licenses, advisories, duplicate versions, sources, native build scripts, and sensitive logging behavior.
