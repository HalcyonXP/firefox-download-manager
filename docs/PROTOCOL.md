# Native Messaging protocol v1

Status: accepted

Protocol version: `1`

Schema: [`protocol/schema/v1/message.schema.json`](../protocol/schema/v1/message.schema.json)

## Purpose and boundary

Protocol v1 is the only control boundary between the Firefox extension and native helper. It transports commands, responses, snapshots, and events; downloaded file bodies never cross it. The helper remains authoritative and the extension can reconstruct its entire UI from snapshots after any page, background-context, connection, or browser restart.

Every payload is untrusted even when Firefox launched the configured host. Both peers validate messages before acting.

## Native Messaging framing

Each message is one UTF-8 JSON object preceded by Firefox Native Messaging's four-byte unsigned length prefix. On Windows the prefix is little-endian. The project imposes a **1,048,576-byte maximum JSON body in both directions**, regardless of a platform's larger theoretical limit.

Readers must handle partial reads, distinguish clean EOF before a prefix from truncation, reject invalid UTF-8/JSON and non-object roots, and read exactly the declared length. A length over the project limit is rejected before allocation. Writers serialize one complete object, enforce the same limit, then write the prefix and body completely.

No secret may be included in a framing or parse error.

## Envelope

Every message carries:

| Field | Meaning |
| --- | --- |
| `protocol_version` | Integer major version; exactly `1` in this contract |
| `correlation_id` | Non-secret caller-generated identifier, 1–128 restricted ASCII characters |
| `kind` | `command`, `response`, or `event` |

Commands and their terminal responses use the same correlation ID. An event caused directly by a command may use that command's correlation ID; unsolicited events use a new helper-generated ID. Correlation IDs must never contain a URL, filename, cookie, token, or other user data.

JSON integers representing byte counts, cursors, rates, or sequence numbers are capped at `9,007,199,254,740,991` so JavaScript can represent them exactly. Unknown properties are rejected throughout v1 rather than silently ignored.

## Startup and version negotiation

After connecting, the extension sends `hello` before any operational command. It lists protocol major versions it supports. The v1 helper selects version `1`, reports its application version, maximum message size, and implemented capabilities.

If the envelope version is unsupported, the helper performs only bounded extraction of a valid correlation ID and returns a v1 `protocol` error with `PROTOCOL_UNSUPPORTED_VERSION` when possible, then closes the connection. If no safe correlation ID can be recovered, it generates one. An unknown command receives `PROTOCOL_UNKNOWN_COMMAND`; malformed known commands receive `PROTOCOL_INVALID_MESSAGE`. No operation occurs after any of these errors.

A peer must not infer support from application version strings. Optional behavior is enabled only by the negotiated protocol and advertised capability. Authentication and SHA-256 fields exist in v1, but the helper advertises and accepts them only after their implementation issues are complete.

## Commands

Operational commands are serialized by the helper per task. A command receives exactly one terminal response, though state/progress events may appear before or after that response.

| Command | Purpose | Successful result |
| --- | --- | --- |
| `hello` | Negotiate protocol and capabilities | Negotiation details |
| `add` | Validate and create a task | Full task |
| `pause` | Reach a safe paused checkpoint | Full task |
| `resume` | Revalidate and request only missing work | Full task |
| `cancel` | Stop work using explicit `keep`/`delete` partial policy | Full task |
| `remove` | Remove eligible task history and optionally retained partial state | Removed task ID |
| `list` | Return a bounded page from an authoritative snapshot | Snapshot page |
| `get` | Return one full task | Full task |
| `update_settings` | Validate and atomically apply a non-empty patch | Complete effective settings |

`add.url` must be an absolute HTTP(S) URL without URL user-info. Schema pattern checks are only preliminary; the helper performs semantic URL parsing. Destination, suggested filename, request context, checksum, and worker count are likewise revalidated by their consuming subsystem.

`pause`, `resume`, `cancel`, and `remove` are idempotent only where the task state definition explicitly permits it. An invalid transition returns `INVALID_TASK_STATE`, never an optimistic success.

## Responses and stable errors

A response repeats the request's `command`, except bootstrap decoding errors use `command: "protocol"`. It has `ok: true` with a command-specific `result`, or `ok: false` with an `error`; it can never contain both.

