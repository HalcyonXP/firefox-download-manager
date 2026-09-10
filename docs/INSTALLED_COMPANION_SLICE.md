# Installed companion/native slice

## Scope and observations

An owned Windows 11 development-candidate test exercised the real setup window's Install button, real current-user native registration, receipt2 and generated shortcut, visible installed companion, actual native stdio/IPC bridge, and the setup Uninstall button. It did not use a mock registration store.

Observed sequence:

1. Firefox/application preflight passed; this host's keys were absent in both registry views under HKCU and HKLM. A fresh owned local/profile domain was created; no normal browser profile was read or changed.
2. Setup's Install control produced a matching receipt2, manifest, helper/XPI hashes and immutable/external shortcut bytes. The independently resolved Programs folder was inside the isolated profile domain, not the normal Start Menu.
3. Setup retained its launched child and exposed that process identifier through its lifetime observation label. The driver used that observation to identify the companion window; it did not acquire cleanup authority through process-name/tree discovery. A running-engine status and actual tray rectangle were observed.
4. A separately retained native bridge process completed one 8 MiB loopback download. Independently computed output SHA-256 matched the deterministic fixture, a Completed event arrived, and reconnect returned exactly one completed task. Native EOF/reconnect did not replace the companion process. This is not independent crash-recovery proof.
5. The existing companion Quit control stopped it. Setup observed exit and joined its retained child; the tray and runtime publication disappeared. The fixture server and native bridge/readers were retired and joined.
6. Setup's Uninstall control removed matching native registration, generation files and shortcut group while preserving the output. Setup then closed and its retained process was joined; registry absence and closed-application checks passed again. Owned fixture files were retained as evidence.

All control actions were synthetic native control messages, not physical input. No Firefox process was launched, no XPI was installed, and no ordinary browser download click or normal Start Menu launch was tested. The package was a dirty-worktree development candidate based on3e94b60 with setup lifetime-observation changes; neither3e94b60 nor later source inherits exact-head installed qualification from this observation. Package/source/report identities remain distinct.

## Retained failures and ownership observations

The first attempt failed before installation: the child environment resolved Programs beneath the isolated USERPROFILE, but that directory had not been prepared. The setup coordination lock existed; the install root and native registration did not. A metadata-only check confirmed that Programs resolved inside the owned domain and did not exist. The failed fixture and its setup window were preserved; the original driver had released its process handle and could not subsequently infer forced-retirement authority from a PID. This is a cleanup deficiency of that failed driver, not a successful containment result.

A second fixture guard incorrectly required Programs beneath the APPDATA spelling; it refused before launching setup. The subsequent fixture independently resolved Programs beneath the owned domain, created that fixture directory, and passed the sequence above. Production's missing/unsafe-directory refusal was not weakened. Later passes do not retroactively supply cleanup or success for either failed attempt.

Setup now exposes retained launch/exit observations separately from its operation status. The child is retained before publishing its identifier; failed waits retain ownership, and a joined-exit observation is emitted only when no launched child remains. These observations do not make setup's launch request a tray-readiness receipt or make a PID standalone ownership authority.

## Operation observations and driver cleanup

Setup label312 starts at `Operation 0: idle`. Each accepted action advances a per-window counter before configuration or worker startup and reports `running`; `complete` is published after configuration refusal or after joining/accepting the worker result and retaining any launched child. Completion is not operation success, tray readiness, durable receipt2 or a cross-process transaction identifier. Ignored requests do not advance the counter.

The diagnostic `SetupOwner` records dispatch uncertainty before sending a control. It never replays a failed delivery. Initial no-child text does not resolve a pending request; cleanup requires the expected completed operation plus no retained Manager (or a joined-child observation). Normal Manager Quit must precede setup Close. Failed waits retain the exact process object; a late exit can be joined without another Close request. Unknown observations preserve the domain and refuse success rather than killing a discovered process. These callbacks require the same retained setup instance and a controlled fixture, not arbitrary concurrent GUI actions.

`DomainPlan` writes a local plan before exclusive domain creation. A plan is not a creation witness and cannot adopt a collision. The no-install window driver now uses these helpers, records status/lifetime before window destruction, and attempts retirement even if failure-record writing fails. Its normal read-only refusal and injected failures before window discovery/after refusal/while writing failure diagnostics were observed with exact retained setup waits and no forced termination; failed cases produced no success report. These checks neither exercise registration nor qualify installed failure recovery. The installed diagnostic driver still needs this integration and review of all partial bridge/server/installation paths.

## Remaining gates

Persistent signed XPI, Firefox restart/native-parent lifetime, supported ordinary-click capture, normal Start Menu/physical tray interaction and exact-final-main package qualification remain open. The diagnostic driver still needs reviewed failure/recovery handling before becoming a general qualification entry point; existing legacy drivers must not be relabeled as receipt2/Firefox qualification. The failed setup window is outside the successful run's cleanup result.

References: [SHORTCUT_OWNERSHIP.md](SHORTCUT_OWNERSHIP.md), [COMPANION_DESIGN.md](COMPANION_DESIGN.md), [USER_WORKFLOW.md](USER_WORKFLOW.md).
