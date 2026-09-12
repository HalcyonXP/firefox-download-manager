import { expect, it } from "vitest";
import { handoffLabel, handoffStageText, canContinueInManager } from "../src/handoff-ui";
const id = "b4ac080c-862f-4ea8-b60c-06a9718b2306";
it("labels pending intent from the matching native task, not guessed or persisted URL data", () => {
  const entry = { id, createdAt: 1, stage: "intent" as const };
  const label = handoffLabel(entry, [{ task_id: id, display_name: "owned.gguf" }]);
  expect(label).toContain("owned.gguf");
  expect(label).toContain(id);
  expect(label).toContain("uncertain");
  expect(handoffLabel(entry, [{ task_id: "other", display_name: "wrong.gguf" }])).toContain(
    "Task details unavailable",
  );
  expect(handoffLabel(entry, [])).not.toContain("owned.gguf");
});
it("does not silently equate explicit confirmation with observed cancellation", () => {
  expect(handoffStageText.cancelled).toContain("cancellation was observed");
  expect(handoffStageText.confirmed).toContain("explicitly confirmed");
  expect(handoffStageText.intent).toContain("uncertain");
  expect(handoffStageText.fallback).toContain("Firefox retained control");
});

it("requires a known prepared native reservation for explicit continuation", () => {
  const task = { task_id: id, display_name: "fixture.bin" };
  expect(canContinueInManager(id, [task])).toBe(false);
  expect(canContinueInManager(id, [{ ...task, handoff_phase: "prepared" }])).toBe(true);
  for (const phase of [null, "unknown", "committed", "aborted"] as const)
    expect(canContinueInManager(id, [{ ...task, handoff_phase: phase }])).toBe(false);
  expect(canContinueInManager("other", [{ ...task, handoff_phase: "prepared" }])).toBe(false);
});
