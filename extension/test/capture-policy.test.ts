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
function setup(crossOriginRedirects = false) {
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
    { crossOriginRedirects },
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
    policy.redirect(request, next.url, 302, [{ name: "Location", value: next.url }]);
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
    policy.redirect(request, next.url, 302, [{ name: "Location", value: next.url }]);
    policy.before(next);
    policy.sent(next, []);
    await expect(policy.headers(next, 200, attachment)).resolves.toEqual({});
    expect(capture).not.toHaveBeenCalled();
  });
  it("refuses cross-origin redirects even if later callbacks return to the initial origin", async () => {
    const { policy, capture, arm } = setup();
    arm();
    policy.redirect(request, "https://other.example.invalid/file", 302, [
      { name: "Location", value: "https://other.example.invalid/file" },
    ]);
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

it("does not follow a different URL than the observed redirect target", async () => {
  const { policy, capture, arm } = setup();
  arm();
  const declared = "https://example.invalid/declared";
  policy.redirect(request, declared, 302, [{ name: "Location", value: declared }]);
  const other = { ...request, url: "https://example.invalid/replaced" };
  policy.before(other);
  policy.sent(other, []);
  await expect(policy.headers(other, 200, attachment)).resolves.toEqual({});
  expect(capture).not.toHaveBeenCalled();
});

describe("opt-in anonymous cross-origin chains", () => {
  const cdn = "https://cdn.example.invalid/download?opaque=synthetic";
  const next = { ...request, url: cdn };
  const location = [{ name: "Location", value: cdn }];
  it("binds an observed redirect and commits only the matching final terminal", async () => {
    const { policy, capture, terminal, arm } = setup(true);
    arm();
    policy.redirect(request, cdn, 302, location);
    policy.before(next);
    policy.sent(next, []);
    await expect(policy.headers(next, 200, attachment)).resolves.toEqual({ cancel: true });
    expect(capture.mock.calls[0]?.[1]).toEqual({ url: cdn, suggested_filename: "file.bin" });
    policy.terminal(next, "NS_ERROR_ABORT");
    expect(terminal).toHaveBeenCalledWith("request", "NS_ERROR_ABORT");
  });
  it("does not select cross-origin behavior by default", async () => {
    const { policy, capture, arm } = setup();
    arm();
    policy.redirect(request, cdn, 302, location);
    policy.before(next);
    policy.sent(next, []);
    await expect(policy.headers(next, 200, attachment)).resolves.toEqual({});
    expect(capture).not.toHaveBeenCalled();
  });
  it.each([
    "missing-transition",
    "missing-headers",
    "duplicate-location",
    "different-location",
    "cookie",
    "challenge",
    "status",
    "downgrade",
    "credentials",
    "later-cookie",
    "private",
    "container",
    "tab",
    "document",
    "double-transition",
  ])("refuses %s without native preparation", async (variant) => {
    const { policy, capture, arm } = setup(true);
    arm();
    if (variant === "credentials")
      policy.sent(request, [{ name: "Authorization", value: "synthetic" }]);
    const target = variant === "downgrade" ? cdn.replace("https:", "http:") : cdn;
    let headers: { name: string; value: string }[] | undefined = [
      { name: "Location", value: target },
    ];
    if (variant === "missing-headers") headers = undefined;
    if (variant === "duplicate-location") headers!.push(...location);
    if (variant === "different-location") headers = [{ name: "Location", value: url }];
    if (variant === "cookie") headers!.push({ name: "Set-Cookie", value: "synthetic=1" });
    if (variant === "challenge") headers!.push({ name: "WWW-Authenticate", value: "Basic" });
    if (variant !== "missing-transition")
      policy.redirect(request, target, variant === "status" ? 200 : 302, headers);
    if (variant === "double-transition") policy.redirect(request, target, 302, headers);
    const details = {
      ...next,
      url: target,
      ...(variant === "private" ? { incognito: true } : {}),
      ...(variant === "container" ? { cookieStoreId: "firefox-container-1" } : {}),
      ...(variant === "tab" ? { tabId: 2 } : {}),
      ...(variant === "document" ? { originUrl: "https://other.example.invalid/page" } : {}),
    };
    policy.before(details);
    policy.sent(
      details,
      variant === "later-cookie" ? [{ name: "Cookie", value: "synthetic=1" }] : [],
    );
    await expect(policy.headers(details, 200, attachment)).resolves.toEqual({});
    expect(capture).not.toHaveBeenCalled();
  });
  it("bounds transitions and never permits an HTTPS downgrade after an upgrade", async () => {
    for (const count of [8, 9]) {
      const { policy, arm } = setup(true);
      arm();
      let details = request;
      for (let i = 0; i < count; i++) {
        const target = `https://cdn${i}.example.invalid/file`;
        policy.redirect(details, target, 307, [{ name: "Location", value: target }]);
        details = { ...details, url: target };
        policy.before(details);
        policy.sent(details, []);
      }
      await expect(policy.headers(details, 200, attachment)).resolves.toEqual(
        count === 8 ? { cancel: true } : {},
      );
    }
    const { policy, arm } = setup(true);
    const start = { ...request, url: "http://example.invalid/file" };
    arm(start);
    policy.redirect(start, url, 301, [{ name: "Location", value: url }]);
    policy.before(request);
    policy.sent(request, []);
    policy.redirect(request, start.url, 302, [{ name: "Location", value: start.url }]);
    policy.before(start);
    policy.sent(start, []);
    await expect(policy.headers(start, 200, attachment)).resolves.toEqual({});
  });
});

it("requires every redirect origin to remain inside the caller's independently owned scope", async () => {
  const capture = vi.fn<BrowserHandoff["capture"]>(async () => ({ cancel: true }));
  const policy = new CapturePolicy(
    { capture, terminal: vi.fn() },
    () => true,
    () => 0,
    { crossOriginRedirects: true, originAllowed: (origin) => origin === "https://example.invalid" },
  );
  policy.click(url, page, 1, true);
  policy.before(request);
  policy.sent(request, []);
  const outside = { ...request, url: "https://other.example.invalid/file" };
  policy.redirect(request, outside.url, 302, [{ name: "Location", value: outside.url }]);
  policy.before(outside);
  policy.sent(outside, []);
  policy.redirect(outside, url, 302, [{ name: "Location", value: url }]);
  policy.before(request);
  policy.sent(request, []);
  await expect(policy.headers(request, 200, attachment)).resolves.toEqual({});
  expect(capture).not.toHaveBeenCalled();
});
