import assert from "node:assert/strict";
import { readdir, readFile } from "node:fs/promises";

import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";

const schemaPath = "protocol/schema/v2/message.schema.json";
const examplesPath = "protocol/schema/v2/examples";
const schema = JSON.parse(await readFile(schemaPath, "utf8"));

// Conditional response schemas require properties declared by their parent object.
// That is valid Draft 2020-12 but triggers Ajv's stricter local required check.
const ajv = new Ajv2020({ allErrors: true, strict: true, strictRequired: false });
ajv.addKeyword("x-sensitive");
addFormats(ajv);
const validate = ajv.compile(schema);

const names = (await readdir(examplesPath)).filter((name) => name.endsWith(".json")).sort();
const examples = [];
for (const name of names) {
  const message = JSON.parse(await readFile(`${examplesPath}/${name}`, "utf8"));
  assert.equal(validate(message), true, `${name}: ${ajv.errorsText(validate.errors)}`);
  examples.push(message);
}

const command = structuredClone(examples.find((message) => message.kind === "command"));
assert(command, "at least one command example is required");

const hostileMessages = [
  { ...command, protocol_version: 99 },
  { ...command, command: "unknown" },
  { ...command, correlation_id: undefined },
  { ...command, unexpected: true },
  { ...command, payload: { ...command.payload, unexpected: true } },
];
for (const message of hostileMessages) {
  assert.equal(validate(message), false, "hostile protocol message was accepted");
}

const prepare = examples.find(
  (message) => message.kind === "command" && message.command === "prepare_handoff",
);
assert(prepare, "handoff prepare conformance vector is required");
for (const request_context of [null, {}, { referrer: "https://example.invalid/page" }]) {
  const invalid = structuredClone(prepare);
  invalid.payload.download.request_context = request_context;
  assert.equal(validate(invalid), false, "handoff session field was accepted");
}
const prepared = examples.find(
  (message) => message.kind === "response" && message.command === "prepare_handoff",
);
assert(prepared, "handoff response conformance vector is required");
for (const phase of ["prepared", "aborted"]) {
  const invalid = structuredClone(prepared);
  invalid.result.phase = phase;
  invalid.result.task.state = "completed";
  assert.equal(validate(invalid), false, "uncommitted handoff claimed completed output");
}

let sessionMessageCount = 0;
for (const sessionPath of process.argv.slice(2)) {
  const document = JSON.parse(await readFile(sessionPath, "utf8"));
  const messages = Array.isArray(document) ? document : [document];
  for (const [index, message] of messages.entries()) {
    assert.equal(
      validate(message),
      true,
      `${sessionPath} message ${index}: ${ajv.errorsText(validate.errors)}`,
    );
    sessionMessageCount += 1;
  }
}

const taskProperties = new Set(Object.keys(schema.$defs.task.properties));
for (const forbidden of [
  "authorization",
  "cookies",
  "credentials",
  "final_url",
  "original_url",
  "referrer",
  "url",
]) {
  assert.equal(taskProperties.has(forbidden), false, `task snapshot exposes ${forbidden}`);
}

const sessionSummary =
  sessionMessageCount === 0 ? "" : `, plus ${sessionMessageCount} live session messages`;
console.log(
  `Validated protocol schema, ${examples.length} examples, hostile cases${sessionSummary}.`,
);
