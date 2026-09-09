# ADR 0015: Explicit cancellation of owned Windows pipe I/O

- Status: Accepted engineering decision; standalone implementation tested, installed qualification pending
- Date: 2026-09-10
- Scope: #50 local transport, not a new owner requirement
- Reversibility: Moderate; replace the boundary if a reviewed dependency exposes equivalent safe, observable cancellation

## Contradiction discovered before integration

The companion design requires joined shutdown without leaving writes waiting indefinitely for a reader. Interprocess 2.4.4's `assume_flushed()` suppresses its own background flush/linger pool, but does not cancel Tokio/Mio's pending writes. Reviewed Mio 1.2.3 `NamedPipe::drop` explicitly cancels reads/connects but preserves writes. Its `Write` implementation can return the entire input length after queuing a write. Thus the initial proposed adapter's no-linger claim was insufficient. No installed bridge or release uses it.

The original all-first-party-Rust `unsafe_code = forbid` policy and the selected wrappers' safe public APIs do not provide the required cancellation operation. Do not silently weaken the shutdown assertion, call a flush, introduce detached threads, or imply write success establishes peer consumption.

## Decision

Keep workspace `unsafe_code = forbid` for every existing crate. Add one small Windows I/O boundary crate with `unsafe_code = deny` and a function-local, documented exception solely for `CancelIoEx` on a retained `BorrowedHandle`. The borrower keeps the exact handle valid during the call. A null OVERLAPPED pointer selects pending requests for that handle; it is not dereferenced by first-party code. No raw handle fabrication, ownership transfer, buffer pointer, SID/token parsing, arbitrary PID, process termination or thread termination is added.

Call cancellation before dropping either pipe role, still suppress Interprocess's extra flush pool, and retain a separate cancellation-failure observation for the session coordinator. A successful cancellation request is NOT an I/O completion or task/command acknowledgement. Runtime/worker joining and native closure/rebind tests must provide their distinct evidence. Failure must not become a successful shutdown report.

This is a deliberate, narrow compiler-policy exception, not an owner-confirmed implementation preference or a relaxation of Windows/TLS/Firefox protections. It must be visible in development/security/dependency guidance. Do not expand this boundary without another explicit review. All protocol, engine, persistence, companion UI and existing setup code retain their stricter lint.

## Alternatives not selected

- Interprocess default flush/linger: a nonreading peer can retain a pipe indefinitely.
- `assume_flushed()` alone: misses the underlying runtime's pending writes.
- Synchronous/nonblocking conversion fixture: an attempted local test hung without a stage trace; do not infer its cause or adopt it as a working cancellation mechanism.
- Authenticated loopback TCP: would change the established transport/confinement design and requires a separate confidentiality/authority review; not substituted to evade the pipe defect.
- Vendoring a modified dependency or running another language's FFI solely to hide first-party unsafe code: obscures rather than resolves the policy exception.

## Required evidence

Retain failed/surviving-mutation history. Verify an actual stalled pending write, peer closure without draining received output, both client/server drop paths, cancellation-failure reporting, endpoint reuse after cleanup, retained cross-process join, and mutation sensitivity. Keep the transport out of the installed engine/bridge until these and the independent authority/lifecycle gates pass.
