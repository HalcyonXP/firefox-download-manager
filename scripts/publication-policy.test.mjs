import assert from "node:assert/strict";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  originalCommitAbsent,
  outsideCheckout,
  platformRecordCounts,
} from "./publication-policy.mjs";

test("comparison inputs must be outside the checkout, not merely dot-prefixed", () => {
  const checkout = join(tmpdir(), "synthetic-checkout");
  assert.equal(outsideCheckout(checkout, join(checkout, "private.json")), false);
  assert.equal(outsideCheckout(checkout, join(checkout, "..private.json")), false);
  assert.equal(outsideCheckout(checkout, checkout), false);
  assert.equal(outsideCheckout(checkout, join(tmpdir(), "synthetic-audit", "private.json")), true);
});

test("independent pull metadata and totals cover an issues-only response without double counting", () => {
  const issue = { number: 1 };
  const pull = { number: 2 };
  const thinPull = { number: 2, pull_request: {} };
  const totals = { issues: 1, pullRequests: 1 };
  const expected = { issues: 1, pullRequests: 1, issueAndPRRecords: 2 };
  assert.deepEqual(platformRecordCounts([issue], [pull], totals), expected);
  assert.deepEqual(platformRecordCounts([issue, thinPull], [pull], totals), expected);
  for (const [issues, pulls, counts] of [
    [[issue], [], totals],
    [[], [pull], totals],
    [[issue, issue], [pull], totals],
    [[issue], [pull, pull], totals],
    [[issue], [{ number: 1 }], totals],
    [[issue, { number: 3, pull_request: {} }], [pull], totals],
    [[{ number: true }], [pull], totals],
    [[{ number: 0 }], [pull], totals],
    [null, [pull], totals],
    [[issue], null, totals],
    [[issue], [pull], undefined],
    [[issue], [pull], { issues: "1", pullRequests: 1 }],
    [[issue], [pull], { issues: 1, pullRequests: 2 }],
  ]) {
    assert.throws(() => platformRecordCounts(issues, pulls, counts), /coverage is incomplete/u);
  }
});

test("only an exact missing-commit response proves a tested original is absent", () => {
  const commit = "a".repeat(40);
  const response = { status: 422, body: { message: `No commit found for SHA: ${commit}` } };
  assert.equal(originalCommitAbsent(response, commit), true);
  for (const status of [0, 200, 401, 403, 404, 429, 500]) {
    assert.equal(originalCommitAbsent({ ...response, status }, commit), false);
  }
  assert.equal(
    originalCommitAbsent({ status: 422, body: { message: "Repository unavailable" } }, commit),
    false,
  );
  assert.equal(originalCommitAbsent(response, "b".repeat(40)), false);
  assert.equal(originalCommitAbsent(response, "not-a-commit"), false);
});
