# ADR 0016: Persistent unsigned personal XPI

- Status: Accepted
- Date: 2026-09-11
- Scope: M5/#49–#53
- Supersedes: signed-XPI distribution and signing/account prerequisites in the earlier ADR0013/0014 direction. Their current text and dependent criteria are aligned with this decision; historical release evidence remains version-specific.

## Decision

Distribute a **persistent unsigned personal XPI** for Firefox Developer Edition on Windows 11. Signed XPIs, Mozilla signing/submission, signing credentials, publisher accounts and AMO listing are not required. Do not add a signing workflow to packaging or treat absent signing credentials as a blocker.

Keep existing Firefox settings and browser/Windows protections unchanged. Developer Edition may already accept persistent unsigned installation without a new settings change. Existing compatibility, a configuration change, artifact signing and observed persistence are distinct. No normal-profile inspection, signature-enforcement preference change, profile injection or policy bypass is introduced.

## Acceptance retained

Bind exact unsigned XPI/native bytes to reviewed source, manifests, stable add-on identity, hashes and notices. Verify setup → visible tray → normal unsigned XPI installation → Firefox restart → ordinary supported download click → one Manager task and independently correct output. Temporary loading/reloading, manual Add and mocks do not qualify that sequence.

Persistence of the exact Manager artifact remains unverified; compatibility of another extension is not Manager evidence. An isolated environment's inability to accept an unchanged unsigned artifact is an environment limitation, not a new signing/account requirement. Owned-state and closed-app preflights, conservative cleanup, safe Firefox fallback and exact-final-main qualification remain required.

## Consequences

Packaging has no third-party signing submission or returned signed-byte stage. Hashes and reproducible builds establish content agreement, not publisher authentication. v0.1.0's temporary unsigned-XPI tests and immutable assets remain historical evidence, not the M5 installation recipe. Reintroducing signed distribution would require a new explicit scope decision, not reinterpretation of persistence or protection requirements.
