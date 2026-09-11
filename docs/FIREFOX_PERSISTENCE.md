# Owned normal-XPI installation observation

Status: implemented/model-tested; **not executed in Firefox**. This is a separate, opt-in installation observation, not M5 install-ready or native-handoff acceptance.

## Scope and input

`python scripts/probe-xpi-persistence.py --package <paired-package> --firefox <Developer-Edition> --report <new-report> --execute-owned-browser`

The existing paired-package validator binds the exact XPI to its package descriptor/payload hashes. `xpi_policy.py` additionally bounds ZIP size/expansion/member counts, checks every CRC, rejects duplicate manifest members and accepts only the current manual product manifest: stable Manager identity/version, nativeMessaging/menus/storage, optional cookies/HTTP(S) authority, private execution disallowed. Broader capture permissions or another manifest/version require an explicit policy review. No diagnostic XPI is substituted and no native/setup executable is launched. The existing paired2df904b XPI passes this input policy; this is file inspection, not observed installation.

The driver retains original closed-app checks, an independent strict process inventory and absence of native registration in all views. A ticket precedes exclusive domain creation. Only the new owned profile/environment is used, with retained browser/fixture objects before launch and conservative retirement. Existing Firefox driver signing-override refusal and protection comparisons remain. Normal profiles are never inspected, copied or injected; registration is never written. This driver makes no concurrent-normal-browser exception.

## Ordinary installation, not an API shortcut

A bounded loopback fixture serves the exact reviewed XPI as `application/x-xpinstall`. A normal trusted link click invokes Firefox's installation UI. The controller recognizes only the visible, enabled site-install and WebExtension-permission prompts, bound to one matching source and the expected add-on identity/required permissions. Unknown prompts, permissions, origins, schema fields or changed labels refuse. An empty optional data-collection category adds no permission; nonempty/unknown categories refuse. UI/security delays remain intact. The actual primary button is clicked, never its internal action callback, and an uncertain click is not replayed.

Privileged automation only observes pending installs, failure events, protection values and the installed add-on receipt. It does not call temporary installation, direct installation APIs, policy injection or protection setters. A failure observer uses Firefox's wrapped install information and exact source/browser binding; undefined error constants cannot classify signature failure, and repeated matching failures become ambiguous rather than overwriting the first outcome.

An active result requires a preceding permission-button attempt followed by an exact active, non-temporary, profile-scope, non-private receipt. After a successful browser exit/join, the installed owned-profile archive must hash-match the input. A second browser starts the same profile **without any load/install call**; active receipt, unchanged protections and exact bytes are checked again. Both browser exits and fixture retirement must be successful before the report.

## Outcomes and limits

`outcome:installed` requires both browser lifetimes and exact persistent receipts. `outcome:signature-requirement-observed` is a separate closed failure observation with no installed receipt, one successful browser retirement and a **nonzero CLI exit**. Neither outcome is inferred in advance. Unknown failures or failed cleanup produce no result report; retained owners and the private domain are preserved. The report distinguishes attempted UI clicks from the later active receipt.

All reports have `qualification:false` and `m5_install_ready:false`. An isolated default environment's refusal is not evidence about normal-profile compatibility and does not introduce signing/accounts or a settings-change requirement. Conversely, a successful result qualifies only the recorded XPI and normal-UI/restart slice—not ordinary automatic capture, the installed companion, a newer package, physical input or final-main artifacts. Package, harness and executable identities remain separate.

## Local verification

Eleven new models/fixture tests cover archive/permission bounds, actual-button dispatch without uncertain replay, temporary/private/ambiguous receipt refusal, retained failed starts/exits, two-lifetime report guards and active-receipt/permission ordering. A real bounded loopback response is byte-checked and joined. Embedded observation JavaScript is compiled and its exact failure/source/constant handling executed with fake browser globals. Six restored-source mutations reject source/schema, temporary receipt, uncertain-click, undefined signature constant and omitted restart-count guards. Complete npm gates (242 TypeScript tests/14 protocol examples) and108 Python tests pass. This verifies the prepared driver, not the real Firefox UI/persistence path.
