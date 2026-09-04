# ADR-0006: Build an original implementation with auditable dependencies

- Status: Accepted
- Date: 2026-09-04
- Reversibility: Dependencies can be replaced; provenance and license duties remain

## Context

Existing download managers can inspire product behavior, but copying implementation may import incompatible licenses, hidden security assumptions, or attribution obligations. Reimplementing commodity cryptography, TLS, HTTP parsing, or serialization would also be unsafe.

## Decision

Write project-specific scheduling, validation, state, protocol, extension, and installation code from the requirements and public standards. Do not copy third-party implementation code unless its exact source, license compatibility, attribution, and maintenance impact are reviewed and recorded first.

Use maintained libraries for security-sensitive and commodity primitives rather than inventing them. Pin resolved dependencies with lockfiles, minimize direct dependencies, retain required notices, and make license and vulnerability reports reproducible in CI. Repository snippets and generated code are subject to the same provenance review.

Design inspiration may be documented with links, but does not authorize copying. Tests should use project-created deterministic payloads and fixtures.

## Rejected alternatives

- **Fork an existing manager:** likely exceeds scope and inherits architecture/licensing decisions not reviewed here.
- **Copy selected range/scheduler code:** provenance and subtle invariant mismatch outweigh short-term speed.
- **Avoid all dependencies:** would require unsafe reinvention of HTTP/TLS and tooling.
- **Unpinned dependencies or CDN scripts:** harms reproducibility and supply-chain review.

## Consequences

Issue #3 must establish lockfiles and dependency-license/audit commands. Pull requests introducing dependencies must explain necessity and license impact. A project license should be selected explicitly before external distribution; repository privacy is not a substitute for provenance records.
