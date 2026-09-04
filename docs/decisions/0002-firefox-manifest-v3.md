# ADR-0002: Use Manifest V3 with a Firefox event page

- Status: Accepted
- Date: 2026-09-04
- Reversibility: Moderate; review against supported Firefox Developer Edition releases

## Context

The initial release targets Firefox Developer Edition rather than cross-browser stores. Both Manifest V2 and V3 are available, but the project should begin on the current permission and lifecycle model without pretending Firefox supports Chrome's background implementation.

Firefox does not currently support `background.service_worker`; for Manifest V3 it supports `background.scripts` as a non-persistent event page. The architecture already requires the native helper to be authoritative and the UI to recover from snapshots, so a restartable background context is acceptable.

## Decision

Use Manifest V3 and a Firefox `background.scripts` event page with `persistent: false`. Register event listeners synchronously, keep no authoritative task state in the background page, reconnect Native Messaging when needed, and recover from a helper snapshot.

Declare a stable Gecko extension ID because the native-host manifest must allow exactly that ID. Request only permissions justified by implemented behavior. As implemented for issue #10, the sole permission is `nativeMessaging`; there are no host permissions. The native connection remains dormant until a manager surface asks for it, and each new port is usable only after hello plus an atomically assembled complete snapshot. Broad host or cookie access is deferred until the authenticated-download issue can justify and minimize it.

Reference: [MDN `background` manifest key](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/manifest.json/background).

## Rejected alternatives

- **Manifest V2 persistent background page:** simpler for connection lifetime, but retains the legacy model and encourages authoritative in-memory state.
- **Chrome-style service worker only:** unsupported by Firefox and inconsistent with the target platform.
- **Dual-browser manifest in the first release:** adds permissions/build complexity outside current scope.

## Consequences

Background suspension and reconnection are expected behavior and must be tested. Build tooling may later emit browser-specific manifests if cross-browser support enters scope; that does not require changing the engine protocol.
