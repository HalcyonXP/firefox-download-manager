# Architecture decision records

Accepted ADRs define the baseline for the initial local release. Superseding a decision requires a new ADR that links to the old record; accepted history is not rewritten.

| ADR | Decision | Status |
| --- | --- | --- |
| [0001](0001-component-boundaries.md) | Firefox WebExtension and Rust component boundaries | Accepted |
| [0002](0002-firefox-manifest-v3.md) | Manifest V3 with a Firefox event page | Accepted |
| [0003](0003-helper-lifecycle.md) | On-demand native-helper lifecycle | v0.1.0 baseline; successor direction in 0013 |
| [0004](0004-storage-and-recovery.md) | Partial-file and recoverable-state model | Accepted |
| [0005](0005-http-segmentation.md) | Strictly validated HTTP segmentation | Accepted |
| [0006](0006-third-party-code.md) | Original implementation and dependency provenance | Accepted |
| [0007](0007-task-controls-retries-progress.md) | Cooperative task controls, bounded retries, and latest-value progress | Accepted |
| [0008](0008-isolated-publication.md) | Isolate publication from retained private history | Superseded by 0009 |
| [0009](0009-public-authority.md) | One public authoritative repository | Accepted |
| [0010](0010-minimal-session-handoff.md) | Explicit memory-only session handoff | Accepted |
| [0011](0011-license-and-available-qualification.md) | Permissive licensing and available-machine qualification | Accepted |
| [0012](0012-qualification-publication-sequence.md) | Candidate acceptance before final-main qualification/publication | Accepted |
| [0013](0013-install-restart-click.md) | Install–restart–click workflow, visible companion and persistent Firefox integration | Accepted direction; mechanism/signing gates pending |
| [0014](0014-autonomous-completion.md) | Standing authority for autonomous completion; operational safeguards remain | Accepted owner instruction |
| [0015](0015-owned-windows-io-cancellation.md) | Explicit owned pipe cancellation; narrow reviewed FFI exception | Accepted engineering decision |

Each record states its reversibility. Security and correctness invariants are not made optional merely because an implementation choice is reversible.
