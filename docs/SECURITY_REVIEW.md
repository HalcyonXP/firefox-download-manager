# Pre-packaging security review — #26

Date: 2026-09-08. Reviewed baseline: `24b638e` plus the #26 changes. Scope: Windows 11, Firefox Developer Edition 156, paired wire v2/helper state v4. This is a source/regression review by the implementer, not independent penetration testing or release approval.

## Goal, authority, and decisions

The confirmed project goal is an installable local manager without VPN integration, telemetry, automatic download interception, or modification of the live browser profile. The implementation decisions below narrow its initial authority; they are not new user-confirmed preferences. #25 is merged and public CI `34248601130` passed; merged-main `34249484925` also passed. The merge command's HTTP 502 was followed by authoritative verification of the successful merge, not an assumption or blind retry.

### Permission inventory

| Permission / surface | Necessity and confinement |
| --- | --- |
| `nativeMessaging` | The only file-transfer/control transport; one fixed host name and extension principal. Each helper message is independently validated. |
| `menus` | Explicit **Download with Manager** action on HTTP(S) links. No blanket Firefox-download interception or cancellation. |
| Optional `cookies` | Only checked per-Add session handoff. `collectSession` first proves permission and a normal default-store source; disabled handoff makes no cookie/API calls. |
| Optional `http://*/*`, `https://*/*` declarations | Eligibility to ask for the selected canonical scheme/host, not pre-granted all-site access. No wildcard host can reach the permission API. Firefox grants cover all ports; helper requests remain confined to exact scheme/host/port. |
| Tabs/action APIs without `tabs` permission | Create the extension page, retain a source tab ID, and inspect required non-URL tab context after opt-in. No browsing-history or page-content permission is requested. |
| Explicit CSP / private-window policy | Packaged scripts/styles only; no external fetch/form/object/frame surface. `incognito: not_allowed` prevents private-window task-history surprises. No content scripts, web-accessible pages, external extension connections, or manifest updater. |

`extension-policy.mjs` and its hostile-manifest tests gate this policy in `npm run check`. Firefox minimum 156 replaces the unqualified 128 API claim; the esbuild syntax target is not API qualification. Only the earlier 156 authentication slice was actually exercised in Firefox. The final policy/artifacts still need #28 browser qualification. Firefox's own browser/network/update behavior is not disabled or represented as manager behavior.

## Findings and disposition

| ID | Finding | Disposition |
| --- | --- | --- |
| S26-1 | A literal, encoded, or IDNA-normalized `*` in a URL hostname became wildcard Firefox site authority in `sessionPermission`. | Fixed before permission request/contains/cookie access; regression covers wildcard subdomains and all-host input. This proves potential overbroad permission requests, not observed credential disclosure. Previously granted permissions are not automatically retracted: use **Revoke optional permissions** in prerelease installations; cancel submitted session tasks separately. |
| S26-2 | Opaque remote ETags were Debug-renderable and outbound `If-Range` was not marked sensitive. Remote metadata can itself contain sensitive/reflected data. | Redact `EntityTag` Debug and mark `If-Range` sensitive while preserving exact header/identity bytes. Test asserts both behavior and redaction. No current raw HTTP tracing/logger is configured. |
| S26-3 | Implicit CSP/private-window defaults and minimum 128 advertised more than the qualified API evidence. | Explicit manifest restrictions and minimum 156, with recurrence guards. Effective final-Firefox behavior remains #28, not a static-test claim. |
| S26-4 | Development install/uninstall scripts rely on path/name assumptions, can replace a different registration, and lack release-grade reparse/ownership/transactional upgrade defenses. | **Release blocker assigned to #27.** Require confined local roots/files, foreign-entry refusal, verified create-new staging, rollback, and ownership-safe failure/uninstall tests. Review completion does not qualify these scripts. |
| S26-5 | Existing security prose still said “no host origins” and treated bounded validator text as ordinary-log-safe. | Corrected to optional selected-site grants and no raw validator logs; protected state is explicitly not secret-free. |

## Trust-boundary evidence

- **Web content → extension:** `creation.ts`, `background.ts`, `session.ts`, and `dashboard.ts`. HTTP(S)/user-info/Windows-component checks, bounded single-use capture (32 entries, 60-second lifetime), exact extension-page sender checks, text-only DOM projection, no request/Authorization harvesting. Cookie source, Secure/expiry/path/domain/partition exclusions and permission loss have regression tests.
- **Extension → helper:** protocol framing/strict JSON/v2 tests reject oversized/truncated/duplicate/unknown inputs. Frames cap at 1 MiB before allocation; URLs at 16 KiB; commands and settings accept a closed shape. Correlation and snapshot sequencing never replay uncertain Adds. Native stdout is framed data; errors/stderr are classified rather than raw input/error chains.
- **Network → storage:** `authentication.rs`, `http_probe.rs`, `scheduler.rs`, and `admission.rs` cover no-contact context-bearing cross-origin redirects, exact ranged status/coverage/size/identity, header injection, fallback, bounded shared admission and cooldown. TLS verification and normal OS routing remain enabled. The helper intentionally permits explicitly selected loopback/LAN HTTP(S); it is not an SSRF sandbox or access-control bypass.
- **Storage/recovery/settings:** storage/persistence/task/native tests cover traversal/device/ADS/reparse entries, collisions, corrupted state, immutable checksums, killed-helper restart, and explicit cleanup. Byte ownership is joined before stop acknowledgement. Final hard-link publication is no-overwrite and preceded by validation. No hardware power-loss, hostile same-user namespace-race immunity, or hard-real-time cancellation is claimed.

