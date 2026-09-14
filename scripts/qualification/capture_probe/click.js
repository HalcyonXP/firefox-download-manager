/* Test-only click observation; never prevents navigation or reads page bodies. */
document.addEventListener(
  "click",
  (event) => {
    const link = event.target instanceof Element ? event.target.closest("a[href]") : null;
    if (
      !link ||
      event.button !== 0 ||
      event.ctrlKey ||
      event.altKey ||
      event.shiftKey ||
      event.metaKey
    )
      return;
    void browser.runtime.sendMessage({
      action: "click",
      target: link.href,
      trusted: event.isTrusted,
    });
  },
  { capture: true, passive: true },
);
