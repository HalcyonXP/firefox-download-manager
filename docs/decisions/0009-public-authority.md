# ADR 0009 — One public authoritative repository

- Status: accepted
- Date: 2026-09-08
- Work item: [#8](https://github.com/HalcyonXP/firefox-download-manager/issues/8)
- Supersedes: [ADR 0008](0008-isolated-publication.md)

## Confirmed intent and implementation choice

The user explicitly requested making the new repository public and ensuring all documentation refers to it. Merely replacing repository URLs would point old work references at unrelated new issue numbers and leave two competing development instructions. Moving the regular issues and consolidating authority is the implementation choice used to fulfill that request; no new license preference or release readiness was inferred.

## Decision

- `HalcyonXP/firefox-download-manager` is public and authoritative for source, issues, milestones, CI, and future releases. Maintainer `origin` points here. Branches and PRs are created directly here; there is no mirror synchronization step.
- Transfer the 22 regular issues, preserving states, creation times, authors, comments, and acceptance meaning. Recreate five milestone identities, preserve labels/board status, and update all current references using [ISSUE_MIGRATION.md](../ISSUE_MIGRATION.md).
- Historical PR refs are not transferred. Cleaned public commits and transferred work history preserve implementation provenance; original review records remain in the private predecessor archive. Never publish or import its sensitive original Git history.
- The owner-private project board remains a planning view. Remove stale predecessor PR cards, add current PRs, and explicitly add new work until repository auto-add is configured/verified. Generic status automation must be checked, not assumed.
- Update current protocol schema repository metadata to the canonical URL without changing wire versions, payloads, or extension/native-host compatibility identifiers.

## Verification and limits

Before the visibility switch, the full audit checked 22 refs, 35 commits, 306 historical blobs, 30 issue/PR records, and all 26 known original sensitive hashes. Generated update logs were separately reviewed. Public/non-fork status was then verified without authentication; the predecessor remained private.

A local recurrence guard rejects retired repository references in current tracked documentation/metadata and directs users to the canonical namespace. Historical commit messages are not rewritten to pretend they used today's issue numbers. Workflow/installation/Firefox/release qualification is tracked separately; public visibility does not turn a blocked or failing test into a pass. Billing, branch protections, account identity, live Firefox profiles, and application behavior were not changed by this decision.
