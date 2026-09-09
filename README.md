# Firefox Download Manager

A local download manager for Firefox Developer Edition on Windows 11. It is intended to improve throughput on servers that support HTTP byte ranges by downloading validated file segments concurrently.

## Repository and privacy status

**[HalcyonXP/firefox-download-manager](https://github.com/HalcyonXP/firefox-download-manager)** is the public, authoritative repository for code, issues, CI, and releases. Use this repository directly; there is no publication mirror to synchronize.

The private predecessor is retained only as an archive. Its sensitive original Git history was not imported. See [publication privacy](docs/PUBLICATION_PRIVACY.md) and the [issue migration map](docs/ISSUE_MIGRATION.md). Public source availability is not an installable-release announcement.

## Project status

Planning is complete and implementation proceeds through the lowest-numbered **Ready** issue. Current status is tracked on the private [Firefox Download Manager project board](https://github.com/users/HalcyonXP/projects/1), in [GitHub Issues](https://github.com/HalcyonXP/firefox-download-manager/issues), and across five milestones:

1. [M0 — Foundation](https://github.com/HalcyonXP/firefox-download-manager/milestone/1)
2. [M1 — Native download MVP](https://github.com/HalcyonXP/firefox-download-manager/milestone/2)
3. [M2 — Firefox integration](https://github.com/HalcyonXP/firefox-download-manager/milestone/3)
4. [M3 — Reliability and authenticated downloads](https://github.com/HalcyonXP/firefox-download-manager/milestone/4)
5. [M4 — Local release](https://github.com/HalcyonXP/firefox-download-manager/milestone/5)

See [docs/PROJECT_PLAN.md](docs/PROJECT_PLAN.md) for scope, execution order, and release criteria; [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for component boundaries and accepted decisions; [docs/SECURITY.md](docs/SECURITY.md) for the threat model and sensitive-data policy; [docs/PROTOCOL.md](docs/PROTOCOL.md) for the versioned extension/helper contract; [docs/HTTP.md](docs/HTTP.md), [docs/STORAGE.md](docs/STORAGE.md), [docs/STATE.md](docs/STATE.md), and [docs/TEST_SERVER.md](docs/TEST_SERVER.md) for strict HTTP, partial-file, recovery, and fixture behavior; and [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for Windows bootstrap and quality commands. See [AGENTS.md](AGENTS.md) for the implementation workflow and non-negotiable constraints used across development sessions.

## Intended architecture

```text
Firefox WebExtension
  ├── Explicit “Download with Manager” action
  ├── Queue, progress, and settings UI
  └── Versioned Native Messaging client
                    │
                    ▼
Rust native helper
  ├── HTTP probe and strict range validation
  ├── Fixed 1/2/4/8-worker segment scheduler with independent request caps
  ├── Cooperative pause/cancel, bounded retry, and coalesced progress
  ├── Validated random-access and bounded sequential partial-file writers
  └── Persistent pause/resume and recovery state
```

## Scope boundaries

The initial product targets direct HTTP(S) downloads selected explicitly by the user. It does not control Proton VPN or any other VPN, change network routes, extract media, support torrents, or automatically replace every Firefox download.

The native helper uses the operating system's normal network route, whether a VPN is connected or not.

## Install v0.1.0

**[Download the qualified Windows-x64 personal release](https://github.com/HalcyonXP/firefox-download-manager/releases/tag/v0.1.0).** Follow its notes and [installation instructions](docs/INSTALLATION.md), using the original `PACKAGE-SHA256SUMS.txt` to verify the ZIP. The [release record](docs/releases/v0.1.0.md) binds the exact main-d03 source, tested bytes, evidence and support limits. Later CI candidates are not replacements for that release.

Qualification used the existing native Windows11 x64 computer and Developer Edition156.0, not a separate clean OS. The helper and temporary XPI are unsigned; do not disable protections. Packaging/recovery design is in [PACKAGING_PLAN.md](docs/PACKAGING_PLAN.md).

The Rust setup executable uses current-user registration, verified immutable generations, ownership receipts and conservative journal recovery. It requires closed Firefox/helpers and does not modify a browser profile or security policy. The old development PowerShell registration scripts are retired and deliberately refuse all operations. Developer build/testing instructions are in [DEVELOPMENT.md](docs/DEVELOPMENT.md).

Firefox Developer Edition 156+ loads the unsigned XPI through `about:debugging` as a temporary add-on; it must be reloaded after browser restarts. The extension requires `nativeMessaging` and `menus`; cookies/selected-site authority is optional and per-Add handoff remains unchecked by default. Wire v2 components must be paired.

## Settings and local diagnostics

The manager page includes helper-owned destination, worker/request caps, retry, retention, and verbose-log settings. Pause active work before saving. See [docs/SETTINGS.md](docs/SETTINGS.md) for defaults, migration, bounded enum-only diagnostics, and recovery.

## Guiding principles

- Correct bytes are more important than optimistic speed.
- Resume and crash safety are core features.
- Four connections is the conservative default; eight is the initial cap.
- Invalid range responses fall back safely or fail—they are never merged blindly.
- Pause/cancel waits for workers and checkpoints retained ranges before acknowledgement.
- Transient retries are bounded and delayed; fatal protocol/storage failures stop.
- Credentials and sensitive URLs are not written to ordinary logs.
- Everything remains local; no analytics, telemetry, or remote updater.

## License

First-party code is free and open-source under the [MIT license](LICENSE). Use, modify, redistribute or sell it with the copyright/permission notice retained. Third-party components keep their own licenses and [required notices](docs/THIRD_PARTY.md). [ADR 0011](docs/decisions/0011-license-and-available-qualification.md) records the owner's FOSS direction and our permissive-license choice; GitHub visibility alone had not selected one.

## Optional authenticated downloads

Per-download [session handoff](docs/AUTHENTICATION.md) supports eligible normal default-store cookies, same-origin referrers, and HTTPS Basic/Bearer values using optional permissions. Secrets remain memory-only; cross-origin redirects are blocked and restart/expiry requires a fresh task. Private, container, partitioned, and first-party-isolated sessions are not supported. The release notes distinguish tested session behavior from unsupported contexts.
