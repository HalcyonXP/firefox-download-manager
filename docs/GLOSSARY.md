# Shared project vocabulary

- **Manager**: the Firefox UI plus its on-demand Rust native helper, not Firefox's built-in downloads.
- **Explicit capture**: a link context-menu action or pasted direct HTTP(S) URL; never automatic interception.
- **Proposed filename**: a Windows-safe name derived from the URL or entered by the user. Issue #11 resolves this before submission; server metadata cannot choose a path. Collision suffixes are selected safely at final promotion.
- **Worker**: one transfer lane (1/2/4/8), not permission to exceed the separate global/per-host request caps.
- **Partial**: helper-managed, unvalidated download storage; not final output.
- **Checkpoint**: flushed completed ranges followed by durable metadata, in that order.
- **Snapshot**: the helper's authoritative task projection; cached UI state is explicitly stale when disconnected.
- **Promotion**: no-overwrite publication only after complete coverage and validation.
- **Ready to install**: versioned, checksummed artifacts and documented setup with release gates passed. It does not imply installation into the user's existing Firefox profile.
- **Qualification gap**: a release criterion for which evidence is missing. Code presence or a mock test is not end-to-end evidence.

## Execution record — 2026-09-08

The user explicitly authorized autonomous development within scope. Installation readiness is the intended outcome; changing their live browser profile is not required to achieve it. Baseline `bdab5bc` is clean, GitHub authentication has repository/project/workflow access, and Windows/MSVC Rust plus Node tools are present. Issues #1–#10 are Done; #11, #14, #16 are Ready. Proceed in numeric Ready order.

Issue #11 acceptance uses “resolved filename”; protocol v1 has no preview command. The implemented interpretation is a visible, editable, Windows-safe **proposed filename** and explicit existing destination, with collision-safe final naming by the helper. This is an implementation decision under the user's delegated authority, not a separately confirmed user preference. Referrers remain absent until the opt-in session policy in #15; no broad host permission is needed for creation.

- **Strong resource identity** (#14): equal final URL, size, mode and validators, including a strong ETag; weak tags and dates alone never justify combining persisted/request byte ranges.

## Publication terms (#40/#41)

- **Writable-history cleanup**: removal from editable branches/tags and tracked content; it does not imply erasure from GitHub caches or closed-PR refs.
- **Publication privacy-cleared**: a repository-specific result after content/history and platform-surface inspection. #41 clears the independent target, not GitHub-retained originals in the private development repository.
- **Public attribution**: GitHub account handles, noreply identities, and reviewed project/third-party identifiers. These remain attributable and are not an anonymity promise.

The user explicitly requested privacy preparation after #14 because private Actions budget is exhausted. They have not requested a visibility change in this task. #14 is complete; #40 takes precedence over further feature work, with unresolved platform-owned findings preserved in #41. Metadata-only rewriting preserved every writable branch tip tree; prior commit hashes/CI run links may no longer identify current records.


### Completion follow-up (#41)

- **Development/history repository** (`origin`): `HalcyonXP/download-manager`, always private unless retained originals are separately removed. Existing numbered work references and the project board remain authoritative here.
- **Publication/CI target** (`publication`): `HalcyonXP/firefox-download-manager`, independently created rather than forked. Only reviewed, checked `main` is synchronized; do not mirror refs or merge publication-only dependency proposals directly.
- **Isolation, not erasure**: original private GitHub records remain inaccessible to public readers because the original stays private; they were not removed by creating the clean target.

The user explicitly requested finishing cleanup. Choosing the documented independent-target alternative under delegated authority is an implementation decision, not a separately confirmed repository-layout preference. GitHub rejected PR-ref deletion; Support sign-in was unavailable and no request was submitted. Fresh Dependabot records were audited rather than assuming a new repository stayed empty. Public GitHub support sign-offs are retained third-party attribution, while commit author/committer emails remain noreply-only.
