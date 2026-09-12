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
    expect(actionsFor("validating").map((item) => item.label)).toEqual(["Cancel", "Open folder"]);
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

it("does not mistake download rate/ETA for validation progress", () => {
  const text = progressText({
    state: "validating",
    bytes_completed: 100,
    expected_size: 100,
    eta_seconds: 0,
    speed_bytes_per_second: 1000,
  } as NativeTask);
  expect(text).toContain("final name withheld");
  expect(text).not.toContain("/s");
  expect(text).not.toContain("0s remaining");
});

it("does not offer ordinary controls for an uncommitted handoff", () => {
  expect(actionsFor("queued", "prepared").map((item) => item.action)).toEqual(["open_folder"]);
  expect(actionsFor("cancelled", "aborted").map((item) => item.action)).toEqual(["open_folder"]);
  expect(actionsFor("completed", "committed").map((item) => item.action)).toEqual(["open_folder"]);
  expect(actionsFor("failed", "unknown").map((item) => item.action)).toEqual(["open_folder"]);
});

it("retains normal transfer controls after commit without promising removable history", () => {
  expect(actionsFor("downloading", "committed").map((item) => item.action)).toEqual([
    "pause",
    "cancel",
    "open_folder",
  ]);
  expect(actionsFor("failed", "committed").map((item) => item.action)).toEqual([
    "resume",
    "open_folder",
  ]);
  expect(actionsFor("cancelled", null).map((item) => item.action)).toContain("remove");
});
it("does not display transfer progress for unused or unknown reservations", () => {
  expect(progressText({ handoff_phase: "prepared" } as NativeTask)).toContain(
    "no Manager transfer",
  );
  expect(progressText({ handoff_phase: "aborted" } as NativeTask)).toContain("identity retained");
  expect(progressText({ handoff_phase: "unknown" } as NativeTask)).toContain("older helper");
});
