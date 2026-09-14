# Protocol sources

The Native Messaging wire contract is documented in [`docs/PROTOCOL.md`](../docs/PROTOCOL.md).

- `schema/v2/message.schema.json` is the normative JSON Schema Draft 2020-12 contract.
- `schema/v2/examples/` contains non-sensitive valid messages used as conformance vectors.
- `ERROR_CODES.md` and `schema/v1/` are archived v1 references; current error codes are defined by the v2 schema and typed Rust registry.

Downloaded file bytes never cross this protocol. Consumers must validate messages, enforce the separate 1 MiB framing limit, and avoid logging raw envelopes. `crates/protocol` implements duplicate-rejecting typed command decoding and complete-read/write framing; `crates/native-host` maps the task engine to v2 responses/events; and `extension/src/native-connection.ts` validates hello/snapshot/event input before replacing extension display state.

The opt-in companion bridge implements `prepared_handoff`; see [`docs/NATIVE_HANDOFF.md`](../docs/NATIVE_HANDOFF.md). Legacy stdio does not advertise it, and automatic browser capture is not selected.
