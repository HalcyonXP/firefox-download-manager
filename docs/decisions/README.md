# Architecture decision records

Accepted ADRs define the baseline for the initial local release. Superseding a decision requires a new ADR that links to the old record; accepted history is not rewritten.

| ADR | Decision | Status |
| --- | --- | --- |
| [0001](0001-component-boundaries.md) | Firefox WebExtension and Rust component boundaries | Accepted |
| [0002](0002-firefox-manifest-v3.md) | Manifest V3 with a Firefox event page | Accepted |
| [0003](0003-helper-lifecycle.md) | On-demand native-helper lifecycle | Accepted |
| [0004](0004-storage-and-recovery.md) | Partial-file and recoverable-state model | Accepted |
| [0005](0005-http-segmentation.md) | Strictly validated HTTP segmentation | Accepted |
| [0006](0006-third-party-code.md) | Original implementation and dependency provenance | Accepted |
| [0007](0007-task-controls-retries-progress.md) | Cooperative task controls, bounded retries, and latest-value progress | Accepted |

Each record states its reversibility. Security and correctness invariants are not made optional merely because an implementation choice is reversible.

- [0008 — Isolate publication from retained private history](0008-isolated-publication.md)
