# Local transport foundation — #50

Status: development transport plus opt-in companion-engine integration and isolated tests, **not an installed/native-messaging bridge**. The preview companion now serves one controller over IPC; setup, installed authority and native stdio forwarding remain unfinished. Its workspace version does not make it part of immutable v0.1.0. This is an engineering implementation of the visible, browser-independent companion requirement; the owner did not specify these mechanisms.

## Boundaries and vocabulary

- **IPC capability**: a 32-byte OS-random shared key, not a Firefox permission/capability string. Debug output is redacted; there is no automatic serialization, URL logging or secure-erasure claim. Importing 32 bytes does not verify their storage authority or entropy.
- **IPC endpoint**: a canonical UUIDv4 used only to derive `\\.\pipe\HalcyonXP.FirefoxDownloadManager.ipc1.<id>`. No caller-supplied hostname, UNC server or arbitrary pipe path is accepted. It is an address, not authentication or installed singleton proof.
- **Authenticated channel**: both peers proved possession of the capability on this connection. This does not prove Mozilla signing, publisher identity, add-on authorization, installed receipt authority, command delivery or safe replay.
- **Cancellation requested**: `CancelIoEx` accepted the request, or reported no pending request. Not I/O completion, pipe closure, worker join or an application receipt. `CancellationWatch` preserves that distinction and failures; the server also accumulates a sticky failure flag.

## Transport contract

The server uses Interprocess 2.4.4's first-instance creation, noninheritable handles, explicit remote-client refusal, a protected DACL and explicit owner containing only the current interactive user's SID. Current local/domain and Entra SID forms are accepted, not system/service identities. SID acquisition uses the Windows-supplied `whoami /user /fo csv /nh`, selected through `GetSystemDirectory`, never PATH or environment executable discovery. The auxiliary console is suppressed. Only a bounded SID is retained in memory; account-name/OEM bytes and diagnostics are not logged. Output is limited to 8193 bytes for an 8192-byte limit, execution has a two-second budget, and the exact child/reader are retired/joined before success. Cleanup is not asserted to have an unconditional Windows kernel time bound.

The client uses Tokio's pipe constructor with its explicit `SECURITY_IDENTIFICATION | SECURITY_SQOS_PRESENT`, not Interprocess's client constructor. Connecting to a squatted endpoint must not grant impersonation authority. There is no HTTP listener, network URL, remote-server fallback or implicit retry/launch/adoption.

At most four reservations cover pending accepts, handshakes and active sessions. One additional kernel instance is reserved for listener replacement. This library spawns no session tasks or queues. The caller must retain and join bounded reader/writer tasks, close both directions on either failure, and inspect cancellation observations before reporting successful shutdown. Authentication and framing do not waive the independent generation/state-lock/private-capability-storage checks.

Same-user arbitrary code and administrators are outside the confinement boundary. Native same-user connections and descriptor construction are tested; another-account/restricted-token access checks and independent installed-descriptor/ACL readback are not yet qualification evidence. No new account, OS feature or profile was created to imply otherwise.

## IPC1 handshake (separate from application wire2)

All fields have fixed lengths; the whole handshake has a two-second async deadline. No application frame is exposed before mutual verification.

1. Server sends eight bytes `DMIPC\x01\0\0` and a fresh 32-byte OS-random challenge.
2. Client sends its fresh 32-byte challenge and HMAC-SHA256 proof.
3. Server verifies the client proof, then sends its own 32-byte proof; client verifies it.

Each proof covers: role label (`DMIPC1/client-proof` or `DMIPC1/server-proof`), the eight-byte magic/version, the endpoint's 16 UUID bytes, the server challenge and client challenge, in that order. RustCrypto HMAC uses constant-time verification. Distinct roles prevent reflection; both fresh challenges and the endpoint binding reject transcript reuse across changed sessions/endpoints. Tests also use an independently generated Python-stdlib HMAC/hashlib vector. No custom hash/MAC implementation or weaker fallback is used.

Application bodies are opaque 1–1048576-byte frames with four-byte little-endian lengths. Zero/oversize declarations fail before body allocation. After the first prefix byte, the remaining prefix and body share one two-second deadline. Authenticated idle reads can wait; they still consume a bounded reservation and can be cancelled by the coordinator. Writes have a two-second budget. Cancelling a polled read/write removes its owned direction, preventing a later call from forgetting partial-frame progress. Invalid local write lengths fail before I/O. Native JSON/version/command validation is a separate layer, not implemented by this framer.

