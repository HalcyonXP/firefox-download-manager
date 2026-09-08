import { directUrl } from "./creation";

export interface SessionInput {
  enabled: boolean;
  referrer: string;
  authorization: string;
}
export interface SessionContext {
  referrer?: string;
  credentials: {
    cookies: {
      name: string;
      value: string;
      domain: string;
      path: string;
      secure: boolean;
      http_only: boolean;
      expires_at: string | null;
    }[];
    authorization?: { scheme: string; value: string };
  };
}
export class SessionError extends Error {
  constructor() {
    super(
      "Session handoff was not submitted. Allow cookies and this site's optional permission, use a normal default-store tab, and check the same-origin referrer / HTTPS Basic or Bearer value. Private, container, partitioned, and first-party-isolated sessions are not supported. Wildcard hosts and IPv6 session permissions are unsupported.",
    );
  }
}

/** Firefox host permissions cover scheme/host, not paths or individual ports. */
export function sessionPermission(target: string): string {
  const url = directUrl(target);
  if (url.hostname.includes(":") || url.hostname.includes("*")) throw new SessionError();
  return `${url.protocol}//${url.hostname}/*`;
}

export function applicableCookie(
  cookie: browser.cookies.Cookie,
  target: URL,
  now: number,
): boolean {
  const domain = cookie.domain.replace(/^\./u, "").toLowerCase();
  return (
    cookie.storeId === "firefox-default" &&
    !cookie.firstPartyDomain &&
    !cookie.partitionKey?.topLevelSite &&
    (!cookie.secure || target.protocol === "https:") &&
    (cookie.session ||
      (cookie.expirationDate !== undefined && cookie.expirationDate * 1000 > now)) &&
    (target.hostname === domain || (!cookie.hostOnly && target.hostname.endsWith(`.${domain}`))) &&
    (target.pathname === cookie.path ||
      (target.pathname.startsWith(cookie.path) &&
        (cookie.path.endsWith("/") || target.pathname.slice(cookie.path.length).startsWith("/"))))
  );
}

/** No cookie data is collected unless the per-download option is enabled. */
export function sessionContext(
  target: string,
  input: SessionInput,
  cookies: readonly browser.cookies.Cookie[],
  now = Date.now(),
): SessionContext | undefined {
  if (!input.enabled) return undefined;
  const url = directUrl(target);
  const context: SessionContext = {
    credentials: {
      cookies: cookies
        .filter((cookie) => applicableCookie(cookie, url, now))
        .map((cookie) => ({
          name: cookie.name,
          value: cookie.value,
          // Preserve host-only versus Domain semantics within the reserved v2 shape.
          domain: cookie.hostOnly
            ? cookie.domain.replace(/^\./u, "")
            : `.${cookie.domain.replace(/^\./u, "")}`,
          path: cookie.path,
          secure: cookie.secure,
          http_only: cookie.httpOnly,
          expires_at: cookie.session ? null : new Date(cookie.expirationDate! * 1000).toISOString(),
        })),
    },
  };
  if (input.referrer) {
    const referrer = directUrl(input.referrer);
    if (referrer.origin !== url.origin || referrer.hash) throw new SessionError();
    context.referrer = input.referrer;
  }
  if (input.authorization) {
    const match = /^(Basic|Bearer) ([\u0021-\u007e]+)$/u.exec(input.authorization);
    if (url.protocol !== "https:" || !match) throw new SessionError();
    context.credentials.authorization = { scheme: match[1]!, value: match[2]! };
  }
  if (
    new TextEncoder().encode(JSON.stringify(context)).byteLength > 64 * 1024 ||
    context.credentials.cookies.length > 256
  )
    throw new SessionError();
  return context;
}

export async function collectSession(
  target: string,
  input: SessionInput,
  tabId: number | undefined,
): Promise<SessionContext | undefined> {
  if (!input.enabled) return undefined;
  try {
    if (
      tabId === undefined ||
      !(await browser.permissions.contains({
        permissions: ["cookies"],
        origins: [sessionPermission(target)],
      }))
    )
      throw new SessionError();
    const tab = await browser.tabs.get(tabId);
    if (tab.incognito || tab.cookieStoreId !== "firefox-default") throw new SessionError();
    const cookies = await browser.cookies.getAll({
      url: target,
      storeId: "firefox-default",
      firstPartyDomain: "",
      partitionKey: {},
    });
    return sessionContext(target, input, cookies);
  } catch {
    throw new SessionError();
  }
}