## Actual log/state inspection

`crates/native-host/tests/privacy.rs` starts the compiled helper with isolated application roots and no native registration or Firefox profile. It negotiates SHA-256, enables verbose diagnostics, submits a rejected insecure Authorization input, then sends a synthetic authenticated/signed-query download whose checksum deliberately mismatches. It inspects framed stdout, stderr, actual settings/task files, and every generated diagnostic line:

- no supplied cookie, Authorization value, or referrer path in output/state/logs;
- no signed query in ordinary logs or snapshots;
- exact signed query deliberately present in protected recovery metadata;
- required v4 session/checksum fields retained and no new final output;
- logs contain only bounded timestamp/fixed-enum records; clean stderr is empty.

The first fixture attempt added a query to a referrer whose server contract requires an exact URL, correctly yielding `AUTH_EXPIRED`. The test was corrected to the existing exact-referrer contract rather than weakening server authentication. The final native-process regression passes.

“Memory-only session” describes supplied context ownership, not secure erasure or a guarantee that remote content/metadata cannot reflect sensitive data. Exact URLs and remote validators are protected recovery data; downloaded content can also be sensitive. Neither this inspection nor publication history guards justify uploading arbitrary state, profiles, diagnostics, crash dumps, or audit backups.

## Dependency/supply-chain audit

- `npm audit` reported **zero** info/low/moderate/high/critical advisories in the locked tree at review. CI now repeats `npm audit --audit-level=high`; all findings still require assessment.
- `cargo deny check` covers the locked all-target graph: advisories, sources, licenses and bans. It passed locally, as did JS license inventory and the full formatting/lint/unit/integration/build gates; known duplicate-version warnings are not represented as zero-warning output.
- Direct dependency/license choices remain in [THIRD_PARTY.md](THIRD_PARTY.md). No new runtime dependency was introduced by #26. Generated notices/SBOM and packaged-binary provenance remain #27.
- Actions are commit-pinned, use read-only repository permissions and `pull_request` rather than privileged target execution; lockfiles are committed. This reduces supply-chain exposure, not proof against compromised toolchains/dependencies.
- No runtime telemetry/analytics/cloud-sync/VPN/remote-updater code was found in the inspected extension/helper. Development dependency downloads, GitHub automation, OS TLS trust/routing, and Firefox's own maintenance are distinct surfaces.

## Remaining gates and risk acceptance boundaries

1. **#27:** resolve S26-4, generate notices, verify package contents/provenance, and document unsigned/signing/install prerequisites. Public visibility did not choose a first-party license.
2. **#28:** actual final-artifact Firefox controls/restart/installation/upgrade/removal, clean application environment, larger files and performance/resource evidence, and versioned/checksummed artifacts. Leave an unowned running Firefox instance untouched; use a safe isolated test environment.
3. Local account/OS/helper replacement, malicious memory mapping/namespace mutation, DNS/OS trust compromise, and downloaded malware remain outside product isolation guarantees. Ordinary corrupt/untrusted state still fails closed.
4. Signed URLs/remote metadata persist where recovery needs them; per-user state and backups must remain private. Revocation cannot retract bytes/context already submitted to the helper.

#26 can complete as a reviewed and regression-tested implementation with these named packaging/qualification blockers. It must not be read as “ready to install.”

## #27 follow-up (packaging boundary passed; final #28 qualification pending)

The S26-4 row above records the inspected #26 baseline, not continuing approval of those scripts. They are now refusal-only retirement stubs. Rust setup implements confined ancestor leases, receipt ownership, bounded copy/hash/probe staging, cooperative domain locking, exact fixed-HKCU transitions, journal rollback/recovery and preservation-safe cleanup/uninstall. Local filesystem/fault tests and actual isolated helper probes are recorded in [PACKAGING_PLAN.md](PACKAGING_PLAN.md); public CI `34274073613` passed actual disposable-registry lifecycle on Windows Server x64 and Windows 11 ARM64 x64 emulation, followed by local native Windows 11 x64 probing of that CI candidate. The S26-4 packaging boundary is satisfied within the documented ordinary-account/fault model; final #28 qualification remains required before release approval.

The initial static MSVC release proposal was not adopted after reviewing its additional distribution terms. The pinned LLVM/MinGW/UCRT recipe, selected runtime-object review and generated nested/vendored notices are documented in [THIRD_PARTY.md](THIRD_PARTY.md). This changes the release compilation target, not the wire/state/security policy or first-party licensing. Full release-target Rust tests passed locally; final-artifact Firefox behavior is not inferred from them.
