# Third-party dependency review

This record complements automated lockfile, license, advisory, ban, and source checks. It documents deliberate direct dependency choices and exceptional transitive licenses; generated release notices remain a packaging requirement.

## First-party licensing boundary

The owner's 2026-09-09 FOSS clarification is implemented as the [MIT license](../LICENSE); see [ADR 0011](decisions/0011-license-and-available-qualification.md). This does not relicense any dependency. The eight-leaf package includes `LICENSE.txt` and full native/runtime/toolchain notices. The nine-leaf XPI/standalone extension build includes the MIT license and reviewed esbuild notice, including when distributed without the helper ZIP. No third-party implementation dependency was added for this change. The prior MSVC distribution-recipe decision is unaffected.

## Runtime Rust dependencies

| Dependency | Purpose | Version policy | License | Rationale |
| --- | --- | --- | --- | --- |
| `bytes` | Reference-counted bounded chunks returned by the HTTP streaming API | Exact workspace pin | MIT | Lets cancellation-aware scheduler code consume Reqwest body chunks without copying at the API boundary; assignment and stream limits still bound retained data |
| `reqwest` | Bounded async HTTP client and maintained protocol/TLS integration | Exact workspace pin | MIT OR Apache-2.0 | Avoids custom HTTP/TLS parsing; default features and automatic decompression are disabled |
| `httpdate` | Strict HTTP date and Retry-After parsing | Exact workspace pin | MIT OR Apache-2.0 | Small standards-focused parser avoids locale/date ambiguity |
| `time` | RFC 3339 cookie-expiry parsing and UTC comparison | Exact 0.3.55 pin, std/parsing only | MIT OR Apache-2.0 | Reviewed metadata before incorporation; avoids a custom date parser, local-time lookup, or credential serialization |
| `sha2` | Streamed user-supplied SHA-256 validation and local package consistency checks (not signatures) | Exact 0.11.0 pin, default features disabled | MIT OR Apache-2.0 | Reviewed package metadata before incorporation; maintained RustCrypto digest implementation, no custom cryptographic code, OID, or allocation feature needed |
| `same-file` | Prove that retained partial and published final names are hard links to one file | Exact workspace pin | Unlicense OR MIT | Uses stable device/file identity rather than comparing paths, timestamps, or contents heuristically; required for conservative crash recovery on Windows |
| `serde` | Strict protocol and internal task-state encoding/decoding | Exact workspace pin with derive support | MIT OR Apache-2.0 | Maintained typed serialization avoids ad hoc field conversion; protocol payloads and persisted version-specific records deny unknown fields |
| `serde_json` | Bounded Native Messaging and task-state JSON | Exact workspace pin with default `std` only | MIT OR Apache-2.0 | Human-inspectable JSON with project size limits; protocol input additionally uses a duplicate-member-rejecting visitor before typed decoding |
| `thiserror` | Typed internal errors with safe project-controlled display text | Exact workspace pin | MIT OR Apache-2.0 | Keeps stable classifications separate from untrusted source text |
| `tokio` | Host runtime, async worker tasks, cancellation notifications, timers, channels, and concurrency semaphores | Exact workspace pin; workspace defaults are `macros`, `rt-multi-thread`, `sync`, and `time`; local IPC additionally requests `io-util`/`net` | MIT | Provides maintained bounded scheduling/session primitives; Native Messaging framing itself remains synchronous stdio and Reqwest supplies its required network features transitively |
| `winsafe` | Native companion preview window and tray APIs | Exact0.0.29 pin; gui/shell only, default features disabled | MIT | Full license/manifest and relevant wrappers reviewed for #50; no transitive dependencies/build script, the shell retains unsafe-forbid, no message-filter changes or raw-pointer conversion. Preview only until companion/package qualification; no third-party source copied |
| `winreg` | Current-user setup registration only | Exact 0.56.0 pin, optional features disabled | MIT | Package metadata/license reviewed before incorporation; safe wrapper avoids project-unsafe Win32 calls, no registry transactions/serialization/date features needed |
| `uuid` | Opaque stable task, installation and generation identifiers | Exact workspace pin with v4 generation only | MIT OR Apache-2.0 | Produces canonical random RFC 4122 UUIDs from the operating system randomness source |

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

