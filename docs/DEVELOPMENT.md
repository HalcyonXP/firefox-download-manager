# Development and bootstrap

The supported development host is a clean 64-bit Windows 11 checkout. CI uses the same Windows-first commands; Windows server CI is not itself Windows 11 browser qualification. Build tools and generated output stay local; no globally installed formatter, linter, or test runner is required.

## Prerequisites

1. [Git for Windows](https://git-scm.com/download/win)
2. [Rustup](https://rustup.rs/) using the MSVC host toolchain
3. Visual Studio 2022 or 2026 C++ Build Tools with **Desktop development with C++** and a Windows SDK
4. Node.js 24 and npm 11
5. Python 3.11+ for allowlist packaging and isolated lifecycle tests
6. Firefox Developer Edition 156 or later for manual extension testing (156 is the exercised API baseline; final-artifact qualification is separate)

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
  setup/                Local Windows setup, ownership and journal recovery
  test-server/          Local deterministic adversarial HTTP fixtures
protocol/
  schema/v2/            Current JSON Schema and conformance examples
  schema/v1/            Archived v1 contract
native-host/            Auditable Firefox host-manifest template
scripts/                Cross-platform checks, package generation and isolated lifecycle tests
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
| `npm run native-host:check` | Cross-check the extension ID, permission, host manifest, Rust constants, and fixed HKCU setup adapter |
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

## Candidate package build and isolated installation tests

Read [INSTALLATION.md](INSTALLATION.md) for user-facing commands. Old `install-native-host.ps1`, `uninstall-native-host.ps1` and `test-native-host-install.ps1` are retired refusal-only stubs: no unsafe convenience fallback remains.

```powershell
./scripts/build-package.ps1 -Output artifacts/package
# For local uncommitted work only (explicitly unqualified):
./scripts/build-package.ps1 -Output artifacts/development-package -Development
.\artifacts\package\download-manager-setup.exe verify
.\artifacts\package\download-manager-setup.exe probe
```

Production candidates require a clean canonical checkout. Each output directory must be new, beneath `artifacts`. The reviewed SHA-256-pinned LLVM/MinGW build toolchain is fetched beneath ignored `target` (no system installation). Rust target `x86_64-pc-windows-gnullvm`, static compiler/MinGW support with system UCRT, path remapping and disabled linker timestamps are used; deterministic ZIP order/timestamps are tested. Build provenance records actual compiler/runtime imports. Repeat measurements determine binary reproducibility—flags alone do not prove it.

The package builder copies only fixed payload leaves and the exact extension build allowlist, not checkout history, profiles, test-server executables, private audit inputs or arbitrary directories. Notices include the locked runtime/build dependency closure, Rust library attribution, vendored native notices, LLVM/MinGW runtime notices and esbuild attribution. No first-party license is selected.

**Only on a disposable current-user environment with no Firefox/helper process or existing registration:**

```powershell
python scripts/test-package-install.py --package artifacts/package --report artifacts/install-evidence.json
```

This exercises actual fixed-HKCU install, invalid-executable launch rollback, same-version upgrade, cleanup, uninstall and opaque state/download preservation through paths containing spaces. It is not Firefox UI testing or itself a legacy decoder test. Existing Rust persistence/recovery suites cover real old-format migration; #28 covers final-browser/restart behavior. The test also refuses empty/malformed/named registration entries, strips the child PATH to System32, and refuses unowned processes/registrations, captures no raw output into the report, and only cleans its own temporary registration/root.

Temporary XPI loading is explicit and must be repeated after Firefox restarts. Do not inspect, modify or stop an unowned live Firefox profile/instance to make a test pass. Mozilla `web-ext` remains outside the locked toolchain pending its separate dependency review.

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

For repeatability measurements, `build-package.ps1 -Rebuild` explicitly cleans only its dedicated `target/package-build` Cargo cache before compilation. Compare new output directories with `scripts/compare-packages.py`; do not treat a cached no-op build or a same-environment match as cross-machine proof. Final-artifact qualification uses exact candidate/release checksums, not an assumed rebuild identity.
