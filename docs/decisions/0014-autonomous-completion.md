# ADR 0014: Integration qualification and operational safety boundaries

- Status: Accepted; distribution scope superseded by [ADR0016](0016-unsigned-personal-xpi.md)
- Date: 2026-09-10
- Scope: M5/#49–#53 and #50/PR56

## Qualification boundaries

Implementation, dependency review, tests, CI, issue/board state, scoped commits/PRs, acceptance-based merges and release qualification remain separate evidence layers. Do not label an unfinished preview as an install-ready release. Current source and exact artifacts require their own checks; older successful runs do not qualify changed bytes.

Registration is shared across Firefox editions/profiles. Installation, upgrade, removal and shared-registration tests require both editions closed normally and conservative ownership checks. Never terminate an unowned browser or change its profile to bypass that condition. A separately reviewed API-only browser experiment with a unique owned profile/add-on and no native-host registration must establish its own isolation; do not remove preflights from the existing combined qualification driver.

Normal Firefox profiles and existing downloads are not test fixtures. Keep existing Firefox settings and signing, TLS and OS protections unchanged. No elevation, VPN changes, telemetry, remote updater, silent logon startup, unowned process termination or private-history publication is introduced. Preserve immutable v0.1.0 assets. Installation qualification uses owned isolated state, not automatic installation into normal profiles.

If an execution condition is unavailable, such as a closed browser environment required for shared-registration work, record the precise technical limitation and continue independent work. Unknown process ownership or failed cleanup prevents a success result; preserve the domain rather than adopting or deleting unowned state.

## Distribution and acceptance

The target is a **persistent unsigned personal XPI**. No signed XPI, Mozilla submission, signing account or credentials are required. This replaces the earlier signing/submission prerequisite. Existing Developer Edition compatibility, a settings change and observed exact-artifact persistence are distinct; no settings change or normal-profile inspection is introduced to qualify unsigned installation.

The acceptance sequence remains [ADR0013](0013-install-restart-click.md): setup → visible tray → unsigned install-once XPI → Firefox restart → ordinary supported download click → one automatic native task and independently correct output. Temporary loading, manual Add or mocks cannot substitute for this sequence. Browser compatibility limitations must be reported as limitations, not converted into a signing/account requirement or a passed test.
