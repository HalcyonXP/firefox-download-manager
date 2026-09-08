# ADR 0010: Origin-confined, memory-only session handoff

- Status: accepted for #23
- Date: 2026-09-08

## Context

The accepted scope calls for ordinary authenticated direct downloads, not arbitrary request cloning. V2 already reserves `add.request_context`, but `resume` carries only a task ID, and its cookie shape lacks store/partition identifiers. Broad session reconstruction would exceed that contract and privacy scope.

## Decision

Use unchecked, per-download handoff from the normal default cookie store with optional cookies/site permissions. Reject private/container/ambiguous source contexts and exclude partitioned/first-party-isolated cookies rather than borrow another session. Document Firefox's scheme/host permission granularity separately from the helper's exact-origin confinement. Permit only explicit same-origin referrers and HTTPS Basic/Bearer values. Block context-bearing origin changes before contact; re-evaluate cookie paths and expiry on every request.

Keep secrets out of persistent types and retain only a required `needs_session` marker in internal format v3. Strict v1/v2 migration remains supported; wire v2 is unchanged. After restart or terminal authentication failure, new credentials require a new task, not reauthorization of existing bytes. Reset opt-in after each submission and provide permission revocation. No live browser profile is modified by development qualification.

## Consequences and alternatives

This deliberately cannot handle every site, Total Cookie Protection partition, session rotation, cross-origin authenticated CDN redirect, or arbitrary authorization scheme. Refusing those cases is preferable to attaching a different context to retained bytes or leaking credentials. A future richer session/refresh contract would require a separate issue and wire-version decision; the current implementation does not imply user approval of that expansion.

Raw cookie-jar persistence, all-origin permissions at installation, redirect forwarding based merely on parent-domain membership, and silent unauthenticated resume were rejected. Standard date parsing uses the reviewed MIT/Apache-2.0 `time` dependency, not a custom calendar implementation. Test instrumentation keeps only synthetic-session boolean assertions.
