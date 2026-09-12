# Firefox ordinary-click API probe — #49 integration evidence

> **Automation-policy correction:** the earlier shared driver used an obsolete opt-out and allowed Firefox automation preference overrides. Recorded behavior/output remains source-scoped, but does not establish unchanged default protection policy. See [FIREFOX_TEST_POLICY.md](FIREFOX_TEST_POLICY.md).

Status: **diagnostic API evidence, not a production interceptor or M5 qualification**. This probe is maintained alongside #50 integration; #49/PR55's earlier engine-library diagnostic is a separate source branch and evidence layer. Distribution scope remains [ADR0016](decisions/0016-unsigned-personal-xpi.md): persistent unsigned personal XPI, no signing/account workflow and unchanged Firefox settings/protections.

## Isolation and entry points

`scripts/probe-firefox-capture.py` composes the existing owned-profile Firefox transport, closed-app preflight, exclusive `DomainPlan`, retained HTTP fixture and bounded report sink. It never installs or removes native registration. It requires registration absence before and after execution and retains the existing driver's closed-Firefox/helper checks; no concurrent-normal-browser exception was added.

The exclusively created domain contains separate browser/local/roaming/profile/download directories. Before any fixture request, only the owned browser's download-location preferences are set and read back to confine even unexpected Firefox fallback output. Normal profiles, existing downloads and signing/TLS/Safe Browsing/update/proxy/sandbox settings are not modified. The retained browser is closed/joined and fixture workers joined before any success report. Failure preserves the domain; unresolved cleanup retains the known actors instead of terminating discovered processes. This is not injected browser-startup/cleanup-failure qualification.

The diagnostic XPI has a fresh `example.invalid` identity, never Manager's identity. Its only permissions are `webRequest`, `webRequestBlocking` and the loopback host pattern; no `nativeMessaging`, cookies, downloads, all-sites or webNavigation permission is requested. A passive top-frame content listener observes trusted link clicks on the fixture page. No page body is scraped. A separate inspector tab retrieves bounded observations without navigating or cancelling the request under test. Temporary installation is explicit and supplies **no persistent-install evidence**.

The probe records only fixed route/context/error classifications and local request numbers, not URLs, queries, cookies, authorization values or raw browser errors. Raw matching URLs remain bounded-count, ephemeral diagnostic state. The XPI is generated only inside the owned domain and is excluded from the product extension build/package.

Maintainer invocation, only with freshly satisfied preflights:

```powershell
python scripts/probe-firefox-capture.py --firefox "C:/Program Files/Firefox Developer Edition/firefox.exe" --report artifacts/<new-report>.json --execute-owned-browser
```

This opens an owned Firefox window and deliberately cancels local fixture requests. It is not an installation instruction or a Manager task handoff.

## Observed browser behavior

A dirty development harness based on f7a6da6 exercised the installed Firefox Developer Edition binary in fresh isolated state. Source digests, diagnostic-XPI hash and binary hash identify the report inputs; they do not identify a released Manager package.

| Case | Observation |
| --- | --- |
| Trusted direct attachment click | GET/main_frame/frame0, `incognito=false`, `cookieStoreId=firefox-default`, `originUrl` matched the click page; asynchronous headers response requested cancellation; the same request ended with `NS_ERROR_ABORT`; no browser output |
| Trusted redirect to extensionless attachment | The original query/order survived the fixture redirect; the same request ID spanned the302 and final200 attachment; click correlation survived; asynchronous cancellation ended with `NS_ERROR_ABORT`; no browser output |
| Ordinary navigation | GET/main_frame, no attachment; not cancelled, Completed observed |
| Form POST | POST/main_frame, HTML response; not cancelled, Completed observed |
| Iframe document | GET/sub_frame, nonzero frame; not cancelled, Completed observed |
| Cancellation-disabled direct control | Same attachment allowed; Completed observed and Firefox produced exactly one independently byte-checked1408-byte file in the isolated download directory |

Here “Completed” means the webRequest terminal event, **not** a Manager task's Completed event. Firefox output was checked independently of that network event. The control output SHA-256 was `44dfb88b20921b26d3237d4adb94cc2df1d63152a27236be3f72630e8b8ee332`.

The strengthened six-case report is `artifacts/capture49-api-joined-exit.json` (ignored/local), with harness SHA-256 `3cf5259b842dca600796bdb17a6ebda36c4d11e7ec560502756d1dbb4a1e36c2`, background source `55f7ecf87da22f8dda12002e4ad36b005ff049c73e41db41deb74e347d3896b2`, click source `a68fa4743d817e1625ab75b70f1f8a92371050ac87d9725c0254247189ed629b`, and diagnostic XPI `345c2969217465b558bfb19e1a40b714f2869efe11f8224ff90d1a8d8a27c7ef`. Browser/fixture retirement completed, exact browser exit0 was required and observed, and final registration absence passed. These observations do not transfer to later source merely because it describes them.

## Retained findings and regression scope

The first five-case report required a correlated error plus no output, but its `NS_BINDING_ABORTED` comparison was false and it did not independently require a specific cancellation error. It is narrower evidence, not the controlled six-case result. The subsequent harness allowlists browser error classifications and adds the uncancelled, independently checked download control. That run observed `NS_ERROR_ABORT` for both cancelled cases. It did not independently require successful browser exit. The final strengthened run repeated all six cases with exact browser exit0 explicitly required; a failed-but-joined exit regression is rejected. The earlier controlled report retains its narrower evidence scope.

Review also identified potential reuse of one click by another same-URL request. The diagnostic now consumes a click for one request ID while retaining that request's redirect chain. Modeled tests cover promise settlement, consumption, redirect identity, missing/private/container/frame/method context, untrusted clicks, unarmed/overflow refusal and bounded redaction. These are diagnostic guards, not proof against real competing same-URL races. An omitted click-consumption mutation initially survived because the test read `.cancel` from a pending Promise as though it were an immediate response. The test now rejects a thenable or queued cancellation timer on the second request. Restored tests pass; omitted consumption, omitted terminal-error validation and bypassed unarmed refusal mutations are rejected. The initial Python HTTP policy test incorrectly used HTTPConnection as a context manager; it failed before a request, was corrected to explicit closing and passed. No product behavior was changed for that test failure.

Python policy/real-loopback tests cover manifest identity/permission boundaries, exact redirect query and bounded body, correlated terminal evidence, and inspector-tab isolation. They never launch Firefox. CI runs these and the JavaScript diagnostic tests; real browser execution remains opt-in.

## Remaining proof and implementation

- No native prepare/accept/cancel/commit exchange exists here. Diagnostic cancellation intentionally creates **no Manager task**; it must never be enabled as production capture. A native failure after browser cancellation would lose the download without a separately designed recovery contract.
- Same-origin local redirects only. Cross-origin attachment redirects, the public GGUF click, authentication/session eligibility, private/container execution, downloads attributed to new tabs, double-click/racing same-URL requests and restart/lost-acknowledgement behavior remain open. Model refusals do not qualify real private/container behavior.
- The trusted-click/message/request ordering worked in these observations; it is not a general ordering guarantee or an approved correlation protocol. Missing/ambiguous correlation must leave Firefox untouched.
- No Manager extension permission migration, persistent unsigned installation/restart, real Firefox-to-installed-companion task, normal Start Menu/physical tray interaction or exact-final-main qualification is established.

The next handoff contract must bind a single observed click/request to an idempotent native reservation, prohibit downloading before an explicit commit, and define bounded abort/expiry and uncertain-commit recovery. This is a design requirement to resolve before #51 selects production interception, not a protocol already implemented by this probe.
