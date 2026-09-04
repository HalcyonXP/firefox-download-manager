import { describe, expect, it } from "vitest";

import { MAX_MESSAGE_BYTES, PROTOCOL_VERSION, isValidCorrelationId } from "../src/protocol";

describe("protocol constants", () => {
  it("matches the accepted v1 contract", () => {
    expect(PROTOCOL_VERSION).toBe(1);
    expect(MAX_MESSAGE_BYTES).toBe(1_048_576);
  });

  it("accepts only bounded non-secret correlation syntax", () => {
    expect(isValidCorrelationId("request-1.start")).toBe(true);
    expect(isValidCorrelationId("contains space")).toBe(false);
    expect(isValidCorrelationId("url?token=secret")).toBe(false);
    expect(isValidCorrelationId("a".repeat(129))).toBe(false);
  });
});
