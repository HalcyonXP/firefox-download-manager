# Parent stdio fixture preparation

## Scope

This is a separate diagnostic for eventual Firefox SDK process/pipe/lifetime observation, **not a Manager controller, protected dispatcher or install candidate**. No browser execution is established here. The normal extension, capture candidate and paired package do not select it.

Here **fileless** means no download-data file, scan or publication. The diagnostic still needs executable, manifest and build inputs; a future browser driver also needs an owned disposable profile. It is not the existing fileless reputation-service probe, whose runner deliberately has no native-process or registration operations. Sharing that adjective does not make their execution or profile modes interchangeable.

| Component | Entry point | Contract |
| --- | --- | --- |
| Native fixture | `crates/test-server/src/bin/parent_stdio_fixture.rs` | Opt-in std-only binary; fixed private argv and bounded stdio; no engine, network, downloads, registry or descendants |
| Session | `extension/parent-probe/session.js` | One attempt, exact fixture records/PID, retained operation and launcher retirement |
| Bootstrap | `extension/parent-probe/api.js` | Explicit SDK globals/imports, fixed no-argument API and nonce-correlated plain observations |
| Background/schema | `extension/parent-probe/{background.js,schema.json}` | One call, no heartbeat/wakeup/network/capture, no caller-selected native data |
| Builder | `scripts/build-parent-probe.mjs` | Fresh CLI compiler process, fixed owned inputs, five bounded payloads; no archive or browser launcher |
| Archive input | `scripts/qualification/parent_input.py` | Externally pinned compiler domain/source/image/record, exclusive five-member archive and bounded independent readback; no compiler/native/browser execution |

The bootstrap intentionally supplies **owned fixture metadata**, not `NativeManifests.lookupManifest`. It does not inspect or change shared native registration. A fixture result therefore cannot qualify registry lookup, installed-image checks, private native IPC authentication or browser policy.

## Fixed native contract

Cargo disables automatic binary discovery and exposes `download-manager-parent-fixture` only with `parent-stdio-fixture`. It is never a package payload. A reviewed owner must place its independently identified bytes at the fixed `download-manager-native-host.exe` name in its own domain. The accepted argv is exactly:

```text
--browser-parent <same-directory/com.halcyonxp.firefox_download_manager.json> download-manager@halcyonxp.local
```

The fixture does not open that manifest. Filename/argv checks are diagnostic structure, **not executable attestation or caller authentication**.

It writes a fixed stderr line and a length-prefixed ready record containing its process ID. It accepts at most one exact UTF-8 ping containing `π`, writes one fixed pong, then waits for frame-boundary EOF. Lengths are nonzero and at most1MiB; incomplete, unknown or repeated frames refuse. Write/flush failures refuse. Success exits0; refusal exits2. No raw input, path or error is logged.

The session compares the native PID with the returned SDK owner's PID. Failed metadata extraction must still return that acquired owner to the retained transport for retirement. A rejected SDK invocation remains indeterminate; no replacement is launched. The background cannot supply a host, argv, URL, hash, verdict or arbitrary frame.

## Observations, not authority

Records carry `scope: "owned-parent-stdio-v1"` and `qualification:false`:

- **echoed** records one completed fixed exchange. It is not private Manager readiness, current browser authority, task completion or a policy decision.
- **retired** includes the launcher's separate startup/process/actual-pipe/callback observations. Success also requires a completed exchange and no session failure. Early pipe rejection, process absence and notification dispatch are not joins.

The observer receives only nonce-correlated plain JSON, not the API object or retained owner. The nonce is correlation, not authentication. A future external controller must observe the receipt itself and independently retain exact process ownership; notification return alone cannot prove delivery.

Context/extension closure uses the existing launcher guards. Session callbacks never await their own retirement or a response from their serialized read loop. Retirement separately waits for the launcher and running operation. A regression exposed unhandled rejection when a synchronous close hook initiated retirement and its later observer notification failed. Rejection is now observed immediately while retaining the original failed promise; it is not converted into a successful or delivered receipt. The existing native-port scheduling tag, shutdown barrier and3-second exact-owner retirement timer remain unchanged. Explicit shutdown is not prevented by idle keepalive.

## Build boundary and checks

The builder accepts only a canonical, ordinary, new diagnostic subdomain immediately beneath `artifacts`, with fixed native image/manifest names. It verifies exact manifest bytes and rereads source/image hashes around compilation. Hash/path checks do not establish executable provenance; builder tests deliberately use **non-executable metadata bytes** and never launch them.

The output is exactly `api.js`, `background.js`, `schema.json`, `manifest.json`, `LICENSE.txt`, plus a separate nonqualifying hash record. The generated manifest has only `nativeMessaging`, a nonpersistent background and its one diagnostic experiment namespace; no sites, cookies, downloads or capture permissions. The stable Manager add-on ID is valid only in a fresh owned profile, never an existing installation.

