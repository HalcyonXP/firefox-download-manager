# Publication privacy record

## Current status

**[HalcyonXP/firefox-download-manager](https://github.com/HalcyonXP/firefox-download-manager) is public and authoritative.** The user explicitly authorized this transition in #8. Code, work issues, milestones, CI, and future releases belong here; there is no separate publication-mirror workflow. [ADR 0009](decisions/0009-public-authority.md) supersedes the earlier temporary arrangement.

The private predecessor is retained only as an archive. GitHub still holds original private-email ancestry in 14 of its closed PR refs. Those originals were **isolated, not erased**; never import them or publish the archive. Regular work issues were reviewed and transferred with their history, without transferring private PR Git refs. [ISSUE_MIGRATION.md](ISSUE_MIGRATION.md) maps their new numbers and historical implementations.

## Verification before public visibility — 2026-09-08

- Independently created, non-fork repository; 22 advertised branch/PR refs, 35 reachable commits, 306 distinct historical blobs, and 120 historical paths checked.
- All 26 known original sensitive commit hashes returned the exact HTTP 422 missing-commit response in this repository. A known-good current commit remained readable, ruling out an authentication failure masquerading as absence.
- Thirty issue/PR records after work transfer, comments, events, milestones, repository metadata, and remaining Actions metadata checked without known-private-value or policy-pattern findings.
- Fresh Dependabot records were included rather than assuming an empty repository stayed empty. Generated update log archives were reviewed for private values/contact/credential patterns, then their run records removed. No application jobs had started and no artifacts/caches/releases/deployments/forks existed at the pre-switch check.
- Public repository access and non-fork status were verified without authentication after the switch. The archive remained private. No billing, branch-protection, account-identity, or live Firefox-profile setting changed.

These are scoped, time-specific checks, not account anonymity or a guarantee about future uploads. Subsequent public CI logs/artifacts require their own inspection and do not inherit a blanket clearance.

## Earlier cleanup and learning history

#29 reviewed tracked first-party content and 278 historical blobs across predecessor branches/PRs, issue/PR text, metadata, Actions logs, and artifacts. One private email was found in commit author/committer metadata and retained run metadata, not in the inspected source blobs. The email and original commit maps are not in tracked files, issues, public reports, or a committed mailmap.

All six writable predecessor histories were metadata-rewritten to GitHub noreply identities, preserving every tip tree. Guarded exact-lease pushes avoided clobbering intervening work. The cleanup reviewed 21 artifact ZIPs and 29 readable log archives, then removed 49 historical runs, 21 artifacts, and seven caches. Old CI URLs may therefore be unavailable; deleting them neither restores budget nor establishes new test evidence.

GitHub rejected deletion of retained PR refs as read-only. Support browser sign-in was unavailable and no request was submitted. #30 selected the independent-target alternative documented in [superseded ADR 0008](decisions/0008-isolated-publication.md). The later public-authority decision transferred regular work issues instead of deleting their decision history.

The detectors were corrected narrowly for project-board URL prose, GitHub's SSH routing principal, and Dependabot's published support sign-offs. The latter are public technical attribution, not private owner contact data; author/committer addresses still require noreply identities.

## Repeatable checks

```powershell
npm run privacy:test
npm run privacy:check
npm run privacy:history
npm run repository:check
```

The normal `npm run check` includes these local policy/repository gates. The HEAD-history guard refuses shallow history and checks author/committer emails and commit messages; its content scan covers tracked working files. It is not an all-ref, historical-blob, platform-log, arbitrary-real-name, or photo audit.

For an authenticated remote Git/platform-metadata review, use private comparison inputs kept **outside** the checkout:

```powershell
$audit = Join-Path $env:LOCALAPPDATA "DownloadManagerPrivacyAudit/2026-09-08"
npm run privacy:publication -- (Join-Path $audit "private-identifiers.json") (Join-Path $audit "original-commits.private.json") --metadata-only
```

The first local JSON contains the private `emails` array and the `archiveRepository` identifier; the second contains known original sensitive commit hashes. Never paste their contents into command arguments, tracked files, CI secrets, or public issues. The verifier fresh-fetches every current branch/tag and PR head/merge ref, checks reachable blobs/paths and contact metadata, probes known originals, and verifies that the archive remains private. Authentication errors, generic 404s, partial pagination, and changing refs are failures, not proof of absence.

**Metadata-only success does not review workflow log ZIPs, build artifacts, or cache contents.** Counts and this limitation appear in its report. Without `--metadata-only`, the initial full-surface gate refuses started jobs or unreviewed artifacts; it must not silently approve those now that public CI can run. Review the relevant archives separately and record the exact commits/run/artifact identities. Diagnostics withhold private values and paths.

## Retained attribution and local handling

GitHub handles, noreply identities, fixed extension/native-host principals, and reviewed third-party notices remain attributable. This is not an anonymity promise or a detector for arbitrary identifying prose. Synthetic path/URL/authentication fixtures are not real user data. No first-party project license was selected by the visibility change. The owner subsequently clarified permissive FOSS intent; [ADR 0011](decisions/0011-license-and-available-qualification.md) records the later MIT decision, without changing privacy/attribution rules.

Private comparison inputs, Support draft, and original recovery bundle remain outside the checkout. Never upload `.git`, audit kits, browser profiles/screenshots, settings/state, diagnostics, cookies, authorization values, signed URLs, or downloaded partials. Prefer a fresh canonical clone; privately preserve and reapply uncommitted work rather than merging pre-scrub branches. The runtime application remains local-only, with no telemetry, cloud synchronization, or remote updater.


## Final-publication metadata coverage correction (#46)

The pre-tag main-d03 audit at 2026-09-09T14:01:08Z reported28 issue/PR records, while independent REST reads and GraphQL totals showed28 regular issues plus18 PRs (46 distinct numbers). The original raw platform response was not retained; its omitted records and cause are not reconstructed. That snapshot is not accepted as complete PR-metadata coverage merely because its other checks passed.

The publication auditor now reads full pull-request metadata independently of the issues endpoint, reconciles distinct regular-issue and PR numbers against separate GraphQL totals, and refuses missing/duplicate/overlapping/inconsistent records. Issues-endpoint PR entries are not double-counted. Pure tests cover both mixed and issues-only responses, malformed numbers/totals and omissions; a passing baseline followed by removal of the pull-count comparison fails the intended assertion, then restored tests pass. An actual corrected remote audit is still required before publication, not an unchanged retry of the earlier incomplete observation.

The report records the auditor revision/dirty status separately from the remotely inspected main commit. Publication-tool/documentation follow-up is not an untested replacement for the frozen product: intended0.1.0 source remains main `d03a56c373bfee37776d23031908a93fc68da89a`, whose exact CI bytes and clean-d03 native/Firefox/setup drivers passed. This distinction is preserved when the auditor is newer than the product tag.


The first corrected remote run refused at the new coverage stage before counts were retained. A separate Node API probe then observed consistent28-issue/19-PR coverage after PR47 was created. Counts-only, bounded, canary-tested failure diagnostics were added; the diagnostic dirty-driver run passed47-record reconciliation. Neither later response reconstructs the earlier refusal. Missing-pull counterexamples/mutations establish the coverage contract; diagnostic-only success is not the final clean-auditor gate. No API assertion was relaxed to obtain that result.
