# Issue and implementation history migration

On 2026-09-08, #8 made **HalcyonXP/firefox-download-manager** the public, authoritative repository. GitHub transferred 22 regular issues, preserving their state, creation time, author, acceptance criteria, and comment history. GitHub assigns new issue IDs/numbers and adjusts references; the mapping below makes that explicit. Five milestones and project-board statuses were preserved under their new issue identities.

All current documentation uses the new numbers. In commit messages and historical snapshots **before this migration**, numeric issue/PR references belong to the predecessor namespace. Do not treat those old numbers as current work IDs. No private PR refs or sensitive original commits were imported; the predecessor remains private solely as an archive.

## Work items

| Historical issue number | Current issue | Work |
| --- | --- | --- |
| 1 | [#9](https://github.com/HalcyonXP/firefox-download-manager/issues/9) | Record architecture, scope, and security decisions |
| 2 | [#10](https://github.com/HalcyonXP/firefox-download-manager/issues/10) | Define the versioned extension/native-helper protocol |
| 3 | [#11](https://github.com/HalcyonXP/firefox-download-manager/issues/11) | Scaffold the WebExtension, Rust workspace, and CI |
| 4 | [#12](https://github.com/HalcyonXP/firefox-download-manager/issues/12) | Build a deterministic adversarial HTTP test server |
| 5 | [#13](https://github.com/HalcyonXP/firefox-download-manager/issues/13) | Implement HTTP probing and strict range-response validation |
| 6 | [#14](https://github.com/HalcyonXP/firefox-download-manager/issues/14) | Implement safe random-access partial-file storage |
| 7 | [#15](https://github.com/HalcyonXP/firefox-download-manager/issues/15) | Persist task and segment state for resumability |
| 8 | [#16](https://github.com/HalcyonXP/firefox-download-manager/issues/16) | Implement the fixed-concurrency segment scheduler |
| 9 | [#17](https://github.com/HalcyonXP/firefox-download-manager/issues/17) | Implement pause, resume, cancellation, retries, and progress events |
| 10 | [#18](https://github.com/HalcyonXP/firefox-download-manager/issues/18) | Implement and register the Windows Native Messaging host |
| 11 | [#19](https://github.com/HalcyonXP/firefox-download-manager/issues/19) | Add “Download with Manager” context-menu and creation dialog |
| 12 | [#20](https://github.com/HalcyonXP/firefox-download-manager/issues/20) | Build the download queue and progress dashboard |
| 13 | [#21](https://github.com/HalcyonXP/firefox-download-manager/issues/21) | Add local settings and diagnostic logging |
| 14 | [#22](https://github.com/HalcyonXP/firefox-download-manager/issues/22) | Validate resource identity during resume and crash recovery |
| 15 | [#23](https://github.com/HalcyonXP/firefox-download-manager/issues/23) | Support authenticated downloads with minimal session handoff |
| 16 | [#24](https://github.com/HalcyonXP/firefox-download-manager/issues/24) | Harden fallback, retry, and throttling behavior |
| 17 | [#25](https://github.com/HalcyonXP/firefox-download-manager/issues/25) | Add final integrity validation and optional checksums |
| 18 | [#26](https://github.com/HalcyonXP/firefox-download-manager/issues/26) | Perform extension-permission and native-helper security review |
| 19 | [#27](https://github.com/HalcyonXP/firefox-download-manager/issues/27) | Package local Windows installation and removal |
| 20 | [#28](https://github.com/HalcyonXP/firefox-download-manager/issues/28) | Run end-to-end qualification and publish the first local release |
| 40 | [#29](https://github.com/HalcyonXP/firefox-download-manager/issues/29) | Remove personal identifying information before public visibility |
| 41 | [#30](https://github.com/HalcyonXP/firefox-download-manager/issues/30) | Complete privacy isolation for a clean publication repository |

## Milestones

| Historical milestone number | Current milestone |
| --- | --- |
| 2 | [M0 — Foundation](https://github.com/HalcyonXP/firefox-download-manager/milestone/1) |
| 3 | [M1 — Native download MVP](https://github.com/HalcyonXP/firefox-download-manager/milestone/2) |
| 4 | [M2 — Firefox integration](https://github.com/HalcyonXP/firefox-download-manager/milestone/3) |
| 5 | [M3 — Reliability and authenticated downloads](https://github.com/HalcyonXP/firefox-download-manager/milestone/4) |
| 6 | [M4 — Local release](https://github.com/HalcyonXP/firefox-download-manager/milestone/5) |

## Historical implementation PRs

These links point to **cleaned public commits**, not inaccessible private PR pages or pre-rewrite hashes. They preserve implementation provenance without importing sensitive PR ancestry. Detailed decisions and evidence remain in the transferred issues and ADRs; the private archive preserves original review records.

| Historical PR number | Cleaned public implementation |
| --- | --- |
| 21 | [Record architecture and security decisions](https://github.com/HalcyonXP/firefox-download-manager/commit/b339e8d172e14be3af26439c65f2fc99683f4cbc) |
| 22 | [Define Native Messaging protocol v1](https://github.com/HalcyonXP/firefox-download-manager/commit/cef05fbbe298a38eed2e5bbec0337695937994c5) |
| 23 | [Scaffold extension Rust workspace and CI](https://github.com/HalcyonXP/firefox-download-manager/commit/e6bef7665dd287acf3afb63653c3614b9920178d) |
| 29 | [Add adversarial HTTP test server](https://github.com/HalcyonXP/firefox-download-manager/commit/fb0033c8764399c7fc5de531cde47e382866c13c) |
| 30 | [Implement HTTP probing and strict range validation](https://github.com/HalcyonXP/firefox-download-manager/commit/d47607b2fac8b239c56bee4429bdbe1244e9ab56) |
| 31 | [Implement safe random-access partial storage](https://github.com/HalcyonXP/firefox-download-manager/commit/af8cf87574c798c61cf9675235ec29cade0ddeb1) |
| 32 | [Persist crash-safe resumable task state](https://github.com/HalcyonXP/firefox-download-manager/commit/03e97f53414a1be584c2a5b0f8e2b42dfcce5634) |
| 33 | [Implement fixed-concurrency segment scheduler](https://github.com/HalcyonXP/firefox-download-manager/commit/856ec235da51973de90f790aef61575ba380b336) |
| 34 | [Implement durable task controls and progress](https://github.com/HalcyonXP/firefox-download-manager/commit/47d20cb0af48fbf90d615a578068a5957b52112d) |
| 35 | [Implement Windows Native Messaging host](https://github.com/HalcyonXP/firefox-download-manager/commit/483e8d4adf8b9e8900fa4e5f8166b6ef95c5222f) |
| 36 | [Implement explicit Firefox download creation workflow](https://github.com/HalcyonXP/firefox-download-manager/commit/93353cf2bd00697d528a9b28fe141c546510999a) |
| 37 | [Build snapshot-driven queue dashboard and protocol v2 folder control](https://github.com/HalcyonXP/firefox-download-manager/commit/5d08d5226f52d2e0d742f9d7a68f83aa3e6e2805) |
| 38 | [Implement local settings and bounded private diagnostics](https://github.com/HalcyonXP/firefox-download-manager/commit/fc2493b00e10aeda48e1d06f35c5092fd2e5bdea) |
| 39 | [Require strong byte identity for segmentation and recovery](https://github.com/HalcyonXP/firefox-download-manager/commit/79de2543c632010ada69a4746c8227863d3273a8) |
| 42 | [Add publication privacy guard and document history remediation](https://github.com/HalcyonXP/firefox-download-manager/commit/5cb10b5218744d888f5354d63964d0b3ca5e9fbc) |
| 43 | [Isolate publication target and verify retained-history absence](https://github.com/HalcyonXP/firefox-download-manager/commit/a4323201439ead5fb90b1de14364fa75d62ef389) |

## Evidence boundaries

Historical CI links may no longer resolve because privacy cleanup removed earlier run records. Transferred comments are dated historical evidence, not claims that current hosted CI or Firefox/release qualification passed. Current CI is [here](https://github.com/HalcyonXP/firefox-download-manager/actions); release acceptance remains #28. No application behavior, wire version, or first-party license grant changed during repository migration.
