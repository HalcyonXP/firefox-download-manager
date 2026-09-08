import assert from "node:assert/strict";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { originalCommitAbsent, outsideCheckout } from "./publication-policy.mjs";

test("comparison inputs must be outside the checkout, not merely dot-prefixed", () => {
  const checkout = join(tmpdir(), "synthetic-checkout");
  assert.equal(outsideCheckout(checkout, join(checkout, "private.json")), false);
  assert.equal(outsideCheckout(checkout, join(checkout, "..private.json")), false);
  assert.equal(outsideCheckout(checkout, checkout), false);
  assert.equal(outsideCheckout(checkout, join(tmpdir(), "synthetic-audit", "private.json")), true);
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
