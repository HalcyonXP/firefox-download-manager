# Development and bootstrap

The supported development host is a clean 64-bit Windows 11 checkout. CI uses the same Windows-first commands. Build tools and generated output stay local; no globally installed formatter, linter, or test runner is required.

## Prerequisites

1. [Git for Windows](https://git-scm.com/download/win)
2. [Rustup](https://rustup.rs/) using the MSVC host toolchain
3. Visual Studio 2022 Build Tools with **Desktop development with C++** and a Windows SDK
4. Node.js 24 and npm 11
5. Firefox Developer Edition for manual extension testing

The repository pins Rust in `rust-toolchain.toml`, records the npm version in `package.json`, and commits `Cargo.lock` and `package-lock.json`. Node 24 is the CI baseline; Node versions accepted by `package.json` may be used locally.

## Clean checkout

Run these commands in PowerShell:

```powershell
git clone https://github.com/HalcyonXP/download-manager.git
Set-Location download-manager

rustup show
npm ci

npm run check
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo build --workspace --all-features --locked
```

The repository is private, so cloning requires an authenticated Git credential. `npm ci` installs all project-local JavaScript tools.

## Project layout

```text
extension/
  src/                  WebExtension TypeScript and static manifest sources
  test/                 Extension unit tests
  dist/                 Generated unpacked extension (ignored)
crates/
  protocol/             Rust Native Messaging types and framing boundary
  engine/               Networking, scheduling, persistence, and storage
  native-host/          Native Messaging executable
  test-server/          Local deterministic adversarial HTTP fixtures
protocol/
  schema/v1/            Normative JSON Schema and conformance examples
scripts/                Cross-platform build and validation scripts
docs/                   Architecture, security, protocol, and plans
```

The browser extension build never contains the Rust helper. The native helper never bundles the test HTTP server. Download bodies never pass through extension build output or Native Messaging.

## Common commands

| Command | Purpose |
| --- | --- |
| `npm run format` | Format extension, scripts, workflow, and protocol JSON sources |
| `npm run lint` | Lint JavaScript and TypeScript |
| `npm run typecheck` | Strict TypeScript check without output |
| `npm test` | Run extension unit tests |
| `npm run protocol:check` | Validate v1 schema, examples, and hostile cases |
| `npm run build` | Recreate `extension/dist` |
| `npm run extension:check` | Validate the built manifest and referenced assets |
| `npm run check` | Run all JavaScript/protocol/extension quality gates |
| `cargo fmt --all` | Format all Rust crates |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Treat Rust lint warnings as failures |
| `cargo test --workspace --all-features --locked` | Run all Rust tests |
| `cargo build --workspace --all-features --locked` | Build all Rust targets |

`extension/dist` and `target` are disposable. Do not edit or commit them.

## Manual extension loading

Build the extension, open `about:debugging#/runtime/this-firefox` in Firefox Developer Edition, choose **Load Temporary Add-on**, and select `extension/dist/manifest.json`:

```powershell
npm run build
```

The scaffold has no download UI or host registration yet. Those are implemented by their dedicated issues. Mozilla's `web-ext` can be reconsidered after its dependency tree has no known high-severity advisory; it is intentionally not part of the locked toolchain at this baseline.

## Dependency and license review

JavaScript inventory and licenses:

```powershell
npm audit
npm run licenses:js
```

Rust advisories, licenses, bans, and sources:

```powershell
cargo install cargo-deny --locked --version 0.20.2
cargo deny check
```

CI runs the JavaScript license summary and `cargo-deny`. Review a dependency's exact license text and attribution requirements before accepting it; a successful automated classification is not legal advice. Dependencies and GitHub Actions are versioned or commit-pinned, and generated lockfile changes must be reviewed.

## Local configuration and secrets

Do not commit `.env` files, credentials, cookies, authorization headers, signed URLs, logs, task state, downloaded partials, native-host registry exports, signing keys, certificates, or packaged binaries. Tests must use deterministic local fixtures and obviously fake values.

The helper must reserve standard output for Native Messaging frames. Local diagnostics go to bounded files or standard error only after redaction.

## CI

`.github/workflows/ci.yml` runs extension/protocol checks and the full Rust format/lint/test/build sequence. The primary job runs on `windows-latest`; dependency policy runs on Linux because `cargo-deny` is platform-independent. CI uploads the generated extension directory for inspection but does not publish or install a release.
