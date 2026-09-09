# Download Manager Implementation Guide

This repository contains a local download manager for Firefox Developer Edition on Windows 11.

## Source of truth

- Canonical public repository: https://github.com/HalcyonXP/firefox-download-manager
- Project board (owner-private planning view): https://github.com/users/HalcyonXP/projects/1
- Project plan: `docs/PROJECT_PLAN.md`
- Work items and acceptance criteria: this repository's GitHub Issues.
- `origin` must refer to the canonical repository for maintainer work. There is no publication mirror or synchronization workflow. The private predecessor is an archive, not a development target; never push its old history.
- Start with the lowest-numbered issue in **Ready** status. Existing work was transferred, not restarted; see `docs/ISSUE_MIGRATION.md` for historical numbering. The released v0.1.0 work is complete. Owner feedback now drives M5/#48–#53; use the board for current Ready status, not historical handoff numbers.

If code, documentation, and an issue disagree, stop and resolve the contradiction explicitly. Update the plan when scope changes.

## Scope constraints

- Use a Firefox WebExtension for capture, controls, and display.
- Use a Rust native helper for networking, scheduling, direct disk writes, validation, and recovery.
- Target Firefox Developer Edition and Windows 11 first.
- v0.1.0 began with explicit **Download with Manager** actions. For the next release, the owner requires setup.exe → visible tray companion → install XPI once → restart Firefox → ordinary download click automatically starts in Manager. Follow ADR0013 and M5; this is not implemented by the released manual workflow.
- Do not integrate with, inspect, configure, or route through Proton VPN. The helper uses the operating system's normal route.
- Do not add torrent, media-extraction, telemetry, analytics, cloud-sync, or remote-update behavior.
- The immutable first release remains manual-only. M5 permits safe supported ordinary-click capture, not blanket interception/replay of every request, POST/blob/private/container/auth context or implicit credential harvesting.
- Persistent installation must use an authorized Mozilla-supported signed path with protections unchanged, not temporary-addon reloads or profile injection. Signing/account authority is not assumed.
- The owner is using Firefox again. Earlier closed-browser authorization is not current authority for new browser/registration tests; obtain fresh consent and preflights and use only owned isolated state.
- Do not copy third-party implementation code unless its license and attribution requirements have been deliberately reviewed.

## User-facing workflow

Keep the quick start to run setup, see tray, install XPI, restart Firefox and click a download link. Keep maintainer/provenance/receipt commands in separate documentation. Do not claim that M5 is delivered or install-ready until that actual workflow passes, or replace it with the rejected right-click/temporary-addon workaround. Preserve version-specific v0.1.0 history rather than relabeling its qualification.

## Correctness and security invariants

1. Accept a ranged response only when its status and `Content-Range` match the requested assignment.
2. Never merge overlapping, missing, out-of-bounds, or resource-inconsistent bytes.
3. Never resume across a known resource identity change.
4. Never expose the final file before successful validation and promotion from partial state.
5. Never silently overwrite an existing final file.
6. Never place cookies, authorization values, or sensitive URL data in ordinary logs.
7. Treat native messages, URLs, headers, filenames, settings, and persisted state as untrusted input.
8. Prefer a safe single-stream fallback or an explicit failure over optimistic behavior that can corrupt output.

## GitHub workflow

Saved project views are available for the status board, current milestone, native helper, Firefox extension, security work, and all work.

For each issue:

1. Confirm its dependencies and acceptance criteria. Add the issue to the project explicitly if absent (`gh project item-add 1 --owner HalcyonXP --url <issue-url>`); do not assume auto-add targets this repository.
2. Move it from **Ready** to **In Progress** on the project board.
3. Create a focused branch named `issue-<number>-<short-name>`.
4. Implement tests with the behavior whenever practical.
5. Run formatting, linting, unit tests, integration tests, and builds relevant to the change.
6. Commit and push the branch; open a pull request containing `Closes #<number>`.
7. Add the pull request to the project if absent and confirm the linked work moves to **In Review**.
8. Merge only when acceptance criteria are satisfied; closing or merging should move the item to **Done**.
9. Promote newly unblocked issue(s) from **Backlog** to **Ready**.

The existing status workflows cover added items, linked PRs, changes requested, closure/merge, and reopening. Their outcomes must be verified. The inherited auto-add workflow targeted the predecessor; this session has not reconfigured it through the unavailable browser settings UI. Explicitly add new issues/PRs and verify status. Stale predecessor PR cards have been removed from the active board without deleting their private records.

Keep commits and pull requests scoped to one issue unless two work items are inseparable and that decision is documented.

## Foundation sequence (completed; current issue numbers)

- #9 Architecture, scope, and security decisions
- #10 Versioned extension/helper protocol
- #11 Repository and CI scaffolding
- #12 Deterministic adversarial HTTP server
- #13 and #14 may then proceed in parallel

The detailed dependency graph and milestone exit criteria are in `docs/PROJECT_PLAN.md`.
