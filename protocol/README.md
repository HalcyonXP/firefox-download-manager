# Protocol sources

The Native Messaging wire contract is documented in [`docs/PROTOCOL.md`](../docs/PROTOCOL.md).

- `schema/v1/message.schema.json` is the normative JSON Schema Draft 2020-12 contract.
- `schema/v1/examples/` contains non-sensitive valid messages used as conformance vectors.
- `ERROR_CODES.md` is the stable v1 error registry.

Downloaded file bytes never cross this protocol. Consumers must validate messages, enforce the separate 1 MiB framing limit, and avoid logging raw envelopes.
