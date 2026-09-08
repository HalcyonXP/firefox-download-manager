# Development and bootstrap

The supported development host is a clean 64-bit Windows 11 checkout. CI uses the same Windows-first commands. Build tools and generated output stay local; no globally installed formatter, linter, or test runner is required.

## Prerequisites

1. [Git for Windows](https://git-scm.com/download/win)
2. [Rustup](https://rustup.rs/) using the MSVC host toolchain
3. Visual Studio 2022 Build Tools with **Desktop development with C++** and a Windows SDK
4. Node.js 24 and npm 11
5. Firefox Developer Edition 156 or later for manual extension testing (156 is the exercised API baseline; final-artifact qualification is separate)

The repository pins Rust in `rust-toolchain.toml`, records the npm version in `package.json`, and commits `Cargo.lock` and `package-lock.json`. Node 24 is the CI baseline; Node versions accepted by `package.json` may be used locally.

## Clean checkout

Run these commands in PowerShell:

```powershell
git clone https://github.com/HalcyonXP/firefox-download-manager.git
Set-Location firefox-download-manager

rustup show
npm ci

npm run check
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo build --workspace --all-features --locked
```

This is the public, authoritative development checkout. Reading/cloning it does not require GitHub authentication; contributing/pushing still requires appropriate authorization. `npm ci` installs all project-local JavaScript tools. Use the same repository for builds, issues, PRs, CI, and releases.

## Project layout

```text
extension/
  src/                  WebExtension TypeScript and static manifest sources
  test/                 Extension unit tests
  dist/                 Generated unpacked extension (ignored)
crates/
  protocol/             Rust Native Messaging types and framing boundary
  engine/               Networking, scheduling, task control, progress, persistence, and storage
  native-host/          Native Messaging executable
  test-server/          Local deterministic adversarial HTTP fixtures
protocol/
  schema/v2/            Current JSON Schema and conformance examples
  schema/v1/            Archived v1 contract
native-host/            Auditable Firefox host-manifest template
scripts/                Cross-platform checks plus Windows registration scripts
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
| `npm run protocol:check` | Validate v2 schema, examples, and hostile cases |
| `npm run native-host:check` | Cross-check the extension ID, permission, host manifest, Rust constants, and HKCU scripts |
| `npm run build` | Recreate `extension/dist` |
| `npm run extension:check` | Validate the built manifest and referenced assets |
| `npm run check` | Run JavaScript/protocol/extension, privacy, and repository-reference gates |
| `npm run repository:check` | Reject retired repository references in current tracked files |
| `cargo fmt --all` | Format all Rust crates |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Treat Rust lint warnings as failures |
| `cargo test --workspace --all-features --locked` | Run all Rust tests |
| `cargo build --workspace --all-features --locked` | Build all Rust targets |
| `cargo test -p download-manager-engine --test task_lifecycle --locked` | Run task lifecycle, safe shutdown, removal, and recovery integration tests |
| `cargo test -p download-manager-native-host --locked` | Run framing-session, negotiation, EOF, and reconnect tests |

`extension/dist` and `target` are disposable. Do not edit or commit them.

The task-lifecycle integration suite uses only loopback deterministic fixtures. It covers durable pause/reopen/resume over missing ranges, periodic bytes-first checkpoints, keep/delete cancellation and terminal removal, cancellation-aware probe and retry sleeps, cooperative process shutdown, a shared probe/transfer retry budget, retry exhaustion and `Retry-After`, fatal no-retry errors, changed resource identity, known/unknown progress, event cadence/overflow, final publication, and runtime-ownership failures. Timing assertions use generous bounds around monotonic behavior; repeat this test target when changing cancellation or event races.

## Native host installation and manual extension loading

Build and register the helper under the current user, then build the extension:

```powershell
./scripts/install-native-host.ps1
npm run build
```

The installer performs a locked release build by default, copies the executable beneath `%LOCALAPPDATA%\HalcyonXP\FirefoxDownloadManager\host`, generates a JSON-escaped absolute manifest path, and writes only the default value of:

```text
HKCU\Software\Mozilla\NativeMessagingHosts\com.halcyonxp.firefox_download_manager
```

**Development-only until #27 resolves the installer findings in [SECURITY_REVIEW.md](SECURITY_REVIEW.md).** Use isolated test roots and refuse existing registrations; do not use or terminate an unowned Firefox instance. No elevation is required. The registration permits only `download-manager@halcyonxp.local`. Open `about:debugging#/runtime/this-firefox` in Firefox Developer Edition, choose **Load Temporary Add-on**, and select `extension/dist/manifest.json`. The background connection object remains on demand: later UI work calls it when a manager surface is opened; every fresh port negotiates v2 and waits for a complete helper snapshot before replacing retained display state.

To exercise the same registration, path-with-spaces, process-launch, hello, initial-snapshot, add-command, and live schema check used by Windows CI:

```powershell
cargo build -p download-manager-native-host --locked
$installRoot = Join-Path $env:TEMP "Download Manager Native Host With Spaces"
./scripts/install-native-host.ps1 -ExecutablePath ./target/debug/download-manager-native-host.exe -InstallRoot $installRoot
./scripts/test-native-host-install.ps1 -InstallRoot $installRoot
./scripts/uninstall-native-host.ps1 -InstallRoot $installRoot
```

For the normal install, remove only the HKCU registration and generated host files with:

```powershell
./scripts/uninstall-native-host.ps1
```

Uninstallation deliberately does not recurse into `%LOCALAPPDATA%\HalcyonXP\FirefoxDownloadManager\state`, alter destination files, or remove completed downloads. Stop any live Native Messaging connection before replacing or removing a Windows executable. Mozilla's `web-ext` can be reconsidered after its dependency tree has no known high-severity advisory; it remains outside the locked toolchain.

## Privacy checks before publication

Run `npm run privacy:test`, `npm run privacy:check`, and `npm run privacy:history` (full history required). The normal `npm run check` includes the policy tests and HEAD-history guard. Never paste matched private values into issues or commit messages. See [PUBLICATION_PRIVACY.md](PUBLICATION_PRIVACY.md) for target-specific clearance and `privacy:publication`; local HEAD pattern checks alone do not clear GitHub-retained original history.

## Maintainer remotes and issue history

`origin` is `https://github.com/HalcyonXP/firefox-download-manager.git`. A checkout from the temporary two-remote arrangement should preserve local changes, then switch its authoritative remote to this URL (or use a fresh clone). This working checkout already uses the new `origin`; its optional `private-archive` remote has pushing disabled.

There is no mirror-publishing step. Create branches and PRs directly in this repository, referencing its current issue numbers. See [ISSUE_MIGRATION.md](ISSUE_MIGRATION.md) before interpreting old commit-message numbers. Never merge or push original private bundles, old PR refs, or pre-scrub branches. Private audit inputs and recovery material stay outside the checkout.

Public CI results must name the exact tested commit. Historical blocked jobs, dependency-update jobs, and mocked Firefox transport are not successful end-to-end release qualification. If a new issue/PR is not on the owner planning board, add it explicitly; repository auto-add has not been verified for this repository.

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

CI runs the JavaScript license summary and `cargo-deny`. Review a dependency's exact license text and attribution requirements before accepting it; a successful automated classification is not legal advice. Dependencies and GitHub Actions are versioned or commit-pinned, and generated lockfile changes must be reviewed. Deliberate direct choices and exceptional transitive licenses are recorded in [THIRD_PARTY.md](THIRD_PARTY.md).

## Local configuration and secrets

Do not commit `.env` files, credentials, cookies, authorization headers, signed URLs, logs, task state, downloaded partials, native-host registry exports, signing keys, certificates, or packaged binaries. Tests must use deterministic local fixtures and obviously fake values.

The helper must reserve standard output for Native Messaging frames. Local diagnostics go to bounded files or standard error only after redaction.

## CI

`.github/workflows/ci.yml` runs extension/protocol/manifest checks and the full Rust format/lint/test/build sequence. The `windows-latest` job installs the built helper into a temporary path containing spaces, verifies its HKCU registration and live framed hello/snapshot exchange, and removes it in a `finally` block. Dependency policy runs on Linux because `cargo-deny` is platform-independent. CI uploads the generated extension directory for inspection but does not publish a release.
