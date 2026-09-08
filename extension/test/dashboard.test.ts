import { describe, expect, it } from "vitest";
import { actionsFor, bytes, progressText } from "../src/dashboard";
import type { NativeTask } from "../src/native-connection";

describe("dashboard projection", () => {
  it("offers state-appropriate controls without optimistic publication", () => {
    expect(actionsFor("failed").map((item) => item.label)).toEqual([
      "Retry",
      "Remove task & partial",
      "Open folder",
    ]);
    expect(actionsFor("completed").map((item) => item.label)).toEqual([
      "Remove history",
      "Open folder",
    ]);
    expect(actionsFor("promoting").map((item) => item.label)).toEqual(["Open folder"]);
    expect(actionsFor("paused").map((item) => item.label)).toContain("Resume");
    expect(actionsFor("queued").map((item) => item.label)).toContain("Start");
  });
  it("does not invent sizes, speed or ETA", () => {
    expect(bytes(null)).toBe("Unknown size");
    expect(bytes(2 ** 30)).toBe("1.0 GiB");
    expect(
      progressText({
        bytes_completed: 1024,
        expected_size: null,
        eta_seconds: null,
        speed_bytes_per_second: null,
        workers: 4,
        transfer_mode: "single",
      } as NativeTask),
    ).toContain("ETA unknown");
  });
});