## #27 release toolchain and native runtime review

The static MSVC prototype passed local runtime/import checks but was not selected for distribution: actual Visual Studio 2026 Community [terms](https://visualstudio.microsoft.com/license-terms/vs2026-ga-community/) contain distributor/external-end-user requirements beyond including notices. No binary from that prototype was published. Choosing additional end-user/first-party terms was not assumed from public visibility.

The selected build-only archive is `llvm-mingw-20260826-ucrt-x86_64.zip`, SHA-256 **`ae601f4e0f72bbdf441ad2df8bb16f037e2e9251559ea6b37b4057aef39c06c3`**, from [LLVM/MinGW 20260826](https://github.com/mstorsjo/llvm-mingw/releases/tag/20260826). Bootstrap verifies archive size/hash and extracted bytes under ignored `target`; no compiler, SDK or toolchain ZIP is shipped. Rust remains 1.93.1; release target is `x86_64-pc-windows-gnullvm`. CMake/Ninja are development tools, not product prerequisites.

- LLVM/MinGW wrapper ISC license and full LLVM Apache-2.0-with-LLVM-exceptions notice reviewed. LLVM revision `ea7d852a70e8bdfaf601d6626a760f9771b2c4b4` is recorded by the compiler. Static compiler/runtime portions are covered by those notices/exceptions; no GCC profiling or toolchain distribution is selected.
- MinGW-w64 revision `a3d708261d5ba659205067cb82cae36e7ae8bbb0`: reviewed complete overall ZPL-2.1, runtime, toolchain, winpthreads MIT/BSD and WinStore MIT notices. Runtime notices, overall license, LLVM notice and winpthreads notice are included conservatively. Windows-supplied UCRT/API DLLs are imported, not copied; MinGW startup/UCRT wrappers are not Microsoft MSVC runtime objects.
- Link maps identified 51 selected MinGW object names in the helper and a 34-name subset in setup, plus startup objects. Reviewed 57 matching source/header notices: public-domain startup/wrappers, ZPL-2.1 general/scan-format code, permissive Keith Marshall formatting and Lucent/David Gay gdtoa notices. The upstream runtime notice's historical Cephes ambiguity is not treated as license clearance: no Cephes math or profiling object is in this selected set. `scripts/runtime-policy.json` gates newly selected objects for further review. `link-self-contained=no` uses the reviewed external runtime rather than an unrecorded bundled CRT.
- MinGW's toolchain notice distinguishes LGPL DirectX/DDK header types/short macros from linked implementation. This application does not select those implementations. Full recommended runtime attribution is retained rather than deleting notices because some describe unused portions.
- Cargo notices include the locked non-dev dependency closure and nested/vendored LICENSE/COPYING/NOTICE/COPYRIGHT files (including AWS-LC material), not just top-level Cargo license expressions. Rust library attribution covers multiple platforms and is labeled accordingly. esbuild attribution is retained. No third-party implementation source is copied into this repository.
- CI `actions/download-artifact` v4 is pinned to `d3f86a106a0bac45b974a628896c90dbdf5c8093`; its full MIT license was read before incorporation. It is build infrastructure, not runtime behavior.

The non-Cargo compiler/runtime review is explicit, not represented as something `cargo deny` alone checks. First-party code is now MIT licensed under the owner's later FOSS direction; third-party terms remain independent. Package consistency/provenance, MIT licensing and these notices do not constitute a publisher signature.

## #50 local IPC dependency review

- `interprocess = 2.4.4`, no defaults, `tokio` only: 0BSD OR Apache-2.0, Rust>=1.75, no build script. Full license/manifest and selected descriptor/listener/stream/drop APIs reviewed. First-instance flag, DACL, remote refusal and handle inheritance are explicit. Its client constructor is NOT used because its CreateFile flags omit SQOS. Neither blocking connect helpers nor flush/linger are used. Its own flush marker is insufficient to cancel Mio writes; ADR0015 records that discovered limitation.
- `hmac = 0.13.0`: MIT OR Apache-2.0, Rust>=1.85, no build script. Standard RustCrypto HMAC-SHA256 and constant-time `verify_slice`, with role/endpoint/nonces binding and an independent test vector; no cryptographic implementation copied. The existing `sha2 = 0.11.0` digest now also resolves `ctutils 0.4.2` and `cmov 0.5.4` through digest's mac feature: both Apache-2.0 OR MIT, reviewed manifests/licenses, no build scripts.
- `getrandom = 0.4.3`: existing UUID dependency now directly pinned for fallible 32-byte keys/nonces and UUID randomness. MIT OR Apache-2.0. Its existing short build script only detects memory-sanitizer configuration; no new OS feature, RNG fallback, download or credentials are introduced.
- `widestring = 1.2.1`, std: MIT OR Apache-2.0, Rust>=1.58, no build script; owned UTF-16 security-descriptor input, not arbitrary native pointers. Full MIT text/manifest reviewed.
- `windows-sys = 0.61.2`: existing MIT OR Apache-2.0 bindings, now directly selected with Foundation/System_IO in the tiny first-party cancellation boundary. This explicit exception is documented in ADR0015; it does NOT mean first-party unsafe remains universally forbidden. No Windows DLL is copied.
- New transitive `doctest-file 1.1.1` (documentation proc macro) and `recvmsg 1.0.0` (message traits) are **0BSD-only**. Initial cargo-deny correctly refused the unlisted license. Complete short zero-clause licenses and manifests were reviewed before adding 0BSD to the allowlist; both have no build script. Keep their notices in the generated dependency inventory even though first-party code remains MIT. `doctest-file` has no transitive dependencies; no third-party source was copied into this checkout.
- `winsafe`'s existing reviewed GetSystemDirectory wrapper selects the Windows-supplied read-only whoami identity utility without PATH/environment executable discovery. The utility is an OS component, not a developer-tool requirement or redistributed payload. No access-token/SID-buffer FFI wrapper was added to the first-party cancellation boundary.

The seven new locked registry packages are hmac, interprocess, widestring, ctutils, cmov, doctest-file and recvmsg; existing package versions were not opportunistically updated. Runtime pending-write/flush behavior was reviewed separately from dependency/license scanning. The transport is not yet part of the installed package's dependency closure or qualification.

## #50 protected-record adapter review

Setup's optional `private-file` feature reuses the exact existing `winsafe 0.0.29` dependency and feature selection only for `GetSystemDirectory`. No registry package/version/license or build script is added; the lockfile adds only setup's optional dependency edge. Default setup builds do not select it. The fixed first-party script invokes Windows-supplied PowerShell/.NET identity, file-creation and security-descriptor APIs. No PowerShell/.NET binary, security-wrapper implementation, custom P/Invoke or third-party source is copied or redistributed. The process, permission and secret-handling boundary is documented in [PRIVATE_RUNTIME_RECORD.md](PRIVATE_RUNTIME_RECORD.md), separately from license scanning and ADR0015.

### Opt-in protected-directory SDK surface

Setup `private-directory` selects the existing exact winsafe0.0.29 advapi feature for ConvertStringSidToSid/LocalFreeSidGuard and InitializeSecurityDescriptor, with synchronous CreateDirectory from kernel. PRIVATE_RUNTIME_RECORD.md records the owned SID/whole-ACL allocation lifetime and layout review; no new registry package or first-party unsafe exception is introduced. The optional setup-to-local-ipc dependency reuses the bounded current-user identity adapter and introduces no dependency cycle.

The installed-runtime composition adds only setup's development dependency edge to the already selected exact Tokio1.53.1 for real-pipe/runtime tests. No registry version, license exception or additional first-party unsafe allowance is introduced.

The application integration adds only optional first-party companion-to-setup/protocol edges. Native stdio pumps use standard-library processes/threads/pipes; no registry package, copied wrapper or additional unsafe allowance is selected. Tokio process stdio was reviewed as blocking-pool backed and is not used for this forwarding path.
