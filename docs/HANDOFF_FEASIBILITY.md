# Ordinary-click handoff feasibility — #49

Status: **in progress, not a delivered capture feature**. #48/PR54 accepted the revised workflow only. This record adds the owner's concrete case; actual Firefox correlation, persistent signing and companion handoff remain unproved. [ADR0013](decisions/0013-install-restart-click.md) governs M5.

## Exact owner-supplied case

- [Public page](https://huggingface.co/bartowski/orcarouter_Qwen3.8-27B-Uncensored-GGUF/tree/main)
- [Ordinary download target](https://huggingface.co/bartowski/orcarouter_Qwen3.8-27B-Uncensored-GGUF/resolve/main/orcarouter_Qwen3.8-27B-Uncensored-Q3_K_M.gguf?download=true)

Keep the original `resolve/main/...gguf?download=true` URL as the acceptance input. The owner supplied these public links after the direction checkpoint; the earlier unknown-provider statement is historical. Their installation method, native connection state and actual clicked DOM element remain unverified. No normal profile was inspected.

The `main` resource can change. Observed size, destination host and validators are not permanent product constants. Never hardcode or publish an expiring signed CDN redirect URL.

## Separate evidence layers

| Observation | Result | Does not establish |
| --- | --- | --- |
| Anonymous Python HEAD requests, no body reads | huggingface.co302 → us.aws.cdn.hf.co200; Content-Length14605736192; byte-range advertisement, ETag and attachment disposition at destination | Actual ranged GET, strong identity, browser capture or full-file integrity |
| Separate Python GET requesting bytes0–3 | Failed with ssl.SSLEOFError wrapped in urllib.error.URLError; no success report | Failed hop/cause, Firefox behavior or engine failure. The hop was not retained; do not reconstruct it |
| Development Rust engine-library probe of the original URL | Passed first-byte and last-byte range/body/validator checks; size14605736192, strong identity, segmented mode, filename present | Full-file transfer/hash, installed/released executable behavior, automatic capture or persistent XPI |
| Diagnostic's owned loopback tests | Real boundary requests plus input bounds and output redaction assertions | Live browser/companion behavior |

The size is approximately14.6GB (13.6GiB). No download task/file or full model was created. The Rust probe requested one byte at each boundary with normal engine redirect/TLS/admission policy, no cookies or authorization handoff. These are development-library observations, not exact-package qualification. The initial live invocation used engine source fromf944247 and an uncommitted diagnostic example; no release identity is assigned to it. Its later success is not a retry of the Python client and does not explain that client's TLS failure. No TLS/proxy/VPN/protection setting changed.

Only bounded result fields were retained for the successful observations: size, mode, validator/filename presence, status and header-only hop hosts. Signed redirect queries and opaque validators must not enter ordinary logs, issues or fixtures. Failed private diagnostics remain private, not release inputs.

## Implications and next proof

- Classification must survive the original public URL's redirect to an attachment response at another origin, rather than guessing from the original `.gguf` suffix. The HEAD response at the original URL did not declare an attachment.
- An anonymous library probe worked for this case at this observation. That does not authorize implicit cookie/Authorization harvesting or claim all Hugging Face resources are anonymous/replayable.
- Build bounded deterministic redirect/attachment cases, including extensionless targets and query preservation. Synthetic signed-looking fixture values must never be copied from live CDN responses.
- Prove actual Firefox request/click correlation, private/container/method classification and ordinary-navigation non-interference before selecting the interceptor. A downloads-item URL alone is insufficient.
- Establish prepare/accept/cancel/commit or an equivalent bounded, idempotent contract with visible handling of uncertain outcomes. Existing v2 Add immediately accepting a task is not cross-process handoff proof.
- Keep signing/account/submission authority explicit. Mozilla's documented signed self-distribution path is not an approved publisher account or a returned signed XPI.
- The final owner-case test remains setup → visible tray → normal signed XPI installation → Firefox restart → ordinary click → one automatically running native task → independent correct full output. Neither this probe nor manual Add can substitute.

The owner is using Firefox. Do not run browser/registration qualification until fresh permission, closed-app and owned-state preflights are satisfied. Do not download the whole model repeatedly while developing the interceptor; use fixtures first and preflight disk/network scope for the final owned real-file test.

## Maintainer-only diagnostic

`crates/engine/examples/probe_metadata.rs` reads a URL from bounded stdin, not arguments, invokes only the engine probe and emits non-secret result fields. It does not launch Firefox, register a host, create tasks or write download files. It has a60-second overall probe deadline in addition to the engine's request limits. URL stdin must reach EOF; this is not an interactive user installation command.

Build with `cargo build -p download-manager-engine --example probe_metadata --locked`, then provide the selected URL on stdin to the built example. Keep credentials and signed URLs out of shell history/arguments. Do not print ResourceProbe, final URLs, request headers or underlying transport error chains in a diagnostic. Successful range probes normally read the two assigned bytes; an ignored range is reported as a single-stream mode without starting a transfer.

Its tests run in the ordinary Cargo test suite (`test = true`) and can be selected with `cargo test -p download-manager-engine --example probe_metadata --locked`. They contact only the owned deterministic fixture, never the public model. Initial engine tests, workspace/all-target/all-feature Clippy and npm checks passed. Increasing the input read bound caused the intended cursor-position assertion to fail; adding the final URL to output caused the intended canary-redaction assertion to fail. Restored tests passed. These mutations establish the diagnostic's bounded/redacted reporting contracts, not browser handoff. User instructions remain in [USER_WORKFLOW.md](USER_WORKFLOW.md); none of these maintainer commands belongs in that five-step target.
