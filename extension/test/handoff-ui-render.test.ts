import { afterEach, expect, it, vi } from "vitest";
import { renderHandoffs, unlinkedReservations } from "../src/handoff-ui";
import { handoffPrompt, dispatchHandoffAction } from "../src/handoff-actions";
import type { HandoffView } from "../src/browser-handoff";

// Exercises actual rendering/callbacks, not browser layout or physical input.
class Element {
  children: Element[] = [];
  textContent = "";
  hidden = false;
  disabled = false;
  listeners = new Map<string, () => void>();
  constructor(readonly tag: string) {}
  append(...nodes: Element[]): void {
    this.children.push(...nodes);
  }
  replaceChildren(...nodes: Element[]): void {
    this.children = nodes;
  }
  addEventListener(name: string, callback: () => void): void {
    this.listeners.set(name, callback);
  }
  all(tag: string): Element[] {
    return this.children.flatMap((node) => [...(node.tag === tag ? [node] : []), ...node.all(tag)]);
  }
}
const id = "b4ac080c-862f-4ea8-b60c-06a9718b2306";
const task = { task_id: id, display_name: "owned.bin", handoff_phase: "prepared" as const };
const ready: HandoffView = { loaded: true, blocked: false, pending: [] };
afterEach(() => vi.unstubAllGlobals());
function render(
  view: HandoffView,
  phase = task.handoff_phase as "prepared" | "aborted" | "committed" | "unknown",
) {
  vi.stubGlobal("document", { createElement: (tag: string) => new Element(tag) });
  const root = new Element("div");
  const act = vi.fn();
  renderHandoffs(root as unknown as HTMLElement, view, act, [{ ...task, handoff_phase: phase }]);
  return { root, act, buttons: root.all("button") };
}
it("shows bounded unlinked candidates only with loaded unblocked history", () => {
  expect(unlinkedReservations({ ...ready, loaded: false }, [task])).toEqual([]);
  expect(unlinkedReservations({ ...ready, blocked: true }, [task])).toEqual([]);
  expect(
    unlinkedReservations({ ...ready, pending: [{ id, stage: "preparing", createdAt: 1 }] }, [task]),
  ).toEqual([]);
  expect(unlinkedReservations(ready, [task, { ...task, handoff_phase: "unknown" }])).toEqual([
    task,
  ]);
  expect(
    unlinkedReservations(
      ready,
      Array.from({ length: 40 }, (_, n) => ({ ...task, task_id: String(n) })),
    ),
  ).toHaveLength(32);
});
it("routes explicit unused-reservation cleanup, never generic Cancel/Remove", () => {
  const { root, buttons, act } = render(ready);
  expect(root.hidden).toBe(false);
  expect(buttons.map((b) => b.textContent)).toEqual(["Discard unused reservation"]);
  buttons[0]!.listeners.get("click")!();
  expect(act).toHaveBeenCalledWith(id, "discard");
  expect(render({ ...ready, loaded: false }).root.hidden).toBe(true);
});
it.each(["cancelled", "confirmed"] as const)(
  "offers an acknowledgement only for native-Aborted %s",
  (stage) => {
    const view: HandoffView = { ...ready, pending: [{ id, stage, createdAt: 1 }] };
    const { buttons, act } = render(view, "aborted");
    expect(buttons.map((b) => b.textContent)).toEqual([
      "I checked the discarded download — dismiss notice",
      "Recheck Manager",
    ]);
    buttons[0]!.listeners.get("click")!();
    expect(act).toHaveBeenCalledWith(id, "acknowledge");
    for (const phase of ["prepared", "committed", "unknown"] as const)
      expect(render(view, phase).buttons.map((b) => b.textContent)).toEqual(["Recheck Manager"]);
    expect(render({ ...view, blocked: true }, "aborted").buttons.every((b) => b.disabled)).toBe(
      true,
    );
  },
);
it("does not substitute acknowledgement for an uncertain intent choice", () => {
  const { buttons } = render(
    { ...ready, pending: [{ id, stage: "intent", createdAt: 1 }] },
    "aborted",
  );
  expect(buttons).toHaveLength(3);
  expect(buttons[0]!.disabled).toBe(true);
  expect(buttons[1]!.textContent).toBe("Keep in Firefox; discard unused reservation");
});
it("warns explicitly that cleanup neither restarts Firefox nor erases retained identity", () => {
  expect(handoffPrompt("acknowledge")).toContain("This button will not start a download");
  expect(handoffPrompt("discard")).toContain("already committed task will not be discarded");
  expect(handoffPrompt("discard")).toContain("retained task identity");
  expect(handoffPrompt("manager")).toContain("competing output");
});

it.each(["manager", "firefox", "acknowledge", "discard"] as const)(
  "requires the actual confirmation result before dispatching %s",
  (choice) => {
    const send = vi.fn();
    const confirm = vi.fn(() => false);
    dispatchHandoffAction(id, choice, confirm, send);
    expect(confirm).toHaveBeenCalledWith(handoffPrompt(choice));
    expect(send).not.toHaveBeenCalled();
    confirm.mockReturnValue(true);
    dispatchHandoffAction(id, choice, confirm, send);
    expect(send).toHaveBeenCalledExactlyOnceWith({
      action: ["acknowledge", "discard"].includes(choice) ? "handoff-cleanup" : "handoff-resolve",
      taskId: id,
      choice,
    });
  },
);
