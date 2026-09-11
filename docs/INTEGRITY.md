# Completion and optional SHA-256

Implemented for #25. Target: Windows 11; wire protocol v2, internal task format v4.

## Contract and vocabulary

- **Expected SHA-256** is the immutable digest explicitly supplied for one Add, not a value taken from server headers. Obtain it from a trusted source. A checksum is not a signature or proof that its source is trustworthy.
- **Structural validation** always requires joined writers, known exact size, and complete, disjoint, bounded coverage. Unknown-length streams must first seal a clean bounded EOF. Preallocation alone is never completed coverage.
- **Computed fingerprint** is a complete64-bit byte length and SHA-256 computed from the retained file handle during validation, not a declared/server/cached expectation. A copied fingerprint is information, not a lease or protection verdict.
- **Validation lease** is non-cloneable storage ownership that freezes helper writes, retains the file lock, and permits one no-overwrite promotion. It is not a persisted assertion that arbitrary future file contents are valid.

Blank input means structural validation only. Otherwise the UI accepts 64 hexadecimal characters; the creation boundary canonicalizes case. Native decoding independently rejects other algorithms, malformed digests, duplicate keys, and unknown shapes. `sha256` must be advertised after the most recent reconnect; unsupported helpers never silently receive an Add with its expectation removed. The reserved v2 shape is now implemented without a wire-major change.

## Read, validate, then publish

1. Checkpoint completed transfer coverage and enter `validating`.
2. On a joined Tokio blocking task, freeze storage assignments and check the owned handle/path identity, exact coverage and length, exclusive file lock, and successful flush.
3. If requested, seek the owned file to zero and feed RustCrypto `sha2` with one **256 KiB** buffer, requiring exact read length and EOF. This includes resumed ranges and disk bytes, not arrival-order network chunks. Compare all 32 digest bytes.
4. Retain the lease across the critical `promoting` checkpoint. Only its create-new promotion may expose the final name; collision suffixing preserves existing files.
5. Checkpoint promotion/partial-link cleanup and completed state, then emit success. Read/flush/lock/hash failures cannot take this path.

Cancellation is checked between bounded reads. The engine awaits blocking validation rather than abandoning its thread, releases the lease, then completes the existing stop/checkpoint path. A blocking filesystem call itself is not forcibly interruptible. The UI offers Cancel during validation, not Pause; it does not mislabel the last download rate/ETA as hashing progress. Promotion remains a non-cancellable atomic publication boundary.

Engine inactivity is distinct from coordinator retirement. Callers closing an engine must await its retained coordinator joins; see [COORDINATOR_OWNERSHIP.md](COORDINATOR_OWNERSHIP.md).

Windows byte-range locks reject ordinary competing file I/O during validation. They do not defeat malicious same-user processes, memory-mapped mutation, all namespace races, or hardware faults; other platforms may only provide advisory locking. The lease independently enforces helper-local ownership. Security/release review must not turn this into a claim of isolation from a compromised local account.

## Opt-in fingerprint interface

`PartialFile::validate_with_fingerprint(expected, cancelled)` always hashes the complete validated main stream, including empty files, whether or not an expected checksum was supplied. It shares the original bounded-read, coverage/length/identity/locking/cancellation path. `ValidatedPartial::fingerprint()` returns the path-free `ValidatedFingerprint` only after successful hashing and any requested checksum comparison. Debug output redacts it. An ordinary `validate(None, ...)` still performs structural validation without hashing and returns no fingerprint.

The validation lease remains non-cloneable and blocks competing helper validation/promotion until consumed or dropped. Dropping it permits a new validation attempt; that attempt must compute new evidence, not reuse a copied fingerprint. The new interface is groundwork for native-byte-bound protection. It does not add a verdict, challenge, browser context or mandatory publication gate, and existing task completion does not select mandatory hashing without an expected checksum. No wire/persistence format or dependency changes are introduced.

## Opt-in fixed-name binding

