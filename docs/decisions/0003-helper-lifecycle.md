# ADR-0003: Run an on-demand user-scoped native helper

- Status: Accepted
- Date: 2026-09-04
- Reversibility: Moderate

## Context

A permanently installed service or unauthenticated local daemon would allow downloads to continue independently of Firefox, but creates service management, IPC authentication, update, privilege, and attack-surface obligations. The initial requirements demand safe restart and recovery, not unattended operation after the browser exits.

## Decision

Firefox launches the helper through Native Messaging under the interactive user's account. The helper owns active work while connected. On EOF, shutdown, or browser exit it stops new network assignments, reaches safe writer checkpoints, persists recoverable state, and exits. A later connection starts a helper that validates and reconstructs state.

Only one writer may own a task/state directory. An exclusive user-scoped state-root lock and per-task controls prevent concurrent helper instances from writing the same task. Unexpected termination is handled by crash-safe metadata and conservative recovery.

The issue-#10 implementation registers an ordinary executable only under the current user's Firefox Native Messaging key. It has no service, listener, scheduled task, elevated component, or independent route/VPN behavior. Standard output is framing-only. Clean EOF and protocol/output errors all pass through cooperative engine shutdown; active downloads checkpoint and pause, incomplete probe/validation fails safely, and a later process emits persisted snapshots after hello.

## Rejected alternatives

- **Windows service:** unnecessary privilege and installation complexity for the initial local release.
- **Always-running tray daemon:** requires a second authenticated IPC protocol and independent lifecycle/UI.
- **Detach after Native Messaging EOF:** risks orphaned and duplicate engines when Firefox reconnects.
- **Keep all state in the extension:** cannot satisfy helper/browser restart recovery safely.

## Consequences

Downloads do not continue after Firefox/helper shutdown in the initial release; they recover safely when the helper starts again. A future background daemon requires a new ADR, authenticated local IPC, singleton ownership, migration, and installer/security review.
