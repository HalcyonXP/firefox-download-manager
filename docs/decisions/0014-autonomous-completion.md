# ADR 0014: Standing authority to complete the owner workflow

- Status: Accepted owner instruction
- Date: 2026-09-10
- Scope: M5/#49–#53 and ongoing #50/PR56
- Supersedes: earlier requirements to obtain another routine approval before each in-scope implementation, isolated test or reviewed distribution step

## Confirmed instruction

The owner explicitly says no further approval or authorization is needed within this project's scope, asks that GitHub manage the project, and directs autonomous development until complete and ready for their installation. This is stronger than the earlier approval of the infographic and the short Proceed message. Do not keep interpreting those older checkpoints as the current authority boundary.

## Operational meaning

Continue implementation, dependency review, tests, CI, issue/board maintenance, scoped commits/PRs, acceptance-based merges, qualification and new release publication without routine approval requests. In-scope owned isolated browser/setup testing is authorized. Reviewed Mozilla signing/submission work necessary for persistent installation is authorized when legitimate publisher credentials/account access and required terms are available; this instruction does not manufacture them. Do not seek another blanket approval instead of doing available work.

Operational safety preflights remain real conditions, not approval rituals. Registration is shared across Firefox editions/profiles, so installation/upgrade/removal/registration tests still require both editions closed normally and conservative ownership checks. Never kill an unowned browser or change its profile to bypass that condition. A separately reviewed API-only browser experiment with a unique owned profile/add-on and no native-host registration must establish its own isolation; do not simply remove preflights from the existing combined qualification driver.

Normal Firefox profiles and existing downloads are not test fixtures. No signing/TLS/OS-protection downgrade, elevation, VPN changes, telemetry, remote updater, silent logon startup, unowned process termination or private-history publication is added to scope. Preserve immutable v0.1.0 assets. Ready for the owner to install does not mean automatically installing into their normal profile now.

If a genuinely external prerequisite is unavailable (for example publisher credentials, identity/account terms, or an occupied browser environment required for shared-registration work), state the precise missing condition, keep it visible on GitHub and continue independent work. Never solicit secrets in public issues/logs, claim signing has occurred without returned signed bytes, or label unfinished previews as ready.

The acceptance goal remains ADR0013's setup → visible tray → signed install-once XPI → Firefox restart → ordinary download click → one automatic native task and independently correct output. No gate is waived by autonomous authority.
