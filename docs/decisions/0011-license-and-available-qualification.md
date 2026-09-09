# ADR 0011: Permissive first-party licensing and available-machine qualification

- Date: 2026-09-09
- Status: accepted implementation of the owner's clarification for #28

## Confirmed intent versus implementation decisions

The owner clarified: **"People can do what they want this is FOSS"** and that this is their **only computer**, so they cannot provide a separate clean Windows 11 x64 environment.

We interpret the FOSS direction as a permissive grant rather than a requirement for copyleft. **MIT is our concrete license selection** implementing that direction; the owner did not name a particular SPDX identifier. First-party source is covered by the repository's [LICENSE](../../LICENSE), permitting use, modification, redistribution, sublicensing and commercial use, subject to retaining its copyright/permission notice. This is not a public-domain dedication or a claim that third-party notices can be removed.

The available-machine constraint changes the first local release's qualification plan. We will qualify the exact package on the owner's existing **native Windows 11 x64 / Firefox Developer Edition** installation with fresh, isolated test profiles/application state and owned setup generations. A separately provisioned, developer-tools-free OS is **not a release prerequisite for this personal release**. This is an explicit plan change, not a clean-machine test being marked passed.

## Consequences

- `LICENSE`, npm metadata and all first-party Cargo packages identify MIT. Registry-publication guards (`private: true`, `publish = false`) remain; those prevent accidental uploads and do not make the source proprietary.
- Packages include `LICENSE.txt` and the existing full `THIRD-PARTY-NOTICES.txt`. The XPI and standalone extension build include the MIT license and reviewed esbuild notice. Third-party terms remain their own; MIT does not relicense dependencies or override their obligations.
- The unreleased, closed package-v1 payload allowlist grows from seven to eight leaves. Older unqualified candidates are not mixed with the new setup/package. Wire v2, task state v4, settings v2, installation receipts and the paired application version remain unchanged. License-bearing artifacts must be rebuilt/retested; earlier artifact reports are not relabeled.
- The #28 issue and project milestone replace the clean-machine criterion with native local installation/use/upgrade/removal in an isolated application domain. Existing development tools remain installed. System32-only child PATH, PE import review and isolated state provide specific evidence, not proof of a factory-clean OS or all Windows configurations.
- Release notes must identify the actually tested OS/browser/architecture and explicitly state that a clean-machine test was unavailable. Windows Server CI and Windows 11 ARM64 x64 emulation remain additional, separately labeled evidence—not replacements for native x64 Firefox testing.
- No second computer, new OS/account, Windows feature, elevation, billing/protection change or live-profile modification will be requested or introduced to manufacture evidence. Closed-process/registration preflights and conservative ownership cleanup remain mandatory. An unowned browser blocks a mutating test until the owner closes it normally.
- The existing unsigned temporary-XPI workflow remains valid without changing signing preferences. A FOSS license is not a publisher signature, security approval, or release qualification.

## Preserved history and remaining work

Public visibility in #8 did **not** select a license; that historical statement remains true. This later clarification resolves the licensing decision. The earlier clean-machine requirement is superseded, not satisfied. The reviewed LLVM/MinGW/UCRT distribution recipe and prior MSVC decision are not reversed.

All other correctness/security, actual Firefox controls/restart, adversarial HTTP, resource measurement, privacy and exact-artifact publication requirements remain. #28 / PR #43 stay In Progress/draft until those gates pass. See [QUALIFICATION_PLAN.md](../QUALIFICATION_PLAN.md), [FIREFOX_QUALIFICATION.md](../FIREFOX_QUALIFICATION.md) and [PROJECT_PLAN.md](../PROJECT_PLAN.md).

## Later changes

Do not present a later licensing change as withdrawing rights already granted for distributed copies. Additional environments can extend future qualification when available; they cannot retroactively turn an unavailable test into passed evidence. Further scope changes must update the issue, plan and release notes explicitly.
