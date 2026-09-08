# Publication privacy review

Review date: 2026-09-08. Requested after resource-identity issue #14; tracked as writable-surface cleanup #40 and platform-retention blocker #41.

**Keep this repository private. Writable history is cleaned, but publication is NOT yet privacy-cleared.** GitHub retains original ancestry through 14 closed pull-request head refs. A normal force-push cannot remove those read-only refs or guarantee removal of cached commit views. Resolve #41 before changing visibility. No visibility, billing, spending-limit, or account-privacy setting was changed.

## Scope and evidence

- Inspected all tracked first-party content and 278 distinct historical blobs across remote branches and PR heads; then additionally fetched all advertised PR merge refs. No private contact address, actual user-profile path, credential token, or private-key pattern was found in those blobs. Path examples are explicitly synthetic; URL user-info fixtures are intentionally adversarial test inputs.
- Found one private email in historical author/committer metadata and retained Actions run metadata. The address and old/new commit mapping are **not** in this document, issues, committed scripts, or a tracked `.mailmap`.
- Rewrote `main` and all five dependency branches using a temporary private mailmap and GitHub's noreply identity. Every branch tip tree was checked unchanged. Guarded, atomic, exact-lease pushes prevented clobbering intervening remote changes. Open dependency PRs remained mergeable. Commit IDs/signatures necessarily changed; prior issue/PR references describe the same code but may name pre-rewrite commits.
- Reviewed issue/PR bodies, issue comments and review comments for contact/private-path/token patterns, plus repository description/homepage, release/deployment metadata, repository/issue events, milestones, and artifact inventories. No corresponding content findings. No releases, deployments, forks, or enabled wiki existed at review time.
- Inspected 21 artifact ZIPs and 29 readable Actions log archives with the known-private-value and path checks. No matching private data in those ZIP contents. Run API metadata did contain the private address.
- Removed all 49 then-existing historical workflow runs (28 successful, 16 failed, 5 cancelled), their 21 artifacts, and 7 dependency caches. Old CI links can consequently be unavailable. This removes historical exposure surfaces; it does **not** restore Actions budget or claim those historical checks ran on new code. Fresh runs are allowed to exist with cleaned commit metadata.
- Re-fetched PR refs after rewriting: 14 closed PR heads still expose original private-address ancestry. This is a verified unresolved finding, not a hypothetical warning. No claim of complete erasure or publication readiness is made.

## Retained attribution and non-findings

GitHub repository ownership, account handles, bot authors, and noreply identities remain attributable to their GitHub accounts. The fixed extension/native-host principal and per-user application namespace are compatibility identifiers, not contact addresses; changing them gratuitously would break registration/recovery. This cleanup does not make the owner's GitHub account anonymous. Reviewed third-party license/attribution content is preserved. No real name, postal address, phone number, or identifiable workstation path was found in inspected first-party text; pattern checks and manual review cannot prove arbitrary prose contains no identifying information.

Local build output, temporary browser profiles/screenshots, runtime state, logs, and audit material are excluded from publication. Never publish a ZIP containing `.git`, local state, or automation sessions. Exact runtime download URLs can legitimately be sensitive; do not commit or upload them as issue attachments.

## Repeatable guard

```powershell
npm run privacy:test
npm run privacy:check
npm run privacy:history
```

The guard scans tracked content for non-allowlisted email addresses, actual profile-path patterns, credential/private-key patterns, and accidental runtime artifacts. The history mode additionally checks **all commits reachable from HEAD** for private author/committer addresses and sensitive messages. It refuses shallow history; CI checks out full history. Output contains only file indexes, line numbers, categories, and counts, never matched private values. The policy has tests using assembled fake data, not real contact information.

This guard is not a secrets scanner for every token type, an arbitrary-real-name detector, or a GitHub-retention audit. It does not scan other remote branches or PR refs automatically. A successful guard must not be interpreted as resolution of #41.

## Required GitHub-side action

Use [GitHub's sensitive-data removal process](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/removing-sensitive-data-from-a-repository) to request removal/dereferencing of the original commit views and closed PR refs. GitHub decides whether and how it will process this personal-data request; support eligibility/outcome has not been confirmed. Local-only original commit/ref details are available for an authenticated support request, not public issue text. After support work, re-fetch every PR head/merge ref and inspect old commit API/patch views before clearing #41.

If GitHub cannot remove the retained data, a separate sanitized publication repository can avoid carrying this repository's PR history, while preserving this original privately for its issues/decisions. That alternative has **not** been created and this repository must not be silently deleted or recreated.

## Collaborator recovery after the rewrite

Prefer a fresh clone. Preserve local uncommitted work privately first, and reapply it onto the cleaned history. Do not merge or push old branches, old bundles, audit refs, or pre-rewrite clones: they can reintroduce the address. First-party commits should use the account's GitHub-provided noreply address. The repository-local Git identity has been set accordingly without weakening GitHub's email-privacy protection.
