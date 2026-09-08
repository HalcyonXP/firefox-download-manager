# Firefox Download Manager

A local download manager for Firefox Developer Edition on Windows 11. It is intended to improve throughput on servers that support HTTP byte ranges by downloading validated file segments concurrently.

## Project status

Planning is complete and implementation proceeds through the lowest-numbered **Ready** issue. Current status is tracked on the private [Firefox Download Manager project board](https://github.com/users/HalcyonXP/projects/1), in [GitHub Issues](https://github.com/HalcyonXP/download-manager/issues), and across five milestones:

1. [M0 — Foundation](https://github.com/HalcyonXP/download-manager/milestone/2)
2. [M1 — Native download MVP](https://github.com/HalcyonXP/download-manager/milestone/3)
3. [M2 — Firefox integration](https://github.com/HalcyonXP/download-manager/milestone/4)
4. [M3 — Reliability and authenticated downloads](https://github.com/HalcyonXP/download-manager/milestone/5)
5. [M4 — Local release](https://github.com/HalcyonXP/download-manager/milestone/6)

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

## Native Messaging development install

Build and register the on-demand helper for the current Windows user, then build and temporarily load the extension:

```powershell
./scripts/install-native-host.ps1
npm run build
```

The installer copies the release helper to the per-user application-data tree and writes only `HKCU\Software\Mozilla\NativeMessagingHosts\com.halcyonxp.firefox_download_manager`. The generated manifest permits only `download-manager@halcyonxp.local`; the extension requests `nativeMessaging` and `menus` and no host access. Paths containing spaces are supported. Remove the registration and installed helper files without touching download state or completed files with:

```powershell
./scripts/uninstall-native-host.ps1
```

See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for verification and manual Firefox steps. The registered transport and reconnect snapshot layer are implemented; explicit link capture and the creation form are implemented; the snapshot-driven dashboard supports pause, resume/start/retry, cancel, remove, and task-destination folder opening. Both components must be rebuilt together for protocol v2. These scripts do not create a service, listener, firewall rule, VPN configuration, or network-route change.

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
