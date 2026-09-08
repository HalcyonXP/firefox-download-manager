# Packaging implementation plan — #27

Baseline: security-reviewed `0a81007`; #26 findings remain binding. This document records implementation intent, not completed installer/release evidence.

## Decisions

- Build a separate **Rust setup executable**, not an execution-policy-bypassing PowerShell installer. Existing development scripts will become guarded wrappers or be superseded; they are not a safe fallback. Registry access uses deliberately reviewed `winreg` 0.56.0 (MIT), with optional features disabled. The metadata and full MIT notice were reviewed; deliberate incorporation added only `winreg` to the lockfile, reusing existing Windows bindings.
- Current-user installation only; no service, elevation request, firewall/route/VPN modification, telemetry, remote update, or browser-profile editing.
- Installation roots must be confined to the user's application-data area, disjoint from task state, ordinary/non-reparse, fully drive-qualified and bounded. Resolve and validate ancestor identity; retain directory ownership through mutation where practical. Reject UNC/device/traversal/ambiguous components, foreign registration, and unowned name collisions rather than guessing ownership.
- Store an explicit versioned **installation receipt** for only fixed generated files. It is local ownership/recovery metadata, not a signature or protection against a compromised account. Hashes must match before replacing/removing owned content. Never sweep arbitrary files or task/download state.
- Use verified create-new staging, a bounded transaction journal and known backups. Register only after complete files and a fresh-state helper launch/hello check succeed. Recover/roll back only proven owned content; preserve unknown or inconsistent entries for explicit inspection. Do not blindly overwrite registry changes made by another actor.
- Upgrade preserves state bytes; the helper, not setup, performs the already-tested v1/v2/v3-to-v4 migrations on its next normal start. Test same-version upgrade and legacy-state fixtures honestly: there is no earlier qualified product release to claim was upgraded.
- Uninstall removes only the matching current-user registration and proven generated files. Unknown files remain. Keep task state and completed downloads. Do not kill running Firefox/helper processes or weaken execution policy/security settings to force replacement.

## Artifacts and provenance

A versioned Windows-x64 candidate package should contain the native helper, setup executable, unsigned XPI, installation/troubleshooting instructions, third-party notices, and bounded package/provenance metadata. The paired package remains 0.1.0 / wire v2 / state v4 / settings v2 unless explicitly changed. Firefox minimum 156 and the security-reviewed permissions/CSP remain fixed.

Use pinned source/toolchain/dependencies; prefer static MSVC CRT linkage to avoid a hidden developer-machine runtime prerequisite. Inspect PE dependencies, remap build paths, and use reproducible archive timestamps/order and linker settings where practical. Record actual tool versions and source commit; compare repeated builds before claiming bit reproducibility. GitHub Actions produces candidate artifacts plus SHA-256 checksums; a CI artifact is not a qualified GitHub release.

Unsigned XPI or temporary/unpacked Developer Edition use is explicitly allowed by #27. Document restart/reload and signing prerequisites without modifying the user's live profile. Public visibility did not select a first-party license; generated third-party attribution does not make that choice.

## Required tests and outstanding environment constraints

- Paths containing spaces; fully qualified/root/ancestor/reparse checks; changed/missing/unknown owned files; foreign registry entries; active-file sharing; concurrent setup; interrupted/faulted staging, launch, registration and rollback; no-op/diagnostic behavior.
- Install/upgrade/uninstall preserve state and completed downloads byte-for-byte. Use only self-owned temporary roots and registration, with cleanup even on launch failure. Refuse existing unowned Firefox/registration rather than interfering with them.
- Validate the actual package, extension ID/capabilities, helper hello and state migrations, notices/checksums, and intended stock-shell/runtime prerequisites.
- Local reconnaissance found Windows PowerShell available and no Windows Sandbox executable. No optional Windows feature, account, protection, billing, or live-profile setting was changed. A stripped developer PATH/fresh application root is useful evidence, **not** a factory-clean Windows 11 machine.
- #28 still owns final-artifact Firefox controls/restart/installation/upgrade/removal, clean-machine evidence, multi-gigabyte CPU/memory/disk/event measurements, and the actual versioned release. Do not substitute mocked transport, CI's Windows server, or the earlier small Firefox authentication slice for those gates.

## Initial boundary checkpoint

The working tree now has a setup **library only**, not an installable setup executable. It provides a bounded, fixed-leaf/duplicate-rejecting package descriptor, streamed hashes, canonical ordinary-directory leases, application-data/state confinement, and a fixed-HKCU registry adapter. Registry operations still require the not-yet-built cooperative setup lock and transaction coordinator. No actual HKCU operation or helper/browser launch has been performed by these new tests.

Filesystem tests exposed two harness assumptions: Windows TEMP used an 8.3 spelling, so comparisons now use the intended canonical path; symbolic-link creation lacked privilege, so a real junction fixture was used without enabling Developer Mode/elevation or skipping the boundary. A generated test-shell path initially contained an escaped vertical tab and was corrected. The initial seven tests cover metadata grammar/duplicates, UTF-16 registry grammar, spaces/state overlap, ordinary ancestor rename protection, junction rejection and corruption/the descriptor size limit. They do not test installer lifecycle.

### Proposed transaction layout to implement next

Prefer immutable generation directories under the installation root, with only the bounded receipt/journal changing and the native registration pointing to one complete verified generation. This avoids overwriting a running helper or needing an allegedly atomic multi-file replacement. Retain proven generations for rollback; remove only known, verified inactive content. Bound generation history and root length. A new generation's XPI must be explicitly loaded/reloaded by the owner; setup does not install it into a live profile. Finalize this design with fault/restart tests rather than treating this proposal as implemented.

Closed receipt/journal models now add two more tests (nine total), with canonical RFC UUIDs, unique/current generation coverage, same-install transition ownership, and required nullable predecessor/successor keys. Full local npm check, Rust formatting/clippy/workspace all-feature tests/build and dependency policy passed at this boundary checkpoint. No #27 remote CI, setup executable, transaction coordinator, global setup lock, actual registry lifecycle, launch probe, package builder or candidate artifact is implied. Installation roots are currently capped at 160 UTF-16 units (directory leases 240) to leave room for generation/manifest names; that bound still needs lifecycle qualification.
