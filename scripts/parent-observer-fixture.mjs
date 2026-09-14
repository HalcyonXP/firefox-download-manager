import assert from "node:assert/strict";
import vm from "node:vm";
export const nonce = "11111111-1111-4111-8111-111111111111";
export const collector = "22222222-2222-4222-8222-222222222222";
export const other = "33333333-3333-4333-8333-333333333333";
export function observerFixture(source, topic, slot, mode = {}) {
  const observers = new Set();
  const calls = [];
  const obs = {
    addObserver(observer, name, weak) {
      assert.equal(name, topic);
      assert.equal(weak, false);
      calls.push("add");
      observers.add(observer);
      if (mode.addThrows) throw Error("modeled add refused");
    },
    removeObserver(observer, name) {
      assert.equal(name, topic);
      assert(observers.has(observer));
      calls.push("remove");
      if (mode.removeThrows) throw Error("modeled removal refused");
      if (!mode.noRemove) observers.delete(observer);
    },
    notifyObservers(subject, name, data) {
      calls.push("notify");
      if (!mode.noNotify) for (const observer of observers) observer.observe(subject, name, data);
    },
  };
  const context = vm.createContext({
    Services: { obs, appinfo: { processType: mode.processType ?? 0 } },
  });
  if (mode.foreignSlot) context[slot] = mode.foreignSlot;
  function invoke(operation, token = collector, n = nonce) {
    let result;
    let delivered = 0;
    context.arguments = [
      operation,
      n,
      token,
      (value) => {
        result = JSON.parse(JSON.stringify(value));
        delivered++;
      },
    ];
    new vm.Script(source).runInContext(context);
    assert.equal(delivered, 1);
    return result;
  }
  return {
    invoke,
    calls,
    observers,
    send: (data, subject = null, name = topic) => obs.notifyObservers(subject, name, data),
  };
}
