# ADR 0008 — Isolate publication from retained private history

- Status: accepted
- Date: 2026-09-08
- Work item: HalcyonXP/download-manager#41

## Context and intent

The user explicitly asked to finish privacy cleanup. GitHub rejected deletion of retained PR refs with HTTP 422 (`refs/pull/* is read-only`). Support requires a browser sign-in not available to this session; no request has been submitted. Rewriting writable history did not erase the original private email from GitHub-owned history.

The intended outcome is a safe publication target. Choosing the already documented separate-repository alternative is an implementation decision under delegated project authority, not a separately confirmed preference for two repositories. Deleting/recreating the original would lose important issue, review, and decision context and was rejected.

## Decision

- `HalcyonXP/download-manager` remains **private**, with the existing project board, issue numbering, work acceptance criteria, and complete decision history. The checkout's `origin` points here.
- `HalcyonXP/firefox-download-manager` is an independently created **non-fork** publication/CI target. The checkout's `publication` remote points here. It is still private pending an explicit visibility decision.
- Synchronize only reviewed, privacy-checked `main`, using a normal fast-forward push. Never mirror refs, force over divergent publication work, push old bundles, or copy retained original PR metadata.
- New Dependabot proposals in the target are new records, not imported historical PRs. Audit their refs/content too. Resolve code/dependency changes in the authoritative development repository before synchronizing `main`.
- Publication privacy clearance is repository-specific. Clearing the independent target does not erase or clear the original. The original must remain private unless GitHub separately removes its retained data.

## Evidence and consequences

The baseline target audit checked all 22 advertised branch/PR refs, 34 reachable commits, and 292 historical blobs. All 26 known original sensitive commit lookups returned the precise missing-commit response, while a known-good current commit remained readable. Seven fresh Dependabot PRs were included; three initial Dependabot update log archives were inspected and their run records removed. See [PUBLICATION_PRIVACY.md](../PUBLICATION_PRIVACY.md) for the repeatable verifier and later synchronized-head evidence.

GitHub attribution, public technical identifiers, and reviewed third-party attribution remain; this is not account anonymization. The public source tree retains links to the private planning record so history is not fabricated or silently renumbered. Ordinary readers can build the publication source without access to that private board.

No visibility, billing, protection, browser-profile, application behavior, or runtime networking setting changes are part of this decision. Privacy clearance is not hosted-CI success or installable-release qualification.
