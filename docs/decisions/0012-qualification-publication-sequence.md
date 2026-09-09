# ADR 0012: Separate candidate implementation acceptance from final publication

- Status: Accepted
- Date: 2026-09-09
- Work items: #28 / PR #43, followed by #46

## Context and confirmed intent

The owner authorized autonomous delivery of a qualified, versioned, checksummed personal release. MIT and qualification on the existing Windows11 x64 computer remain as recorded in ADR0011. The owner did not specifically request this issue split; it is our implementation/workflow decision for that unchanged outcome.

Three prior requirements formed a sequencing conflict: #28 included publication in its acceptance checklist; the implementation guide permits merging only after acceptance; the qualification plan requires authoritative merged-main CI before publication. One closes-on-merge implementation PR cannot satisfy all three in that order. Silently merging with an incomplete publication criterion or calling a PR candidate “merged-main evidence” would be misleading.

## Decision

1. #28 / PR43 owns implementation/candidate qualification: all functional, adversarial, controls/recovery, environment, resource, documentation and harness-safety acceptance. `RELEASE_MATRIX.md` states the layered evidence and support boundaries; it never relabels Rust tests/mocks as additional exact-artifact/browser cases.
2. Move the versioned-publication criterion, final-main artifact/source/tag qualification and final public-input review to dependent **#46**. This is a tracked transfer, not a checked-off or waived criterion. M4 and the owner-facing release remain incomplete until #46 passes.
3. Merge PR43 only after its revised candidate criteria and current PR CI pass. Require authoritative merged-main CI, then promote #46 to Ready. Failure in merged-main CI blocks #46; previous candidate success is not a waiver.
4. #46 freezes the intended main commit and exact candidate bytes, reruns the clean identified native/Firefox/installer/resource gates against them, completes final privacy/log/artifact/cache/release-input review and publishes a new source-linked versioned/checksummed release. No untested rebuild may replace those bytes.
5. Verify tag/assets/public release identity and issue/board/milestone outcomes. Documentation/evidence follow-up may describe an immutable qualified tag; it must not imply a later different main candidate was itself released or tested as those bytes.

No extra computer/account, protection downgrade, unowned process/profile access, unknown-registration adoption or scope expansion is authorized. Final testing still requires both Firefox editions closed normally and a fresh ownership/registration preflight. Checksums and notices do not establish publisher authenticity.

## Consequences and reversibility

Closing #28 means the qualified candidate implementation is accepted, **not that a release exists or is install-ready**. #46 remains the critical M4 exit item until final qualification/publication completes. All original release requirements continue in the project plan/issues, with their failure history and unavailable clean-OS disclosure intact.

The administrative split is reversible by reconciling issue/plan dependencies without losing any gate. A release's source, published checksums/evidence and already granted license rights must not be retroactively rewritten to match later inputs or withdrawn by a workflow change.
