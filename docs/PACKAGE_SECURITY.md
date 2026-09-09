# Package security boundaries

Canonical source and review: https://github.com/HalcyonXP/firefox-download-manager/tree/main/docs

This package contains local code, not an updater or authenticated publisher verification system. Download it from the canonical release and compare the published ZIP SHA-256. A hash alone cannot authenticate malicious bytes accompanied by an edited descriptor. First-party code is MIT licensed (`LICENSE.txt`); third-party notices retain their own terms. Licensing is not a publisher signature or a qualification result.

Setup is current-user only, requires closed Firefox/helpers before mutation, and never changes a browser profile, elevation/security policy, routes, firewall or VPN. Its directory leases and fixed-file receipts reject ordinary reparse/traversal/collision/foreign-registration mistakes; they are not a sandbox against a compromised account. Installation uses verified immutable generations and a bounded recovery journal. Unknown or inconsistent files are preserved, not swept. Registry and filesystem durability are not an atomic power-loss transaction. Follow INSTALL.md recovery guidance.

The extension requires nativeMessaging and menus. Cookie/selected-site permission is optional and the per-Add handoff defaults off. Firefox site grants cover all ports; the helper confines supplied context to an exact scheme/host/port and rejects context-bearing cross-origin redirects before contact. Private/container/ambiguous cookie sources and wildcard/IPv6 session permission construction are unsupported; private-window operation is disabled. Minimum Firefox 156 and the exact packaged CSP must be tested in final-browser qualification, not inferred from static schema checks.

Supplied session context is memory-only and excluded from ordinary state/logs. This is not secure erasure against process inspection or dumps. Exact signed URLs, server validators and downloaded content may themselves be sensitive protected recovery data. Never upload raw application state, browser profiles or browser logs.

Output is published only after structural validation and optional user-supplied SHA-256 validation. A digest is not a digital signature or proof of a trustworthy source. Checksum failure publishes no new final file; changing a mistaken expectation requires a fresh Add. Existing output is never silently overwritten. Retained partials require consistent strong resource identity before combining ranges.

Explicitly selected loopback/LAN HTTP(S) targets remain allowed: the helper is not an SSRF sandbox. No TLS-verification bypass, VPN integration, media extraction, torrents, blanket interception, telemetry, cloud sync or remote updater is provided. Known limits and tested environments belong to the release notes for the exact checksummed artifact; candidate builds alone do not qualify a release.
