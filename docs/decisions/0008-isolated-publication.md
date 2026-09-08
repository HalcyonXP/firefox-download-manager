# ADR 0008 — Isolate publication from retained private history

- Status: superseded by [ADR 0009](0009-public-authority.md)
- Date: 2026-09-08
- Work item: [#30](https://github.com/HalcyonXP/firefox-download-manager/issues/30), transferred with its history

## Historical context and decision

The user asked to finish privacy cleanup. GitHub rejected deletion of retained PR refs with HTTP 422 (`refs/pull/* is read-only`). Support required a browser sign-in unavailable to the session; no request was submitted. Writable-history rewriting had not erased the private email from GitHub-owned originals.

Under delegated project authority, an independently created non-fork repository, `HalcyonXP/firefox-download-manager`, was selected instead of deleting/recreating the predecessor. The temporary decision kept development/issues in the private predecessor and synchronized only cleaned `main` to the independent target. At that time both were private. This was an implementation choice, not a separately confirmed preference for two repositories.

## Evidence retained

The baseline target audit checked all 22 advertised branch/PR refs, 34 reachable commits, and 292 historical blobs. All 26 known original sensitive commit lookups returned the exact missing-commit response while a known-good current commit remained readable. Seven fresh Dependabot PRs were included; three initial update log archives were inspected and their run records removed. The synchronized-head audit subsequently covered 35 commits and 306 blobs. See [PUBLICATION_PRIVACY.md](../PUBLICATION_PRIVACY.md).

GitHub attribution, technical identifiers, and reviewed third-party notices remained. The result established isolation, not erasure or account anonymity. It did not qualify a release or establish successful application CI.

## Supersession

The user then explicitly requested public visibility and consistent documentation referring to the new repository. #8 and ADR 0009 replace the temporary two-repository workflow with a **single public authority**. Regular work issues were transferred with their history; the predecessor remains private only as an archive. Do not follow the former synchronization procedure. The prohibition on importing sensitive original Git refs remains in force.