A successful write can mean runtime buffering, not peer consumption. No flush, transport receipt or automatic replay is inferred. Automatic Firefox capture still needs the separately versioned prepare/commit/uncertain-outcome contract in #49/#51; wire2 Add is not that contract.

## Owned I/O retirement and compiler-policy exception

[ADR0015](decisions/0015-owned-windows-io-cancellation.md) explicitly resolves a discovered mismatch: suppressing Interprocess's background linger alone does not cancel Mio's preserved pending writes. Both pipe roles now request cancellation on the exact borrowed handle before drop. The server also suppresses Interprocess's additional flush pool. Cancellation failures are observable, not silently relabeled successful cleanup.

Only `crates/windows-io` has a function-local FFI exception for `CancelIoEx`, using a retained `BorrowedHandle` and a null OVERLAPPED selector. All existing crates retain workspace `unsafe_code = forbid`; the boundary itself defaults to deny. No handle fabrication, pointer ownership conversion, buffer pointer, process/PID lookup or termination is introduced by that API. A structural policy test confines the exception, but is not a memory-safety/completion proof.

## Evidence and learning history

- Portable core tests cover canonical endpoints/redacted debug, role/endpoint/challenge/key binding, an independent HMAC vector, mutual authentication and wrong-key/fake-server refusal, silent-handshake deadline, exact/max frames, bad length, partial-frame deadline and cancellation retirement.
- Native Windows tests exercise current-user lookup, actual named pipes, exclusive binding/rebinding, capacity, wrong key, silent peer and reservation release. A retained child-process integration test passes its key/address only through the exact child's stdin, exchanges application-level receipts, joins the child and rebinds. No Firefox or native-host registration is involved.
- The first descriptor-builder compile failed because the API required `Some(security)`. Clippy feedback corrected documentation, a let-else and test patterns without relaxing gates.
- The first immediate peer-write-refusal assertion failed once after an earlier pass. A Rust drop is not itself a synchronous peer-side completion observer. No exact original scheduling trace was retained.
- A small-write/asynchronous-peer fixture survived removal of `assume_flushed()`: not reading at the application layer did not establish an unread kernel buffer. Reviewed Mio source schedules read-ahead. That mutation did not prove coverage and is retained as such.
- A synchronous/nonblocking-handle-conversion fixture hung: the runner reported over 60 seconds, and the tool invocation timed out at 120 seconds. It had no stage trace or retained-handle containment record; a later process-name snapshot found no matching test/Cargo process, which is not a reconstruction of successful joining. The experiment was removed, no success report was produced, and its cause remains unresolved.
- A one-MiB first-write stall assertion then failed. Source review established that Mio can return the queued buffer's length; the test now queues a bounded first write and requires the **next** write to stall. It tests both sending roles and observes peer closure without draining output, then checks cancellation status, cleanup and endpoint reuse.
- Omitting the actual cancellation request now fails specifically at the native peer-closure assertion. The owned executable runner recorded exact-handle containment status and process join; it did not use process-name/PID/tree termination. Restored-source tests and final CI remain separate gates.
- Initial dependency policy rejected the newly resolved 0BSD-only `doctest-file` and `recvmsg`, not an advisory. Their complete licenses/metadata were reviewed before adding the explicit 0BSD allowance. Do not hide that failure as an unexplained rerun.

## Integration still required

Private endpoint/capability publication and ACL/receipt readback; installed per-user singleton authority and migration; complete lifecycle qualification of the new one-controller engine session; native stdio forwarding and coordinated cancellation/join; reconnect/uncertain-command semantics; ordinary setup UI/shortcuts/upgrade/uninstall; actual Firefox lifetime/capture and persistent unsigned XPI; final exact-package qualification/publication. Do not close #50 or claim install readiness from this library's tests.

## Initial transport validation checkpoint (historical)