`ValidatedPartial::bind_final_name(index)` consumes a fully hashed lease and returns a non-cloneable `NamedValidatedPartial`. Index0 freezes the original sanitized component;1–9999 use the existing bounded numbered-name policy. `file_name()` and `fingerprint()` expose that immutable name and computed full-length identity while the owner retains the validation lock. Binding performs no directory lookup or reservation. Structural-only leases and out-of-budget indexes refuse; the refused lease is dropped.

Named `promote()` attempts exactly that one component through the existing coverage/length/flush/identity/Internet-zone/create-new path. A late collision cannot select another name or overwrite the existing entry. Failure consumes the binding and releases the lease; another attempt needs fresh validation and separately established current authority. The primitive does not query reputation or consume a verdict. Existing task completion does not select it: ordinary promotion retains automatic numbering and ordinary no-expectation validation remains structural-only.

An actual storage counterexample first confirmed that ordinary publication could choose an alternate after a late collision while preserving the existing file. That is correct ordinary no-overwrite behavior, but insufficient for a prior exact-name decision. Three fixed-name regressions now cover refusal/no alternate, mandatory fingerprint/index bounds, Windows competing-writer exclusion, drop/revalidation/changed bytes, an explicit nonzero final index, Internet-zone readback and redaction. Two compile-fail examples reject Clone/Copy; six executed/rejected mutations cover fallback, index binding/budget, mandatory fingerprint, held lock and name redaction. No browser verdict, native challenge/task binding, restart recovery or installation acceptance is inferred.

## Mismatch and recovery

`CHECKSUM_MISMATCH` fails before promotion, with no final file or success event. The existing helper-owned **retain partials on failure** setting explicitly governs retention (default: retain). Retained mismatched bytes are not silently treated as success, a fresh single stream, or an automatically corrected digest. Check the expected digest and Add a fresh task; Remove task & partial deliberately removes the old partial. Explicit retry retains the original expectation and can fail again on the same retained bytes.

Format v4 requires `expected_sha256` to be present as null or exactly 64 hex characters; parsing accepts either case and serialization emits lowercase. It also enforces the documented presence of nullable task/validator keys, resolving the earlier Serde Option omission discrepancy. Dedicated v1/v2/v3 shapes migrate with no checksum because those versions could not accept one. V1 supplies its historical four-worker default; v1/v2 supply `needs_session: false`; v3 preserves its required session marker. New fields disguised as an old format, missing current keys, invalid digests, and future versions fail closed.

Every retry/resumed prepublication run rehashes the complete owned file if an expectation exists. Interrupted validation recovers as failed and requires explicit retry. The digest is not a credential, but is deliberately redacted from ordinary Debug output and not added to logs or wire snapshots. Existing published files, including recovered completed/promotion records, are not continuously rehashed; validation is a pre-publication check, not a file-monitoring service.

## Evidence and limits

- Fingerprint tests additionally cover hashing without an expectation, absence on structural-only validation, cancellation/expected mismatch, recomputation after changed bytes, lease exclusion, active writers/gaps and redacted output.
- Storage tests cover independent empty/`abc`/million-`a` vectors, bounded read cadence, cancellation/revalidation, same-length disk corruption, active writers/gaps/length changes, exclusive lease ownership, and Windows competing-I/O/lock release.
- Engine tests cover 1/2/4/8 configured workers, ranged/ignored-range/unknown-length/empty responses, collisions, validation → promotion → completion event ordering, both mismatch-retention policies, and restart/retry without dropping an expectation. Fixture digests were computed independently using Python `hashlib` and the published fixture formula.
- Native dispatch tests assert `CHECKSUM_MISMATCH` snapshots. The actual process-kill/restart test now requires the independently computed 8 MiB digest `8da825cc025655c14fd604596e953db07bfdacdfa12361af4f89d67f00eaa934` as well as byte equality.
- Extension tests cover digest construction/rejection, capability loss after reconnect, and validation controls/projection.

These are local/CI integration boundaries, not multi-gigabyte memory/performance measurements or new real-Firefox checksum UI evidence. The earlier real-Firefox authentication slice predates this feature. Broader installation, browser, security, performance, and artifact qualification remain #26–#28.
