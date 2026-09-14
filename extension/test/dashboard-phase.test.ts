import { afterEach, expect, it, vi } from "vitest";
import { Dashboard } from "../src/dashboard";
import type { NativeTask } from "../src/native-connection";

// Small DOM model exercises the actual Dashboard scheduler/row/button wiring.
// It is not browser layout, accessibility or physical-input qualification.
class Element {
  children: Element[] = [];
  dataset: Record<string, string> = {};
  textContent = "";
  disabled = false;
  readonly listeners = new Map<string, () => void>();
  constructor(readonly tag: string) {}
  append(...nodes: Element[]): void {
    this.children.push(...nodes);
  }
  replaceChildren(...nodes: Element[]): void {
    this.children = nodes;
  }
  setAttribute(): void {}
  removeAttribute(): void {}
  contains(): boolean {
    return false;
  }
  addEventListener(event: string, callback: () => void): void {
    this.listeners.set(event, callback);
  }
  querySelectorAll(tag: string): Element[] {
    return this.children.flatMap((node) => [
      ...(node.tag === tag ? [node] : []),
      ...node.querySelectorAll(tag),
    ]);
  }
}

afterEach(() => vi.unstubAllGlobals());
it("renders phase-aware controls and invalidates the same-state action cache", () => {
  const frames: Array<() => void> = [];
  vi.stubGlobal("requestAnimationFrame", (callback: () => void) => frames.push(callback));
  vi.stubGlobal("document", { createElement: (tag: string) => new Element(tag) });
  const root = new Element("div");
  const act = vi.fn();
  const dashboard = new Dashboard(root as unknown as HTMLElement, act);
  const task: NativeTask = {
    task_id: "a4ac080c-862f-4ea8-b60c-06a9718b2306",
    display_name: "fixture.bin",
    destination: "owned",
    source_origin: "https://example.invalid",
    state: "queued",
    transfer_mode: "pending",
    expected_size: null,
    bytes_completed: 0,
    workers: 1,
    speed_bytes_per_second: null,
    eta_seconds: null,
    created_at: "",
    updated_at: "",
    error: null,
    handoff_phase: "prepared",
  };
  const labels = (): string[] =>
    root.querySelectorAll("button").map((button) => button.textContent);
  dashboard.update({ connected: true, tasks: [task] });
  frames.shift()!();
  expect(labels()).toEqual(["Open folder"]);
  expect(root.children[0]?.children[1]?.textContent).toBe("WAITING FOR HANDOFF");
  const committed = { ...task, handoff_phase: "committed" as const };
  dashboard.update({ connected: true, tasks: [committed] });
  frames.shift()!();
  expect(labels()).toEqual(["Pause", "Start", "Cancel", "Open folder"]);
  root.querySelectorAll("button")[1]?.listeners.get("click")!();
  expect(act).toHaveBeenCalledWith("resume", committed);
  dashboard.update({ connected: false, tasks: [committed] });
  frames.shift()!();
  expect(root.querySelectorAll("button").every((button) => button.disabled)).toBe(true);
});
