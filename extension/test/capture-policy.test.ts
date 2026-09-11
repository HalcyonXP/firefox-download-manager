import { describe, expect, it, vi } from "vitest";
import { CapturePolicy, attachmentFilename, type CaptureRequest } from "../src/capture-policy";
import type { BrowserHandoff } from "../src/browser-handoff";
const page = "https://example.invalid/page";
const url = "https://example.invalid/file.bin";
const attachment = [{ name: "Content-Disposition", value: 'attachment; filename="file.bin"' }];
const request: CaptureRequest = {
  requestId: "request",
  url,
  tabId: 1,
  frameId: 0,
  method: "GET",
  type: "main_frame",
  incognito: false,
  cookieStoreId: "firefox-default",
  originUrl: page,
};
function setup() {
  let enabled = true;
  let now = 0;
  const capture = vi.fn<BrowserHandoff["capture"]>(async (_key, _download, eligible) =>
    eligible() ? { cancel: true } : {},
  );
  const terminal = vi.fn<BrowserHandoff["terminal"]>();
  const policy = new CapturePolicy(
    { capture, terminal },
    () => enabled,
    () => now,
  );
  const arm = (details = request) => {
    policy.click(details.url, page, details.tabId, true);
    policy.before(details);
    policy.sent(details, []);
  };
  return {
    policy,
    capture,
    terminal,
    arm,
    disable: () => {
      enabled = false;
    },
    expire: () => {
      now = 5001;
    },
  };
}
describe("supported request eligibility", () => {
  it("correlates one observed click and sends only the anonymous URL and safe name", async () => {
    const { policy, capture, arm } = setup();
    arm();
    await expect(policy.headers(request, 200, attachment)).resolves.toEqual({ cancel: true });
    expect(capture.mock.calls[0]?.slice(0, 2)).toEqual([
      "request",
      { url, suggested_filename: "file.bin" },
    ]);
    policy.before({ ...request, requestId: "other" });
    policy.sent({ ...request, requestId: "other" }, []);
    await expect(
      policy.headers({ ...request, requestId: "other" }, 200, attachment),
    ).resolves.toEqual({});
    expect(capture).toHaveBeenCalledTimes(1);
  });
  it.each([
    { method: "POST" },
    { type: "sub_frame", frameId: 1 },
    { incognito: true },
    { cookieStoreId: "firefox-container-1" },
    { cookieStoreId: "missing" },
    { originUrl: "https://example.invalid/other" },
    { url: "blob:https://example.invalid/id" },
    { url: "https://name:secret@example.invalid/file" },
  ])("refuses unsupported context %#", async (patch) => {
    const { policy, capture, arm } = setup();
    const details = { ...request, ...patch };
    arm(details);
    await expect(policy.headers(details, 200, attachment)).resolves.toEqual({});
    expect(capture).not.toHaveBeenCalled();
  });
  it.each(["Cookie", "Authorization", "Proxy-Authorization", "X-Api-Key", "Cookie "])(
    "does not replay %s context",
    async (name) => {
      const { policy, capture, arm } = setup();
      arm();
      policy.sent(request, [{ name, value: "synthetic-secret-not-to-be-replayed" }]);
      await expect(policy.headers(request, 200, attachment)).resolves.toEqual({});
      expect(capture).not.toHaveBeenCalled();
    },
  );
  it.each(
    [
      [{ name: "Set-Cookie", value: "session=fixture" }],
      [{ name: "Vary", value: "Cookie" }],
      [{ name: "Content-Encoding", value: "gzip" }],
      [{ name: "Content-Range", value: "bytes 0-1/3" }],
      [{ name: "WWW-Authenticate", value: "Basic" }],
      attachment,
    ].map((extra) => ({ extra })),
  )("refuses session/representation ambiguity %#", async ({ extra }) => {
    const { policy, capture, arm } = setup();
    arm();
    await expect(policy.headers(request, 200, [...attachment, ...extra])).resolves.toEqual({});
    expect(capture).not.toHaveBeenCalled();
  });
  it("requires sent-header evidence and an actual200 attachment", async () => {
    const { policy, capture } = setup();
    policy.click(url, page, 1, true);
    policy.before(request);
    await expect(policy.headers(request, 200, attachment)).resolves.toEqual({});
    policy.sent(request, []);
    await expect(policy.headers(request, 206, attachment)).resolves.toEqual({});
    await expect(
      policy.headers(request, 200, [{ name: "Content-Type", value: "text/html" }]),
    ).resolves.toEqual({});
    expect(capture).not.toHaveBeenCalled();
  });
  it("preserves same-origin redirect identity but requires fresh sent-header evidence", async () => {
    const { policy, capture, arm } = setup();
    arm();
    const next = { ...request, url: "https://example.invalid/final?opaque=fixture" };
    policy.redirect(request, next.url);
    policy.before(next);
    await expect(policy.headers(next, 200, attachment)).resolves.toEqual({});
    policy.sent(next, []);
    await expect(policy.headers(next, 200, attachment)).resolves.toEqual({ cancel: true });
    expect(capture.mock.calls[0]?.[1].url).toBe(next.url);
  });
  it("does not erase credentials observed earlier in an otherwise same-origin redirect chain", async () => {
    const { policy, capture, arm } = setup();
    arm();
    policy.sent(request, [{ name: "Cookie", value: "synthetic-session" }]);
    const next = { ...request, url: "https://example.invalid/final" };
    policy.redirect(request, next.url);
    policy.before(next);
    policy.sent(next, []);
    await expect(policy.headers(next, 200, attachment)).resolves.toEqual({});
    expect(capture).not.toHaveBeenCalled();
  });
  it("refuses cross-origin redirects even if later callbacks return to the initial origin", async () => {
    const { policy, capture, arm } = setup();
    arm();
    policy.redirect(request, "https://other.example.invalid/file");
    policy.before(request);
    policy.sent(request, []);
    await expect(policy.headers(request, 200, attachment)).resolves.toEqual({});
    expect(capture).not.toHaveBeenCalled();
  });
  it("does not borrow a click delivered after the request was already created", async () => {
    const { policy, capture } = setup();
    policy.before(request);
    policy.click(url, page, 1, true);
    policy.sent(request, []);
    await expect(policy.headers(request, 200, attachment)).resolves.toEqual({});
    expect(capture).not.toHaveBeenCalled();
  });
  it("invalidates ambiguous rapid clicks, stale clicks and changed-tab headers", async () => {
    for (const kind of ["rapid", "expired", "tab", "disabled", "untrusted"]) {
      const { policy, capture, arm, expire, disable } = setup();
      if (kind === "untrusted") {
        policy.click(url, page, 1, false);
        policy.before(request);
        policy.sent(request, []);
      } else arm();
      if (kind === "rapid") policy.click(url, page, 1, true);
      if (kind === "expired") expire();
      if (kind === "disabled") disable();
      await expect(
        policy.headers(kind === "tab" ? { ...request, tabId: 2 } : request, 200, attachment),
      ).resolves.toEqual({});
      expect(capture).not.toHaveBeenCalled();
    }
  });
  it("keeps the eligibility callback live throughout native preparation", async () => {
    const { policy, capture, arm } = setup();
    arm();
    await policy.headers(request, 200, attachment);
    const eligible = capture.mock.calls[0]![2];
    expect(eligible()).toBe(true);
    policy.click(url, page, 1, true);
    expect(eligible()).toBe(false);
  });
  it.each(["original", "later"])(
    "does not let later metadata replace the prepared URL (%s terminal)",
    async (which) => {
      const { policy, terminal, arm } = setup();
      arm();
      await policy.headers(request, 200, attachment);
      const later = { ...request, url: "https://example.invalid/other" };
      policy.before(later);
      policy.sent(later, []);
      policy.terminal(which === "original" ? request : later, "NS_ERROR_ABORT");
      expect(terminal).toHaveBeenCalledWith("request", undefined);
    },
  );
  it("does not classify a terminal event for a different URL as proof of cancellation", async () => {
    const { policy, terminal, arm } = setup();
    arm();
    await policy.headers(request, 200, attachment);
    policy.terminal({ ...request, url: "https://example.invalid/other" }, "NS_ERROR_ABORT");
    expect(terminal).toHaveBeenCalledWith("request", undefined);
  });
  it("does not classify a terminal event from another tab as proof of cancellation", async () => {
    const { policy, terminal, arm } = setup();
    arm();
    await policy.headers(request, 200, attachment);
    policy.terminal({ ...request, tabId: 2 }, "NS_ERROR_ABORT");
    expect(terminal).toHaveBeenCalledWith("request", undefined);
  });
});
it("uses Windows-safe attachment names without treating server metadata as paths", () => {
  expect(
    attachmentFilename(
      [{ name: "Content-Disposition", value: "attachment; filename*=UTF-8''model%20file.gguf" }],
      url,
    ),
  ).toBe("model file.gguf");
  expect(
    attachmentFilename(
      [{ name: "Content-Disposition", value: 'attachment; filename="../CON"' }],
      url,
    ),
  ).toBe(".._CON");
  expect(
    attachmentFilename(
      [{ name: "Content-Disposition", value: "attachment; filename*=UTF-8''%ff" }],
      url,
    ),
  ).toBeUndefined();
});
