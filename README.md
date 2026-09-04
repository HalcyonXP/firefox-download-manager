# Firefox Download Manager

A local download manager for Firefox Developer Edition on Windows 11. It is intended to improve throughput on servers that support HTTP byte ranges by downloading validated file segments concurrently.

## Project status

Planning. Implementation work is tracked in [GitHub Issues](https://github.com/HalcyonXP/download-manager/issues) and organized into five milestones:

1. [M0 — Foundation](https://github.com/HalcyonXP/download-manager/milestone/2)
2. [M1 — Native download MVP](https://github.com/HalcyonXP/download-manager/milestone/3)
3. [M2 — Firefox integration](https://github.com/HalcyonXP/download-manager/milestone/4)
4. [M3 — Reliability and authenticated downloads](https://github.com/HalcyonXP/download-manager/milestone/5)
5. [M4 — Local release](https://github.com/HalcyonXP/download-manager/milestone/6)

See [docs/PROJECT_PLAN.md](docs/PROJECT_PLAN.md) for scope, architecture, execution order, and release criteria.

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
  ├── Concurrent segment scheduler
  ├── Direct random-access file writer
  └── Persistent pause/resume and recovery state
```

## Scope boundaries

The initial product targets direct HTTP(S) downloads selected explicitly by the user. It does not control Proton VPN or any other VPN, change network routes, extract media, support torrents, or automatically replace every Firefox download.

The native helper uses the operating system's normal network route, whether a VPN is connected or not.

## Guiding principles

- Correct bytes are more important than optimistic speed.
- Resume and crash safety are core features.
- Four connections is the conservative default; eight is the initial cap.
- Invalid range responses fall back safely or fail—they are never merged blindly.
- Credentials and sensitive URLs are not written to ordinary logs.
- Everything remains local; no analytics, telemetry, or remote updater.
