# Owned normal-XPI installation observation

Status: an owned default-profile run observed a signature requirement; **persistent installation was not observed**. This is a separate, opt-in installation observation, not M5 install-ready or native-handoff acceptance.

## Scope and input

`python scripts/probe-xpi-persistence.py --package <paired-package> --firefox <Developer-Edition> --report <new-report> --execute-owned-browser`

The existing paired-package validator binds the exact XPI to its package descriptor/payload hashes. `xpi_policy.py` additionally bounds ZIP size/expansion/member counts, checks every CRC, rejects duplicate manifest members and accepts only the current manual product manifest: stable Manager identity/version, nativeMessaging/menus/storage, optional cookies/HTTP(S) authority, private execution disallowed. Broader capture permissions or another manifest/version require an explicit policy review. No diagnostic XPI is substituted and no native/setup executable is launched. The existing paired2df904b XPI passes this input policy; the execution below refused installation, not input validation.

The driver retains original closed-app checks, an independent strict process inventory and absence of native registration in all views. A ticket precedes exclusive domain creation. Only the new owned profile/environment is used, with retained browser/fixture objects before launch and conservative retirement. Existing Firefox driver signing-override refusal and protection comparisons remain. Normal profiles are never inspected, copied or injected; registration is never written. This driver makes no concurrent-normal-browser exception.

## Ordinary installation, not an API shortcut

A bounded loopback fixture serves the exact reviewed XPI as `application/x-xpinstall`. A normal trusted link click invokes Firefox's installation UI. The controller recognizes only the visible, enabled site-install and WebExtension-permission prompts, bound to one matching source and the expected add-on identity/required permissions. Unknown prompts, permissions, origins, schema fields or changed labels refuse. An empty optional data-collection category adds no permission; nonempty/unknown categories refuse. UI/security delays remain intact. The actual primary button is clicked, never its internal action callback, and an uncertain click is not replayed.

Privileged automation only observes pending installs, failure events, protection values and the installed add-on receipt. It does not call temporary installation, direct installation APIs, policy injection or protection setters. A failure observer uses Firefox's wrapped install information and exact source/browser binding; undefined error constants cannot classify signature failure, and repeated matching failures become ambiguous rather than overwriting the first outcome.

An active result requires a preceding permission-button attempt followed by an exact active, non-temporary, profile-scope, non-private receipt. After a successful browser exit/join, the installed owned-profile archive must hash-match the input. A second browser starts the same profile **without any load/install call**; active receipt, unchanged protections and exact bytes are checked again. Both browser exits and fixture retirement must be successful before the report.

## Outcomes and limits

`outcome:installed` requires both browser lifetimes and exact persistent receipts. `outcome:signature-requirement-observed` is a separate closed failure observation with no installed receipt, one successful browser retirement and a **nonzero CLI exit**. Neither outcome is inferred in advance. Unknown failures or failed cleanup produce no result report; retained owners and the private domain are preserved. The report distinguishes attempted UI clicks from the later active receipt.

All reports have `qualification:false` and `m5_install_ready:false`. An isolated default environment's refusal is not evidence about normal-profile compatibility and does not introduce signing/accounts or a settings-change requirement. Conversely, a successful result qualifies only the recorded XPI and normal-UI/restart slice—not ordinary automatic capture, the installed companion, a newer package, physical input or final-main artifacts. Package, harness and executable identities remain separate.

## Model verification at preparation checkpointb34e01e

Eleven new models/fixture tests cover archive/permission bounds, actual-button dispatch without uncertain replay, temporary/private/ambiguous receipt refusal, retained failed starts/exits, two-lifetime report guards and active-receipt/permission ordering. A real bounded loopback response is byte-checked and joined. Embedded observation JavaScript is compiled and its exact failure/source/constant handling executed with fake browser globals. Six restored-source mutations reject source/schema, temporary receipt, uncertain-click, undefined signature constant and omitted restart-count guards. Complete npm gates (242 TypeScript tests/14 protocol examples) and108 Python tests pass. This verifies the prepared driver, not the real Firefox UI/persistence path.

## Actual default-profile observation (bc36eb5 / packaged2df904b)

The first b34e01e run failed before its install click during protection observation and produced no result report. A fileless regression reproduced the absent-preference read failure. Commitbc36eb5 reads the preference type first: absent `xpinstall.enabled` is recorded as `null`, not invented as On/Off, and unexpected types still refuse. Signature enforcement must remain a real boolean. No preference is created or changed, and before/after comparisons remain exact.

The absent-preference regression brings the focused persistence suite to12 passing models. A new cleanbc36eb5 run against the unchanged packaged2df904b manual XPI then observed Firefox's exact signature-required failure, bound to the owned browser/source, before any approval-button attempt. Report `persistence62-normal-ui.json` has `outcome:signature-requirement-observed`, CLI exit1, one successful browser exit/join, joined fixture, no installed receipt, no temporary loading and unchanged absent registration. Fresh final app/registration absence and input SHA256 were independently checked. The owned profile reported `signatures_required:true` and absent `install_enabled:null` throughout.

- Packaged XPI SHA256: `d4b6140dd5ab129177bc9e607ae743ff69e35d932aa9a2d25adf1ce6d53b9408`.
- Harness: `bc36eb5f0a556b5c9dae815bd0155a229c7e8e4d`, clean.
- Package source: `2df904b9e04a83a6bd48c1ae4d3e728d9c30f1fb`.
- Firefox Developer Edition156 executable SHA256: `8a6a2339da19ccd55be1832583d6ed83a7f25c03a3fab5fe5af52ebd2e68708c`; Windows11 build26200.

Neither approval UI nor restart persistence was reached. This establishes a default-profile test-environment compatibility limit for these bytes, not normal-profile compatibility, a signing/account requirement, a settings-change recommendation or rejection of unsigned distribution. Exact persistent installation remains unverified; repeating the same unchanged default-profile attempt would not resolve that gap. No installation workaround or protection override was introduced.
