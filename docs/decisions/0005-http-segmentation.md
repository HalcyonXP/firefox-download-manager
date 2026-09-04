# ADR-0005: Segment only after strict ranged-GET validation

- Status: Accepted
- Date: 2026-09-04
- Reversibility: Scheduling heuristics are reversible; validation rules are not

## Context

`HEAD` and `Accept-Ranges` are unreliable indicators of actual range behavior. Servers can ignore ranges, return malformed `Content-Range` values, transform content, mutate resources, or provide inconsistent validators. Optimistically merging such responses can silently corrupt output.

## Decision

Probe with a small ranged `GET` using `Accept-Encoding: identity`. Enable segmentation only after a valid `206` response whose `Content-Range`, body bounds, total size, final URL, and validators are internally consistent. Apply the same validation to every worker response before writing.

Assignments are disjoint and cover only known-size resources. Per-task workers are 1, 2, 4, or 8 (default 4, cap 8), additionally constrained by per-host and global limits. Unsupported behavior falls back to one stream only before segmented output would be mixed; otherwise the task revalidates or fails explicitly.

## Rejected alternatives

- **Trust `HEAD` or `Accept-Ranges`:** advisory metadata does not prove response behavior.
- **Accept `200` for a range and slice locally:** wastes bandwidth and can mix identities.
- **Fixed chunks merged afterward:** unnecessary merge phase and weaker direct-write invariants.
- **Unbounded/dynamic connection racing:** impolite, unpredictable, and difficult to recover.
- **Multi-origin mirrors:** resource-equivalence and credential concerns are outside scope.

## Consequences

The adversarial server is foundational test infrastructure. Resource mutation, redirects, encoding, retries, and tail splitting must preserve the same assignment and identity invariants.
