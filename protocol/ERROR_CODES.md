# Protocol v1 error registry

Codes are stable machine-readable values. Display text is explanatory and unstable. `Retryable` means a caller may retry only after the stated condition; it never authorizes an unbounded automatic loop.

| Code | Meaning | Retryable |
| --- | --- | --- |
| `PROTOCOL_UNSUPPORTED_VERSION` | No supported wire major was selected | After installing compatible components |
| `PROTOCOL_UNKNOWN_COMMAND` | Command name is not defined by the selected major | No |
| `PROTOCOL_INVALID_MESSAGE` | JSON shape, value, or semantic validation failed | After correcting the request |
| `PROTOCOL_MESSAGE_TOO_LARGE` | Framed body exceeds 1 MiB | After reducing it |
| `INVALID_URL` | URL parsing or policy validation failed | After correction |
| `UNSUPPORTED_SCHEME` | Target is not direct HTTP(S) | No for the same URL |
| `INVALID_DESTINATION` | Destination is malformed, unsafe, or unavailable | After correction |
| `INVALID_FILENAME` | Suggested or resolved filename is unsafe | After correction |
| `INVALID_SETTINGS` | Settings or patch violate bounds/policy | After correction |
| `TASK_NOT_FOUND` | Task ID does not exist | No for that ID |
| `INVALID_TASK_STATE` | Command is invalid for the current task state | After state changes |
| `AUTH_REQUIRED` | Server requires eligible authentication | After user authorization |
| `AUTH_EXPIRED` | Supplied session material is no longer accepted | After refreshing authorization |
| `REDIRECT_REJECTED` | Redirect violated count, scheme, origin, or credential policy | No without policy/input change |
| `PROBE_FAILED` | Resource could not be characterized safely | According to bounded retry policy |
| `RANGE_UNSUPPORTED` | Safe segmentation is unavailable | Helper may use safe single stream |
| `RANGE_RESPONSE_INVALID` | Ranged response did not match its assignment | Only after revalidation/policy backoff |
| `RESOURCE_CHANGED` | Size, validator, or identity conflicts with accepted bytes | Only as a new/restarted task |
| `HTTP_STATUS` | Terminal or currently unhandled HTTP status | According to status and retry policy |
| `RETRY_EXHAUSTED` | Bounded retry budget was consumed | By explicit user retry |
| `STORAGE_ERROR` | Filesystem operation failed without a more specific code | After resolving the condition |
| `DISK_FULL` | Destination cannot accept required bytes | After freeing/changing storage |
| `ACCESS_DENIED` | Account lacks required filesystem access | After permission/destination change |
| `FILE_LOCKED` | Sharing or locking prevents safe access/promotion | After releasing the lock |
| `FILE_EXISTS` | Collision-free final name could not be selected/created | After destination/name change |
| `STATE_CORRUPT` | Persisted metadata/partial state is invalid or incompatible | Not automatically |
| `CHECKSUM_MISMATCH` | User-supplied digest does not match completed bytes | Only as a new/restarted task |
| `CANCELLED` | Operation ended due to explicit cancellation | By explicit new action |
| `INTERNAL_ERROR` | Bounded unexpected helper failure | By explicit retry after diagnosis |

Adding a code is a protocol-shape change because v1 uses a strict enum and therefore requires a new protocol major. Implementations may initially map an unforeseen internal condition to `INTERNAL_ERROR` without exposing sensitive details.
