// One diagnostic invocation. No wakeup event, heartbeat, timer, permissions
// request, download capture, network request or caller-controlled native data.
void browser.managerParentProbe.run().catch(() => {
  // The independent controller requires exact ready/retired observations.
  // Never print raw privileged errors, fixture paths or native output.
});
