# ADR-0003: Run an on-demand user-scoped native helper

- Status: Accepted
- Date: 2026-09-04
- Reversibility: Moderate

## Context

A permanently installed service or unauthenticated local daemon would allow downloads to continue independently of Firefox, but creates service management, IPC authentication, update, privilege, and attack-surface obligations. The initial requirements demand safe restart and recovery, not unattended operation after the browser exits.

## Decision

Firefox launches the helper through Native Messaging under the interactive user's account. The helper owns active work while connected. On EOF, shutdown, or browser exit it stops new network assignments, reaches safe writer checkpoints, persists recoverable state, and exits. A later connection starts a helper that validates and reconstructs state.

Only one writer may own a task/state directory. A named user-scoped ownership primitive and task locks prevent concurrent helper instances from writing the same task. Unexpected termination is handled by crash-safe metadata and conservative recovery.

## Rejected alternatives

- **Windows service:** unnecessary privilege and installation complexity for the initial local release.
- **Always-running tray daemon:** requires a second authenticated IPC protocol and independent lifecycle/UI.
- **Detach after Native Messaging EOF:** risks orphaned and duplicate engines when Firefox reconnects.
- **Keep all state in the extension:** cannot satisfy helper/browser restart recovery safely.

## Consequences

Downloads do not continue after Firefox/helper shutdown in the initial release; they recover safely when the helper starts again. A future background daemon requires a new ADR, authenticated local IPC, singleton ownership, migration, and installer/security review.
