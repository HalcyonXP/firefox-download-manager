import assert from "node:assert/strict";
import test from "node:test";
import {
  isLocalInstructionPath,
  repository,
  repositoryFindings,
  repositoryUrl,
} from "./repository-policy.mjs";

test("local instruction files are refused at every directory depth and case", () => {
  for (const path of ["AGENTS.md", "agents.md", "docs/AGENTS.md", "a/b/Agents.MD"]) {
    assert.equal(isLocalInstructionPath(path), true);
  }
  for (const path of ["README.md", "docs/DEVELOPMENT.md", "agents.mdx", "my-agents.md"]) {
    assert.equal(isLocalInstructionPath(path), false);
  }
});

test("canonical HTTPS, SSH, issue, and schema references remain valid", () => {
  for (const text of [
    repository,
    `${repositoryUrl}.git`,
    `git@github.com:${repository}.git`,
    `${repositoryUrl}/issues/23`,
    `${repositoryUrl}/protocol/schema/v2/message.schema.json`,
  ]) {
    assert.deepEqual(repositoryFindings(text), []);
  }
});

test("retired namespace is rejected regardless of link form or case, without echoing it", () => {
  const retired = ["HalcyonXP", "download-manager"].join("/");
  for (const text of [
    `https://github.com/${retired}/issues/15`,
    `git@github.com:${retired}.git`,
    `${retired}#41`,
    retired.toUpperCase(),
  ]) {
    const findings = repositoryFindings(`Heading\n${text}`);
    assert.deepEqual(findings, [{ line: 2, category: "retired-repository-reference" }]);
    assert.equal(JSON.stringify(findings).includes(retired), false);
  }
});
