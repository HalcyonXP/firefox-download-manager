# Protected runtime-record primitive

Status: an opt-in Windows component in setup's `private-file` feature. No installed caller selects it yet. It is **not** installed bridge, singleton, generation, native forwarding or Firefox qualification. The default setup package does not select this feature.

## Purpose and authority limits

A future companion must publish its endpoint/capability without exposing a secret before permissions are established. `PublishedFile::create` and `PrivateFile::open` provide one fixed `companion-runtime.json` file, not its eventual record schema. Contents must be 1–4096 bytes. The caller must independently establish directory ownership and installation/receipt authority; `DirectoryLease` alone establishes neither.

The supplied directory must already have a **protected DACL** (discretionary access-control list), a current-user/SYSTEM/Administrators owner, and no other unprivileged principal able to mutate it or its children. Protection prevents later ancestor inheritance from reopening the namespace. The adapter does not seal, repair or adopt a supplied directory. Preparation of the installed private directory remains unfinished.

Only ordinary allow/deny ACEs (access-control entries) are accepted in the parent. Inherit-only ACEs do not apply to that directory. Allow masks for other principals must exclude generic all/write, delete, change-DACL/owner, add-file/subdirectory, write-EA/attributes and delete-child rights (`0x500d0156`). Other read permissions are not secret-file permissions: the file gets its own protected descriptor with no inheritance.

Same-user arbitrary code, Administrators and SYSTEM are outside this confidentiality boundary. Actual cross-account/restricted-token behavior remains unqualified. No new account, privilege adjustment or service is involved. No claim is made about secure erasure or encrypted storage.

## Creation, readback and leases

1. Retain canonical ordinary-directory ancestors against rename/deletion. Reject ambiguous, nonlocal and reparse paths using existing setup confinement.
2. Resolve Windows PowerShell through `GetSystemDirectory`, never PATH or an environment-supplied executable. Run a fixed embedded script with `-NoProfile -NonInteractive` and `CREATE_NO_WINDOW`. No execution-policy setting, profile, registry, network call, external child command, custom P/Invoke or dynamically compiled interop is used.
3. The auxiliary process receives **only operation and path over stdin**. It derives the current per-user SID from its Windows identity. No contents, key, hex/base64 payload or supplied path is placed in command-line arguments. Secret contents never enter PowerShell, including its parameter logging.
4. After checking the parent, .NET `FileStream` creates a new empty file with the protected current-user-only full-control descriptor **in the OS creation operation**. Existing entries are refused, never truncated or adopted. The child retains read/write access and denies deletion.
5. Actual owner/DACL readback must find precisely the current user as owner, a protected DACL and one non-inherited/non-inheritable allow ACE granting that user exact file full control. Null/broad/other-principal/unprotected descriptors are refused.
6. The child sends `ready`; this means a retained empty protected file, **not a published record or an engine ready signal**. Rust opens that same non-reparse file for writing while the creator's handle prevents replacement. A second auxiliary invocation independently checks actual permissions before Rust writes anything. Rust writes and calls `sync_all`; the auxiliary process never reads or writes those contents.
7. Rust drops its writer, sends `close`, observes the child's exact `ok` receipt and successful process exit, and joins its I/O worker. A normal `PrivateFile` reader denies shared writes, so it cannot open the record while the creator/writer remains active. File-name presence is not readable publication.
8. The publisher reopens a read-only, no-write/no-delete lease, independently verifies actual permissions again **before reading**, and compares the complete bounded contents to the original bytes. Readers use the same pre-read checks. Only successful create-new publication yields the removal-capable wrapper; read-only opening does not authorize removal or adoption.

The parent and file leases remain with the returned object. Removal closes the publisher's read handle while retaining ancestors and removes only its fixed file; competing readers cause refusal. No recursive directory cleanup or stale-record recovery is provided. Failed, empty or partially written creations remain for separately owned recovery; an error never authorizes replacement.

## Auxiliary-process boundary

Input is at most 2 KiB and contains two JSON fields. Readiness is exactly six bytes; completion is exactly two, with a one-byte excess detector. Diagnostics are fixed; OS exceptions are not echoed. A retained worker handles potentially blocking stdin/stdout. The five-second per-adapter deadline bounds waiting for helper work, not an unconditional bound on filesystem I/O or joined cleanup. Early errors close the coordination channel, retire only the exact retained child handle and join the worker. Process exit, receipt validity and worker join are separate checks. A blocked or constrained Windows PowerShell environment refuses operation; protections are not changed to obtain success.

This reuses the already reviewed winsafe system-directory API and Windows-supplied .NET file/security classes, not a new Rust FFI exception or a new registry dependency. The feature does not alter ADR0015.

## Component evidence

Two real Windows tests exercise the owned lifecycle and malformed-receipt/stalled-child cleanup. Fixtures contain deterministic public bytes, not actual credentials. Cases include Unicode/space/apostrophe paths, empty/oversized refusal, exact 4 KiB handling, preservation of unleased existing contents, retained-creator sharing/deletion refusal, abort preservation, concurrent readers, write/deletion leases, extra/wrong-principal/null/unprotected file ACL refusal and unprotected/other-writable parent refusal. The stalled-child case first observes real readiness, then uses a shorter 100 ms test-only deadline and checks file-handle release after joined retirement.

Initial attempts failed because `Get-Acl` was unavailable in the auxiliary invocation and enum-to-Boolean conversion raised `InvalidCastException`. The selected script uses .NET file/directory access-control APIs directly and explicit integer flag checks. A separate Python-created diagnostic parent was refused by the writer policy; that refusal was not weakened or relabeled as a successful fixture.

Six restored-source mutations fail their intended observations: replace CreateNew with overwrite (existing bytes change), omit file protection or current-principal verification (unsafe read is accepted), release the creator before readiness (the incomplete file becomes openable), omit parent protection, and omit the parent writer mask (unsafe publication is accepted). Each exact retained test executable joined without forced termination. One runner edit had a mismatched quoted replacement and stopped before applying the parent mutation; its finally restored source, and the corrected parent-only run supplied the remaining two results.

Restored full-workspace formatting, all-target/all-feature Clippy, tests and build pass with one Cargo build job; npm checks and dependency policy pass. Staged privacy-pattern screening passes for 211 tracked files; it is not a platform/history audit. Current-head CI remains a separate gate. These tests do not establish installed authority, cross-account isolation, filesystem crash atomicity, persistent XPI installation, capture or a completed download receipt.

## 09781e1 hosted release-target failure

CI34424720412 passed Windows debug quality and dependency policy, but **failed** the package candidate-build step: both new protected-file tests failed during initial fixture-adapter startup in the LLVM/UCRT release-target workspace run (about five seconds). The repeated build, byte comparison and dependent emulation did not qualify. This was not the previous combined-job budget cancellation. No phase trace establishes the cause.

A focused local reproduction using the same pinned LLVM/UCRT target, release profile, static/remap flags and one Cargo build job passed both tests without changing source or deadlines. That does not explain or erase the hosted failure. Test-only fixed phase diagnostics now distinguish retained process launch, worker entry, completed request write, readiness/completion byte reads, close write, deadline/early exit and returned wait/join operations. They contain no input/path/SID/content values; production diagnostics, protocol, permissions and five-second deadline are unchanged. This is an investigative change, not a claimed behavioral fix or permission to rerun unchanged until green.