`error.code` is stable protocol data. `display_message` is bounded explanatory text intended for people and may change without a protocol version. Consumers must branch on the code, not parse display text. Error context permits only a task ID, HTTP status, or retry-after seconds so arbitrary server text and URLs cannot leak through errors.

The complete registry and retry guidance are in [`protocol/ERROR_CODES.md`](../protocol/ERROR_CODES.md). Removing or changing the meaning of a code requires a new major protocol version.

## Events, ordering, and coalescing

Events contain a connection-local monotonically increasing `sequence` and `emitted_at`. The sequence detects dropped/reordered events but is not persisted as task truth and may restart after reconnection.

- `state_changed`: a full task plus its previous state.
- `progress`: a replaceable sample of counters, smoothed rate, ETA, and active workers.
- `warning`: a stable warning code that does not itself make the task terminal.
- `completed`: the full task after validation and final-file promotion.
- `failed`: the full failed task and stable terminal error.
- `snapshot`: one bounded page of authoritative tasks.

Progress is deliberately coalescible. The helper may replace unsent progress for the same task with a newer sample and may rate-limit samples. The extension treats each sample as an absolute value, never a delta. State changes, warnings, completion, and failure are not discarded as progress noise.

On a sequence gap, reconnect, dashboard opening, or uncertain state, the extension requests `list` and rebuilds from snapshot pages instead of guessing.

## Snapshot consistency and pagination

A snapshot page carries `snapshot_id`, zero-based `page_index`, at most 200 tasks, `next_cursor`, and `complete`.

- All pages in one reconstruction share the same snapshot ID.
- The extension starts with `list.cursor: null` and follows only helper-issued cursors.
- `complete: true` requires `next_cursor: null`.
- Until the complete page arrives, the extension does not present the partial collection as an authoritative replacement.
- Invalid, duplicated, expired, out-of-order, or mixed-snapshot cursors cause the extension to discard the assembly and restart.
- Events received during assembly are applied only after the complete snapshot, ordered by sequence; uncertainty triggers another snapshot.

The helper may emit the same page form as a `snapshot` event after connection. Pagination ensures snapshots stay under the framing limit.

## Task representation

Task snapshots intentionally contain enough display/control state but not request secrets. They include an opaque UUID, safe display name, destination, source origin without path/query/user-info, lifecycle state, transfer mode, sizes, worker/rate/ETA values, timestamps, and a bounded stable error.

The native helper may persist additional internal fields—such as exact URLs, validators, ranges, and partial paths—that are not ordinary protocol snapshot data. Internal representation is not part of the wire contract.

## Sensitive fields

Schema fields marked `x-sensitive` require special handling. These include URL, referrer, cookie values, and authorization values.

- Sensitive command data is consumed only by the operation that needs it.
- Credentials are memory-only and are never echoed in responses, snapshots, events, errors, persisted task state, panic messages, or routine logs.
- Exact URLs are not returned in ordinary task snapshots; `source_origin` contains no user-info, path, query, or fragment.
- Debug serialization of raw envelopes is prohibited.
- Redaction occurs before structured data reaches a log formatter.
- Cross-origin redirects strip credentials unless an explicit authenticated-download policy permits transfer.

The credential shape reserves the reviewed boundary for issue #15; implementations must reject it until the `authenticated_requests` capability is advertised.

## Compatibility rules

Protocol versions are positive integer majors. V1 uses strict schemas and rejects unknown fields, so changing required fields, accepted values, command/event names, or object shape requires a new major version. Documentation-only clarifications and implementation bug fixes that preserve the schema do not.

When multiple majors exist, the helper selects the highest mutually supported version from `hello.supported_versions`. A helper may implement more than one major internally, but each connection uses exactly one after negotiation. Unknown majors and commands always fail closed.

Application versions and persisted-state schema versions are independent of the wire protocol version.

## Examples and validation

Non-sensitive examples are under [`protocol/schema/v1/examples`](../protocol/schema/v1/examples). They are normative test vectors for shape, not promises that every represented capability is already implemented. The schema declares JSON Schema Draft 2020-12 and validates each complete framed-body object independently.
