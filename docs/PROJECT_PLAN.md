# Project Plan

**Target:** setup → visible tray companion → install the unsigned XPI once → restart Firefox → an ordinary supported download click produces one Manager task and independently correct output.

**Status:** M5 is not install-ready. The native companion and browser coordination have owned integration evidence. A separate automatic-capture candidate is now implemented; its ordinary browser workflow is not yet qualified. The default package remains manual-only.

## Delivery blocks

### 1. Automatic-capture candidate

Deliver one buildable candidate using the actual extension entry point and Manager UI, not diagnostic arming or manual Add as a substitute for capture.

- Explicitly reviewed HTTP(S), webRequest and blocking authority; passive top-frame trusted-click observation.
- Default-On preference only becomes effective after verified website/API permissions and a compatible connected companion. Off and permission revocation prevent new cancellation authorization.
- Ordinary Firefox permission UI, permission readback, understandable fallback and recovery controls. No implicit cookie/Authorization collection.
- Same-tab/default-store/nonprivate anonymous attachment policy and bounded redirect binding remain authoritative; do not broaden eligibility merely to pass a provider test.
- Separate development artifact and manifest; no change to default release selection or immutable v0.1.0 assets.

**Current implementation:** [CAPTURE_CANDIDATE.md](CAPTURE_CANDIDATE.md). Permission/state/entry/build tests are not real browser acceptance.

### 2. Consolidated acceptance campaign

Use the candidate with a source-identified paired companion in owned state. Reuse the existing fixture, installer, browser and retirement components. Preserve original closed-app/registration preflights and exact retained owners.

| Acceptance area | Required result |
| --- | --- |
| Ordinary capture | Trusted click, correct cancellation correlation, exactly one native task and correct output; no diagnostic arming or manual Add substitution |
| Preference and permissions | Default On after approval, actual Off/On, persisted Off, permission denial/revocation/regrant; unsupported or unavailable capture stays in Firefox |
| Request boundaries | Untouched navigation; correct POST/blob/frame/private/container/new-tab handling; conservative same-URL/race/session behavior |
| Real provider | Original public Hugging Face GGUF link from #49, distinct-host/TLS redirect and attachment, independently checked output; no hardcoded expiring URL or logged opaque validators |
| Recovery | Existing missing-terminal, lost-reply, unlinked and Aborted controls retain identity and explicit outcomes; no blind replay or unsafe history deletion |
| Installed lifecycle | Visible companion, reconnect/restart behavior, shortcut/cold-start/Quit/uninstall, joined cleanup and preserved output |
| Persistent XPI | Exact unsigned artifact active after ordinary installation and restart without a loading API, profile injection or protection change |

**Persistence limitation:** the owned default-profile observation refused the older unsigned XPI before approval. It does not determine normal-profile compatibility or create a signing/settings-change requirement. Exact persistence remains unverified, with no demonstrated permitted resolution yet. Do not repeat the unchanged default-profile attempt, count temporary loading as persistence, or label a partial campaign install-ready. See [FIREFOX_PERSISTENCE.md](FIREFOX_PERSISTENCE.md).

A failed or ambiguous case stops its execution domain; owners must be retired/joined before analysis or another run. Record source identities and independently verify results. Failed/cancelled CI, older artifacts and component evidence never substitute for an acceptance result.

### 3. Final package and release

After the campaign's applicable gates pass:

1. Resolve remaining compatibility/history limits and reconcile PR55's separate source explicitly.
2. Select the paired setup/application/capture XPI together, with matching versions, manifests, hashes and notices.
3. Review and integrate accepted source; run authoritative final-main build/CI and exact-artifact lifecycle/workflow checks.
4. Publish new artifacts and short installation instructions. Preserve v0.1.0 tags/assets and identify environment limitations accurately.

There is no signing account, signing submission, public marketplace or remote-updater stage. Release approval requires completed evidence, not a commit count or estimated percentage.

## Evidence baseline and open limits

| Area | Established evidence | Important limit |
| --- | --- | --- |
| Download engine and v0.1.0 | Historical scoped local release and final-main qualification | Manual-only; not the M5 workflow |
| Installed companion | Setup/tray/bridge/output/reconnect/Quit/uninstall and owned fault cases | Source-specific; normal shortcut/physical/cold-start/final artifacts remain separate |
| Native transaction | Immutable Prepared/Committed/Aborted identity, durable commit-before-dispatch, lost-reply/status recovery | No arbitrary machine/storage failure exactly-once claim; safe history/expiry remains open |
| Browser integration | Clean ff5e960 harness with clean paired2df904b passed seven owned loopback scenarios;15 output files independently checked | Temporary diagnostic XPI, not the new automatic candidate or persistence/public-provider qualification |
| Private/container | Actual container classification and private capability denial; correct Firefox-only output | Zero container identity is not default-cookie-store eligibility; no private/session replay authority |
| Unsigned installation | Clean bc36eb5 / packaged2df904b observed signature-required refusal in owned defaults | No installed receipt, approval UI or persistent restart result |
| CI | CI34594905167 at555609b passed all four jobs | Earlier intermittent adapter/bridge failures remain unexplained; no timeout/concurrency workaround established |

Detailed evidence belongs in the relevant reference, not repeated in this plan:

- [INSTALLED_BROWSER_SLICE.md](INSTALLED_BROWSER_SLICE.md): exact browser/native source identities, observations and hashes.
- [INSTALLED_COMPANION_SLICE.md](INSTALLED_COMPANION_SLICE.md), [SHORTCUT_OWNERSHIP.md](SHORTCUT_OWNERSHIP.md): installed lifecycle and ownership.
- [BROWSER_HANDOFF.md](BROWSER_HANDOFF.md), [NATIVE_HANDOFF.md](NATIVE_HANDOFF.md), [LOCAL_IPC.md](LOCAL_IPC.md): coordination contracts and limitations.
- [PRIVATE_RUNTIME_RECORD.md](PRIVATE_RUNTIME_RECORD.md): native resource/adapter evidence and distinct historical failure phases.
- [GLOSSARY.md](GLOSSARY.md), [decisions](decisions/README.md): terminology and architecture rationale.

## Scope and dependencies

[M5](https://github.com/HalcyonXP/firefox-download-manager/milestone/6) tracks #48–#53. [Project board](https://github.com/users/HalcyonXP/projects/1): #49/#50 and draft PR55/56 are In Review; #51–#53 remain acceptance-blocked in Backlog. Candidate implementation does not close those gates.

```text
#48 → #49 browser/persistence + #50 companion/setup
#49 + #50 → #51 accepted automatic capture
#49 + #50 + #51 → #52 accepted unsigned paired package
merged accepted source + authoritative main CI → #53 final-artifact qualification/publication
```

PR55 is a separate engine-diagnostic branch, not implicitly included in PR56. Reconcile source/document overlaps before integration; older signing prerequisites are superseded by [ADR0016](decisions/0016-unsigned-personal-xpi.md).

The available-machine qualification boundaries in ADR0011/0012 and owned-state protections in ADR0014 remain. No normal-profile testing, protection downgrade, elevation, unowned process termination, silent logon startup, VPN/routing integration, telemetry, cloud sync, torrents or media extraction is introduced. Treat persisted/input state as untrusted; preserve range/resource identity, correct output coverage, exclusive final promotion and sanitized logs.

Historical issue mapping and release scope remain in [ISSUE_MIGRATION.md](ISSUE_MIGRATION.md), the linked issues and version-specific evidence; they do not redefine current acceptance.