The restored source passes 17 local-IPC unit tests, one retained cross-process integration test and one Windows-boundary error-path test. Endpoint-binding and preallocation-length mutations fail their intended assertions; the omitted-cancellation mutation separately fails native peer closure. Full workspace Clippy and tests/build, JavaScript checks and reviewed dependency policy have run; authoritative new-head CI remains required.

The first full workspace build failed with E0786 while mapping Rust core metadata: Windows reported OS error 1455 (paging-file/commit capacity), followed by an allocation failure. No corrupt-toolchain diagnosis was established and nothing was reinstalled or deleted. A later read-only preflight observed 19,143 MiB available physical memory and 17,601/63,395 MiB committed/limit, with no matching Cargo/compiler process. The workspace tests and build then passed with **one Cargo build job**, preserving default test concurrency and every assertion/deadline. No paging-file/OS configuration or unowned application was changed. That later memory sample does not reconstruct peak usage during the failed build.

## Unreleased IPC1 limit reconciliation before engine integration

The initial 44d6e1d transport chose 64 KiB, while `crates/protocol/src/lib.rs` and `extension/src/protocol.ts` require 1 MiB and `native-connection.ts` checks the advertised limit for exact equality. That would have made a transparent bridge incompatible. Before connecting any engine/installed host, IPC now imports the native `MAX_MESSAGE_BYTES` constant as its frame limit instead of maintaining a smaller independent value. This explicitly revises the unshipped IPC1 draft, not the immutable wire2 release or Firefox's expected contract.

A native-decoder-valid hello padded with legal JSON whitespace to exactly 1 MiB now crosses the transport byte-for-byte; empty/oversize input still fails before body allocation, and deadlines/cancellation are unchanged. The 64 KiB limits for qualification metadata/ordinary logs are separate contracts and are not enlarged. At most four admitted sessions remains unchanged; future coordinator queues need explicit byte budgets as well as frame counts before integration. The initial 19 checks/44d CI are historical input evidence; the revised frame limit needs its own new-head validation.

The revised limit passes 18 IPC unit tests, one retained cross-process test and one boundary error-path test (20 total), full workspace tests/build/Clippy with one Cargo build job, npm check, dependency policy and privacy206. Restoring the old 64 KiB value fails the new native-compatibility assertion. New-head CI remains required; none of this qualifies the installed bridge.

## First engine integration: evidence scope and learning

The opt-in native-host `local-bridge` feature now lets the preview companion's retained worker serve ONE browser protocol controller against its owner. This is not native stdio forwarding or installed endpoint discovery. The released default stdio entry point retains EOF shutdown. Input uses an eight-frame queue: 8 MiB queued, one body awaiting delivery, and one body under dispatch (10 MiB input bodies maximum). Output awaits one encoded frame at a time rather than collecting history into an output queue. Snapshot pages remain ordered before deltas; only each page is projected, while captured history/list memory still scales with task history. Encoded/decoded structures and wrapper/kernel buffers are additional memory, not hidden inside that input-body count.

The initial real-pipe tests failed because the fixture expected event sequence 1; the unchanged native session starts at 0. Corrected the fixture to the existing protocol, and corrected a latent Add-field typo to `suggested_filename` before that path ran. The next run passed the 80-task history/second-controller/refused-peer reconnect case, but observed `promoting` after the correct final file appeared. Engine source promotes before persisting Completed; the test now separately waits for a Get Completed receipt, without changing that transition, loosening output checks or inferring completion from file presence. These failures and their private domains remain retained.

The restored tests exercise a real 64 KiB fixture transfer continuing after pipe disconnect, one-task reconnect and byte-for-byte independent fixture comparison; 80-task incremental history and second-controller refusal/reconnect; and an 800-task nonreading peer with observed pending client output, joined Quit, real closure, state-lock release and endpoint rebind. They do not identify a kernel buffer byte count or prove Firefox capture, signed install, cross-account ACL enforcement or installed generation authority. Final checks/mutation evidence and current-head CI remain separate gates.


Additional actual-pipe checks cover refused URL commands/current settings across reconnect and idle-controller Quit (five engine/pipe tests total). The first omitted-output-drop mutation failed at a generic observation deadline, not its intended Quit assertion; this was retained and the observer now names its stage. The nonreading-write variant then survived that mutation: cancelling the in-progress write had already retired the output direction, so that fixture could not prove explicit idle-output drop. The new idle-controller fixture rejects the omission at `retained companion worker must join successfully`. Making controller loss shut down the owner separately fails `owned engine/IPC observation deadline: final file promotion`. Both final mutations use exact retained executable handles, join before recording their rejected result, and restore source; neither required forced termination. Earlier mismatched/surviving attempts are not sensitivity evidence.

