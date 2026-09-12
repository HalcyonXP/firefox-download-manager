// One attempt. Polling observes status; it never resubmits the service query.
(async () => {
  const output = document.getElementById("receipt");
  try {
    let receipt = await browser.managerProtection.start();
    for (let count = 0; count < 150 && receipt.stage === "pending"; count++) {
      output.textContent = JSON.stringify(receipt);
      await new Promise((resolve) => setTimeout(resolve, 100));
      receipt = await browser.managerProtection.snapshot();
    }
    output.textContent = JSON.stringify(receipt);
  } catch {
    output.textContent = "unavailable";
  }
})();