Compilation is CLI-only, with `ESBUILD_WORKER_THREADS=0`, `ESBUILD_MAX_BUFFER=16777216` and no binary override set **before Node starts**. An initial test imported esbuild before changing its worker environment and timed out before bundle output. Esbuild caches worker support at module initialization; a later worker inherits the changed environment and may not recognize its worker role. The failed test has no established retained join receipt and remains nonqualifying. Corrected tests use fresh bounded synchronous CLI children and preserve their exit/wait records; no SDK worker, timeout or test-concurrency workaround was introduced.

Component evidence currently comprises four Rust units, seven session models and three builder/bundled-SDK cases. The bundled model exercises actual bootstrap/session/launcher/transport bytes with modeled SDK owners, explicit encoder/decoder import, cross-realm detached framing and process-versus-pipe retirement. Fifteen targeted mutations were rejected and restored: six Rust, seven session, one API-argument execution case and one compiled-global source-policy case. Those models are not actual Firefox or native-fixture-process evidence. An initial standalone execution attempt refused its original closed-app preflight before domain creation or fixture launch; the guard was not relaxed.

A subsequent standalone batch at clean `e852421d151aa2b8a1b863539be8a11527ff47f5` passed five cases after a retained clean-source build. The image SHA256 was `b8c635cc822d87afe837de6d631cb4cd8078aba1cd5346deebf3b99adf825b74`. Roundtrip and boundary EOF exited0; unknown frame, repeated frame and ordinary argv exited2. Every case independently compared expected output and fixed stderr; each permitted ready frame also matched the retained process PID. All five observed natural exit and retained process waits plus reader joins. Original closed-app/native-registration-absence preflights passed before each case and afterward. No Firefox, profile or registration operation occurred. This is **standalone Windows process evidence**, not SDK process/pipe/realm, authentication or installed acceptance.

### Archive input boundary

`BuildExpectation` carries the retained controller's exact compiler domain, source commit/dirty state, nonce and image/build-record hashes. These values must originate outside the input being inspected. A caller that merely copies self-reported hashes has not established compiler provenance, image ownership or execution authority. The module does not launch a compiler or validate a compiler wait receipt itself.

The domain is pinned because the compiled API embeds absolute native paths: moving the archive inputs cannot silently redirect their ownership to another domain. Current source bytes, the exact native manifest, all five payload hashes and the closed extension manifest are checked. `clean=False` is an explicit diagnostic readback option, not permission to relabel a dirty build clean. A reproduced model exposed Python dictionary equality accepting numeric0 in place of `persistent:false`; typed JSON comparison now rejects that substitution.

Packing exclusively creates `parent-stdio-fixture.xpi`; an archive appearing after validation is not overwritten. Readback requires an externally retained archive hash, five exact uncompressed ordinary members, bounded sizes, no duplicate/extra members, comments or extra fields, actual member-byte equality and a final input/source recheck. Failed or partially written domains remain for their owner to handle. These checks are integrity boundaries, not protection against an actively hostile same-user process or permission to use an existing profile. Bundles embed owned paths and remain private, nonqualifying test inputs, never release/CI publication inputs.

Twelve metadata-only Python tests and eight rejected/restored mutations cover the compiler-domain pin, external build hash, dirty-state pin, typed manifest, actual archive bytes, final reread, exclusive output and image hash. A retained CLI compiler using **non-executable native metadata bytes** also passed builder-to-archive interoperability. The final validator independently reread that archive with unchanged compiled sources; no native process or Firefox was launched by this check.

Maintainer checks, not installation instructions:

```powershell
cargo test -p download-manager-test-server --bin download-manager-parent-fixture --features parent-stdio-fixture --locked -j 1
node --test scripts/parent-fixture-session.test.mjs scripts/parent-fixture-build.test.mjs
python -m unittest discover -s scripts -p test_parent_input.py
```

## Before live selection

A separate driver still needs reviewed profile/process authority, new-domain tickets, clean source and exact image identity, original fresh closed-app/registration preflights, actual SDK launch/pipe/realm observations and independently retained process waits. Idle, explicit add-on disable and browser shutdown are distinct cases. Unknown startup or failed cleanup must preserve the domain and refuse success.

Archive integrity and standalone fixture evidence do not qualify persistent installation, actual event-page lifetime, private helper readiness, one-controller integration, request/handoff/context association, complete policy enforcement or real-file publication. Those remain requirements in [FIREFOX_PROTECTION_BRIDGE.md](FIREFOX_PROTECTION_BRIDGE.md) and [PROJECT_PLAN.md](PROJECT_PLAN.md).
