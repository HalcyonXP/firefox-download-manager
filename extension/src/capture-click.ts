// Passive click observation only. No page bodies, form values or navigation changes.
import { directUrl } from "./creation";

document.addEventListener(
  "click",
  (event: MouseEvent) => {
    if (
      !event.isTrusted ||
      event.defaultPrevented ||
      event.button !== 0 ||
      event.ctrlKey ||
      event.altKey ||
      event.shiftKey ||
      event.metaKey
    )
      return;
    const link = event.target instanceof Element ? event.target.closest("a[href]") : null;
    if (!(link instanceof HTMLAnchorElement) || (link.target && link.target !== "_self")) return;
    try {
      const target = directUrl(link.href);
      target.hash = "";
      void browser.runtime
        .sendMessage({ action: "ordinary-download-click", target: target.href, trusted: true })
        .catch(() => {});
    } catch {
      /* Unsupported URLs stay entirely with Firefox. */
    }
  },
  { capture: true, passive: true },
);
