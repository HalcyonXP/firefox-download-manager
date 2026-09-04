# Third-party dependency review

This record complements automated lockfile, license, advisory, ban, and source checks. It documents deliberate direct dependency choices and exceptional transitive licenses; generated release notices remain a packaging requirement.

## Runtime Rust dependencies

| Dependency | Purpose | Version policy | License | Rationale |
| --- | --- | --- | --- | --- |
| `reqwest` | Bounded async HTTP client and maintained protocol/TLS integration | Exact workspace pin | MIT OR Apache-2.0 | Avoids custom HTTP/TLS parsing; default features and automatic decompression are disabled |
| `httpdate` | Strict HTTP date and Retry-After parsing | Exact workspace pin | MIT OR Apache-2.0 | Small standards-focused parser avoids locale/date ambiguity |
| `thiserror` | Typed internal errors with safe project-controlled display text | Exact workspace pin | MIT OR Apache-2.0 | Keeps stable classifications separate from untrusted source text |

`tokio` is currently a direct test dependency and a transitive runtime dependency of Reqwest. It is exact-pinned under the workspace and MIT licensed. Later scheduler work may make its direct runtime role explicit.

Reqwest is configured with its Rustls/platform-verifier path rather than disabling certificate checks or introducing native OpenSSL installation. This transitively includes `webpki-root-certs`, whose certificate-data package uses **CDLA-Permissive-2.0**. That permissive data license was reviewed and explicitly added to `deny.toml`; its required license/attribution material must be retained in generated release notices.

No dependency enables cookie storage, transparent gzip/Brotli/deflate/Zstandard decoding, HTTP/3, SOCKS, or request JSON features in the probing baseline.

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