The preview GUI now starts this explicit memory-only binding only after its existing visible-tray gate. It publishes no key/endpoint file and changes no native registration. Default stdio remains legacy; the bridge feature is selected by the companion only. Workspace metadata/notices may conservatively include the newly reachable optional dependency closure; that does not qualify new package bytes. The final server cancellation-failure readback also covers a pending unauthenticated accept retired by Quit. No additional FFI allowance is introduced.

After the earlier Windows commit-capacity failure, the Windows CI quality job now also uses one Cargo build job. Test concurrency, assertions, individual deadlines and the 30-minute job deadline are unchanged. The preceding a3a7917 CI34402091393 passed; its success does not cover this engine increment.


Restored-source workspace tests/build/Clippy passed with one Cargo build job; the existing optional hedging measurement remains ignored, not newly qualified. Default-feature native-host Clippy also passed. npm check and dependency policy passed. The new native preview report `artifacts/companion50-engine-ipc-preview.json` passed six owned Windows x64 checks on a dirty identified development tree; it is not inherited from d88e7b7, an installed bridge report or physical tray input. Tracked-file privacy screening and current-head CI must be checked for the final staged/committed source.

## Protected-record component (not installed discovery)

Setup's opt-in `private-file` feature now provides create-new protected file creation, independent parent/owner/DACL readback and retained file/ancestor leases. See [PRIVATE_RUNTIME_RECORD.md](PRIVATE_RUNTIME_RECORD.md) for the 4 KiB content bound, protected-parent prerequisite, fixed auxiliary-process protocol and tests. Secret bytes remain in Rust. The component does not seal or adopt a directory, choose the installed record schema, verify a receipt/generation, acquire engine authority or replace stdio. No installed caller selects it, and the preview still publishes no endpoint/key file.

The separate opt-in `private-directory` creator prepares a new protected `companion-runtime` directory below an independently owned parent, retaining read leases across access grant and refusing existing entries. Its CurrentUser SID observation and directory creation witness are not installation receipts or engine authority; installed entry points and record/generation binding now exist behind opt-in features, without installed qualification. See PRIVATE_RUNTIME_RECORD.md for SDK layout review, namespace policy, failed metadata-only lease assumptions and mutation evidence.


An unselected `installed-runtime` composition now binds protected records to independently inspected current-image/receipt/generation/manifest/registration metadata. Server getters permit explicit protected storage of the transport key; HTTP session credentials remain memory-only. Runtime record1 does not change IPC1's proof/framing or wire2. A privately owned bound server is exposed only after successful record publication. The image borrow and publication witness do not acquire an engine state lock, launch an installed process or prove browser installation. See PRIVATE_RUNTIME_RECORD.md for schema, bounded candidate enumeration and outstanding discovery/stdio/lifecycle gaps.


The opt-in application now forwards native stdio through exact owned I/O-only children and retained parent threads; two real-executable tests connect it to the existing engine/controller. The installed worker also connects retained image inspection to state-lock ownership and private runtime publication. These are implemented entry paths, selected only by the development paired package, not installed-browser qualification; COMPANION_DESIGN.md and SHORTCUT_OWNERSHIP.md record implemented receipt2 shortcut ownership and the remaining installed handoff/Start Menu/browser qualification gates.

Follow-up private-parent-pipe tests now cover output-consumer stalling and held native input after parent-side pipe loss. I/O-only process self-retirement is distinct from cooperative thread joining; normal relay retirement still waits for exact processes and joins retained parent threads. See the application checkpoint for the two initially failing observations and implemented liveness channels.

Paired development packaging now selects application/setup together; installed launch requests derive the current helper through existing setup coordination and retained image/registration checks. Package probes, UI observations and memory-only-registration lifecycle tests still do not qualify installed Firefox transport. Post-Quit tests distinguish bounded buffered terminal events from actual transport termination; see COMPANION_DESIGN.md for the failed first-read premise and negative live-peer check.
