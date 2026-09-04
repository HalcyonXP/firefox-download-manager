import assert from "node:assert/strict";
import { readdir, readFile } from "node:fs/promises";

import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";

const schemaPath = "protocol/schema/v1/message.schema.json";
const examplesPath = "protocol/schema/v1/examples";
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
  { ...command, protocol_version: 2 },
  { ...command, command: "unknown" },
  { ...command, correlation_id: undefined },
  { ...command, unexpected: true },
  { ...command, payload: { ...command.payload, unexpected: true } },
];
for (const message of hostileMessages) {
  assert.equal(validate(message), false, "hostile protocol message was accepted");
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
