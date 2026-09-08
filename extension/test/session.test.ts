import { afterEach, describe, expect, it, vi } from "vitest";
import {
  applicableCookie,
  collectSession,
  sessionContext,
  sessionPermission,
} from "../src/session";

const cookie: browser.cookies.Cookie = {
  name: "fixture_session",
  value: "not-a-real-session",
  domain: "example.test",
  path: "/private",
  secure: true,
  httpOnly: true,
  hostOnly: true,
  session: true,
  storeId: "firefox-default",
  firstPartyDomain: "",
  sameSite: "lax",
};
const enabled = { enabled: true, referrer: "", authorization: "" };
const target = "https://example.test/private/file?sig=a%2Fb%2BC&x=2&x=1";
afterEach(() => vi.unstubAllGlobals());

describe("opt-in minimal session boundary", () => {
  it("does nothing, including no API calls, when not explicitly enabled", async () => {
    expect(await collectSession(target, { ...enabled, enabled: false }, undefined)).toBeUndefined();
    expect(sessionContext(target, { ...enabled, enabled: false }, [cookie])).toBeUndefined();
  });
  it("requests site scope without query, user info, wildcard subdomains, or port promises", () => {
    expect(sessionPermission("https://example.test:8443/private?secret=fake")).toBe(
      "https://example.test/*",
    );
    expect(() => sessionPermission("https://user:pass@example.test/x")).toThrow();
  });
  it("includes HttpOnly and preserves host-only versus domain semantics", () => {
    const context = sessionContext(target, enabled, [
      cookie,
      { ...cookie, domain: ".example.test", hostOnly: false },
    ])!;
    expect(context.credentials.cookies[0]).toMatchObject({
      http_only: true,
      secure: true,
      expires_at: null,
      domain: "example.test",
    });
    expect(context.credentials.cookies[1]?.domain).toBe(".example.test");
  });
  it.each([
    { domain: "other.test" },
    { domain: "ample.test" },
    { path: "/priv" },
    { path: "/private/other" },
    { storeId: "firefox-container-1" },
    { firstPartyDomain: "example.test" },
    { partitionKey: { topLevelSite: "https://example.test" } },
    { session: false, expirationDate: 10 },
  ])("excludes inapplicable or unsupported cookies: %j", (patch) => {
    expect(applicableCookie({ ...cookie, ...patch }, new URL(target), 10_000)).toBe(false);
  });
  it("never transfers Secure cookies to HTTP or host-only cookies to a subdomain", () => {
    expect(applicableCookie(cookie, new URL(target.replace("https:", "http:")), 0)).toBe(false);
    expect(
      applicableCookie(cookie, new URL(target.replace("example.test", "sub.example.test")), 0),
    ).toBe(false);
    expect(
      applicableCookie(
        { ...cookie, hostOnly: false, domain: ".example.test" },
        new URL(target.replace("example.test", "sub.example.test")),
        0,
      ),
    ).toBe(true);
  });
  it("permits only explicit same-origin referrer and bounded HTTPS Basic/Bearer values", () => {
    const context = sessionContext(
      target,
      {
        ...enabled,
        referrer: "https://example.test/page?fake=1",
        authorization: "Bearer synthetic-value",
      },
      [],
    );
    expect(context?.credentials.authorization).toEqual({
      scheme: "Bearer",
      value: "synthetic-value",
    });
    for (const patch of [
      { referrer: "https://other.test/page" },
      { referrer: "https://example.test/page#secret" },
      { authorization: "Bearer secret\r\nInjected: yes" },
      { authorization: "Digest fake" },
    ])
      expect(() => sessionContext(target, { ...enabled, ...patch }, [])).toThrow();
    expect(() =>
      sessionContext(
        target.replace("https:", "http:"),
        { ...enabled, authorization: "Basic fake" },
        [],
      ),
    ).toThrow();
  });
  it("queries only the selected URL in the default unpartitioned store after permission", async () => {
    const getAll = vi.fn().mockResolvedValue([cookie]);
    vi.stubGlobal("browser", {
      permissions: { contains: vi.fn().mockResolvedValue(true) },
      tabs: {
        get: vi.fn().mockResolvedValue({ cookieStoreId: "firefox-default", incognito: false }),
      },
      cookies: { getAll },
    });
    const context = await collectSession(target, enabled, 1);
    expect(context?.credentials.cookies).toHaveLength(1);
    expect(getAll).toHaveBeenCalledExactlyOnceWith({
      url: target,
      storeId: "firefox-default",
      firstPartyDomain: "",
      partitionKey: {},
    });
  });
  it.each([
    { cookieStoreId: "firefox-container-1", incognito: false },
    { cookieStoreId: "firefox-default", incognito: true },
    { incognito: false },
  ])("rejects ambiguous source context before reading cookies", async (tab) => {
    const getAll = vi.fn();
    vi.stubGlobal("browser", {
      permissions: { contains: vi.fn().mockResolvedValue(true) },
      tabs: { get: vi.fn().mockResolvedValue(tab) },
      cookies: { getAll },
    });
    await expect(collectSession(target, enabled, 1)).rejects.toThrow(
      "Session handoff was not submitted",
    );
    expect(getAll).not.toHaveBeenCalled();
  });
  it("does not expose browser API errors or read cookies without permission", async () => {
    const getAll = vi.fn();
    vi.stubGlobal("browser", {
      permissions: { contains: vi.fn().mockRejectedValue(new Error("private synthetic URL")) },
      cookies: { getAll },
    });
    await expect(collectSession(target, enabled, 1)).rejects.not.toThrow("private synthetic URL");
    expect(getAll).not.toHaveBeenCalled();
  });
});

describe("selected-site permission wildcard confinement", () => {
  it.each([
    "https://*.example.test/file",
    "https://%2a.example.test/file",
    "https://*/file",
    "https://＊.example.test/file",
  ])("rejects %s before even checking permissions", async (url) => {
    const contains = vi.fn();
    const getAll = vi.fn();
    vi.stubGlobal("browser", { permissions: { contains }, cookies: { getAll } });
    expect(() => sessionPermission(url)).toThrow();
    await expect(collectSession(url, enabled, 1)).rejects.toThrow();
    expect(contains).not.toHaveBeenCalled();
    expect(getAll).not.toHaveBeenCalled();
  });
});
