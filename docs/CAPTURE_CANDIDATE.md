# Automatic-capture development candidate

**Implemented and buildable, not install-ready or browser-qualified.** This is a candidate product entry, not the loopback diagnostic. It has no inspector, arming command, seeded task or fault-injection controls. Default `npm run build` and paired-package selection remain manual-only; immutable v0.1.0 artifacts are unchanged.

## Build and identity

From the repository root, with existing Node/Python dependencies:

```powershell
python scripts/build-capture-candidate.py --output artifacts/<new-candidate-directory>
```

The parent must already exist. The builder exclusively creates a directory below `artifacts`, refuses alias parents/previous output, uses the existing bounded synchronous esbuild compiler, and retains/waits its exact parent on an interrupted wait rather than killing it and abandoning a child. Eight fixed XPI assets are size/hash checked, create-new archived and independently reread. `BUILD.json` identifies source commit/dirty state and payload hashes; `candidate.json` identifies the XPI hash and explicitly records `candidate:true`, `qualification:false`. Build failures preserve the output directory.

The XPI is `download-manager-capture-candidate.xpi`, version0.2.0, stable ID `download-manager@halcyonxp.local`. It is unsigned and separately named. This directory is not a paired installer package and is not accepted by the ordinary package or existing narrow manual-XPI persistence input policy. A compatible companion must advertise both `prepared_handoff` and `task_handoff_phase`. Existing paired2df904b remains a separately identified native integration input, not a newly built or final qualified package.

## Authority and behavior

`extension/candidate/manifest.json` explicitly selects:

- Existing nativeMessaging, menus and own storage, plus webRequest/webRequestBlocking.
- HTTP(S) host authority and a passive `click.js` content script, top-frame only, no about:blank injection.
- Existing optional cookies for explicit manual session operations only. Automatic capture never requests/collects cookies or Authorization context.
- Existing private-execution denial and restrictive extension-page CSP; no web-accessible resources, external messaging or remote update URL.

The candidate has its own strict manifest validator; additional authority does not become permissible in the manual manifest. Firefox's normal installation/website-permission controls remain authoritative. No permission prompt, preference override or installation action occurs during building.

`automatic-background.ts` selects the existing coordination/policy without diagnostic arming. New cancellation authorization requires all of: completed listener activation, loaded/verified On preference, verified host/API permissions, connected native transport and both handoff capabilities. Missing/failed permissions, unsupported requests or an unavailable/incompatible companion leave Firefox in control.

`CaptureAccess` observes permission changes and immediately revokes authorization before asynchronous readback. Revision checks prevent old permission reads from restoring stale grants. Partial listener registration stays failed; it cannot recover into authority without a revocation observer. This state is independent of the saved capture preference and never changes existing transfers or native history.

The Manager UI separates the On preference from website authority. **Allow / check website access** requests only the declared site/webRequest permissions from its button handler. It never requests on startup, never infers authority from the prompt's result and never automatically replays an uncertain request. A background readback follows approval, denial or failure. Off remains a separate immediate control for new capture authorization.

The underlying conservative policy is unchanged: prior trusted same-tab click, default-store nonprivate top-frame anonymous GET, bounded validated attachment/redirect chain and immutable terminal binding; observed cancellation precedes native commit. The cross-origin option is selected in the candidate but does not establish public-provider/TLS/session acceptance.

## Acceptance still required

The source/build/permission models and prior loopback diagnostic observations are different evidence layers. This candidate still needs one consolidated owned campaign covering normal startup/click, actual Firefox permission denial/removal/regrant, Off/restart, unsupported requests and races, public-provider output and installed lifecycle. Browser-protection implications and exact-artifact compatibility remain release gates, not consequences of unchanged preference values alone.

Exact unsigned-XPI persistence remains unresolved as recorded in [FIREFOX_PERSISTENCE.md](FIREFOX_PERSISTENCE.md). Do not rerun unchanged defaults, inspect normal profiles, change signature enforcement, introduce signing, or use a temporary load to claim persistence. [PROJECT_PLAN.md](PROJECT_PLAN.md) defines the delivery blocks and remaining acceptance.

Local verification: complete npm gates with249 TypeScript tests and14 protocol examples,119 Python tests including a real candidate build/ZIP readback and interrupted-wait model, strict candidate/manual manifest policy, and four targeted authority mutations pass. These are source/build models, not permission-prompt or live capture acceptance.
