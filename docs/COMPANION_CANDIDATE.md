# Paired companion development candidate

This is maintainer integration material, **not an install-ready release or the M5 quick start**. It does not replace the immutable manual-only v0.1.0 package.

The opt-in builder selects the companion application at the existing `download-manager-native-host.exe` leaf and pairs it with application-mode setup. Double-clicking that setup opens install/upgrade, Open Manager, repair, recovery, retired-generation cleanup and uninstall controls. Installation requests a visible companion launch after releasing setup coordination. A launch request is not a tray/readiness receipt; the companion independently verifies its current image and acquires the engine lock only after its visible-shell gate.

The package probe uses the explicit `--package-probe` entry and a bounded closed compatibility response. It starts no engine and depends on no installed registration. Its entry-family/wire/package versions do not prove signatures, live tray visibility, command commitment or Firefox installation. Legacy setup still uses its isolated native hello probe; paired setup deliberately refuses incompatible legacy repair targets instead of falling back across modes.

Use only independently owned installation/test domains and documented closed-app/registration preflights. Do not install over normal state merely to test. Setup never opens, stops, inspects or changes Firefox profiles. Existing journals and unknown entries retain their conservative refusal/recovery rules. A launched companion intentionally outlives setup; closing setup is not companion shutdown.

An ordinary launch shortcut and its receipt/journal migration, independently observed installed tray/bridge lifetime, unsigned persistent XPI, restart and ordinary-click capture remain unqualified. Do not substitute temporary extension loading, a right-click action or a successful metadata probe for those gates. No signed XPI or signing/account workflow is required. Exact unsigned-XPI persistence remains unverified; package metadata is not browser acceptance.

Maintainer build selection is `scripts/build-package.ps1 -Development -Companion -Output artifacts/<new-owned-directory>`. This opt-in recipe refuses production selection, preserves the eight payload roles, includes the companion's actual dependency closure/notices, and requires reviewed stock-system imports/runtime objects even for this development candidate. It does not publish or install anything.

The paired installer now creates a per-installation Start Menu shortcut with explicit receipt2/journal2 ownership. See [SHORTCUT_OWNERSHIP.md](SHORTCUT_OWNERSHIP.md) before maintainer migration/recovery testing. The default legacy setup cannot operate these records, and the legacy qualification driver is not a receipt2/browser qualification path. Installed Start Menu/GUI-to-tray evidence is still required.
