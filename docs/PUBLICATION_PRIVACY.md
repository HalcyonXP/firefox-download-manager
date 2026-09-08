# Publication privacy review

Review date: 2026-09-08. Requested after resource-identity issue #14; tracked as writable-surface cleanup #40 and platform-retention blocker #41.

## Current disposition: two distinct repositories

- **Publication/CI target:** [HalcyonXP/firefox-download-manager](https://github.com/HalcyonXP/firefox-download-manager). Independently created, not a fork. Its initial publication privacy audit passed; this is the only designated candidate for future public visibility. It remains private until the user requests a visibility change.
- **Private development/history repository:** [HalcyonXP/download-manager](https://github.com/HalcyonXP/download-manager). **Keep it private.** GitHub still retains original private-email ancestry in 14 closed PR refs. Those originals were not erased; isolation avoids publishing them.

The user asked to finish cleanup. #41 therefore selected the previously documented separate-target alternative rather than silently deleting/recreating the original. [ADR 0008](decisions/0008-isolated-publication.md) records the decision and its limits. No visibility, billing, protection, account-privacy, or live Firefox-profile setting was changed.

## Isolated-target evidence (2026-09-08)

- Baseline fresh fetch: 22 branch/PR refs, 34 reachable commits, 292 distinct historical blobs, and 116 historical paths. Checked all commit contacts/messages and file content, not just the working tree.
- All 26 known original sensitive commit hashes returned GitHub's exact HTTP 422 missing-commit response in the target. A known-good current commit remained readable, so this was not an authentication failure masquerading as absence.
- Seven new Dependabot PRs appeared automatically after the first push. They were included in the audit; no old PR records were transferred. Their public GitHub support sign-offs and SSH routing principal were reviewed as technical attribution, not private contact data. Author/committer contacts still require noreply identities.
- Inspected three new Dependabot update log ZIPs (21 files), with no private-address or contact/credential-pattern candidates, then removed those three run records. Remaining application CI jobs had not started; no target artifacts, caches, releases, deployments, or forks existed.
- Repository metadata, all-state issue/PR records, comments, events, milestones, and remaining Actions metadata were checked. No known private-value or policy-pattern findings remained. Re-run verification after each preparation push; the report records the exact checked target `main` commit.

This establishes scoped publication clearance, not anonymity or proof about arbitrary identifying prose. The original repository remains deliberately outside that clearance.

## Earlier writable-history cleanup (#40)

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

This guard is not a secrets scanner for every token type, an arbitrary-real-name detector, or a GitHub-retention audit. It does not scan other remote branches or PR refs automatically. A successful HEAD guard alone does not establish platform clearance or authorize publishing the original repository.

## Original-repository retention (still unresolved)

Use [GitHub's sensitive-data removal process](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/removing-sensitive-data-from-a-repository) to request removal/dereferencing of the original commit views and closed PR refs. GitHub decides whether and how it will process this personal-data request; support eligibility/outcome has not been confirmed. Local-only original commit/ref details are available for an authenticated support request, not public issue text. After support work, re-fetch every PR head/merge ref and inspect old commit API/patch views before ever considering public visibility for the original repository.

The separate sanitized target has now been created and audited under #41. The original is preserved privately for its issues/decisions; it was not deleted, renamed, transferred, or recreated. A private Support-request draft and recovery bundle remain outside the checkout, not in either repository's published refs.

## Repeat the initial-target verification

The authenticated CLI verifier needs two local JSON inputs kept **outside** the checkout: `{ "emails": [...] }` containing the private comparison value(s), and an array of known original commit hashes. Do not copy these values into examples, command arguments, tracked files, CI secrets, or public reports. The existing inputs are under the local `DownloadManagerPrivacyAudit` directory.

```powershell
$audit = Join-Path $env:LOCALAPPDATA "DownloadManagerPrivacyAudit/2026-09-08"
npm run privacy:publication -- (Join-Path $audit "private-identifiers.json") (Join-Path $audit "original-commits.private.json")
```

The verifier uses a disposable fresh bare clone, fetches every target branch/tag and PR head/merge ref, scans all reachable historical blobs/paths and commit identities/messages, checks GitHub metadata, and probes every known original hash. It requires both repositories to remain private for this initial check. Authentication errors, generic 404s, partial pagination, missing inputs, unexpected artifacts, or started jobs are not treated as success. Started jobs require a separate full log/artifact review before this initial gate can pass. Its JSON report contains counts and a cleaned target commit, never private comparison values. The original Support/recovery kit must remain private.

This is a pre-publication verifier, not a claim to comprehensively inspect all future workflow logs or detect arbitrary real names/photos. Routine `privacy:history` remains in CI; repeat platform/log review when surfaces change.

## Collaborator recovery after the rewrite

Prefer a fresh clone. Preserve local uncommitted work privately first, and reapply it onto the cleaned history. Do not merge or push old branches, old bundles, audit refs, or pre-rewrite clones: they can reintroduce the address. First-party commits should use the account's GitHub-provided noreply address. The repository-local Git identity has been set accordingly without weakening GitHub's email-privacy protection.
