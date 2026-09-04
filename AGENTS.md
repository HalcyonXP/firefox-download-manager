# Download Manager Implementation Guide

This repository contains a local download manager for Firefox Developer Edition on Windows 11.

## Source of truth

- Project board: https://github.com/users/HalcyonXP/projects/1
- Project plan: `docs/PROJECT_PLAN.md`
- Work items and acceptance criteria: GitHub Issues
- Start with the lowest-numbered issue in **Ready** status. At handoff, this is issue #1.

If code, documentation, and an issue disagree, stop and resolve the contradiction explicitly. Update the plan when scope changes.

## Scope constraints

- Use a Firefox WebExtension for capture, controls, and display.
- Use a Rust native helper for networking, scheduling, direct disk writes, validation, and recovery.
- Target Firefox Developer Edition and Windows 11 first.
- Begin with explicit **Download with Manager** actions for direct HTTP(S) URLs.
- Do not integrate with, inspect, configure, or route through Proton VPN. The helper uses the operating system's normal route.
- Do not add torrent, media-extraction, telemetry, analytics, cloud-sync, or remote-update behavior.
- Do not automatically intercept every Firefox download in the first release.
- Do not copy third-party implementation code unless its license and attribution requirements have been deliberately reviewed.

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

1. Confirm its dependencies and acceptance criteria.
2. Move it from **Ready** to **In Progress** on the project board.
3. Create a focused branch named `issue-<number>-<short-name>`.
4. Implement tests with the behavior whenever practical.
5. Run formatting, linting, unit tests, integration tests, and builds relevant to the change.
6. Commit and push the branch; open a pull request containing `Closes #<number>`.
7. Confirm the linked pull request moves the item to **In Review**.
8. Merge only when acceptance criteria are satisfied; closing or merging should move the item to **Done**.
9. Promote newly unblocked issue(s) from **Backlog** to **Ready**.

Project automations add new open repository issues and pull requests, place added items in **Backlog**, move linked pull-request work to **In Review**, move changes-requested work to **In Progress**, move closed/merged work to **Done**, and move reopened work to **Ready**. Verify automation outcomes rather than assuming they ran.

Keep commits and pull requests scoped to one issue unless two work items are inseparable and that decision is documented.

## Immediate implementation sequence

- #1 Architecture, scope, and security decisions
- #2 Versioned extension/helper protocol
- #3 Repository and CI scaffolding
- #4 Deterministic adversarial HTTP server
- #5 and #6 may then proceed in parallel

The detailed dependency graph and milestone exit criteria are in `docs/PROJECT_PLAN.md`.
