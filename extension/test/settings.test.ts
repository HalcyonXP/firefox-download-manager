import { describe, expect, it } from "vitest";
import { nativeSettings } from "../src/native-connection";

describe("settings projection boundary", () => {
  const valid = {
    destination: "C:\\Downloads",
    default_workers: 4,
    global_concurrency: 16,
    per_host_concurrency: 8,
    retry_limit: 5,
    keep_partial_on_cancel: true,
    keep_partial_on_failure: true,
    verbose_logging: false,
  };
  it("accepts complete effective settings", () => {
    expect(nativeSettings(valid)).toEqual(valid);
  });
  it.each([
    { global_concurrency: 33 },
    { default_workers: 3 },
    { retry_limit: 21 },
    { verbose_logging: "yes" },
    { cookie: "never-here" },
  ])("rejects invalid or secret-bearing projection %j", (patch) => {
    expect(nativeSettings({ ...valid, ...patch })).toBeUndefined();
  });
});
