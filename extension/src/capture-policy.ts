import { directUrl, safeFilename, suggestedFilename } from "./creation";
import type { BrowserHandoff, CaptureDecision } from "./browser-handoff";

export interface CaptureRequest {
  readonly requestId: string;
  readonly url: string;
  readonly method: string;
  readonly type: string;
  readonly tabId: number;
  readonly frameId: number;
  readonly incognito?: boolean | undefined;
  readonly cookieStoreId?: string | undefined;
  readonly originUrl?: string | undefined;
}
interface Header {
  readonly name: string;
  readonly value?: string | undefined;
}
const ANONYMOUS_HEADERS = new Set([
  "host",
  "user-agent",
  "accept",
  "accept-language",
  "accept-encoding",
  "connection",
  "upgrade-insecure-requests",
  "sec-fetch-dest",
  "sec-fetch-mode",
  "sec-fetch-site",
  "sec-fetch-user",
  "priority",
  "te",
  "referer",
  "dnt",
  "sec-gpc",
  "cache-control",
  "pragma",
]);
interface Click {
  readonly target: string;
  readonly document: string;
  readonly time: number;
  valid: boolean;
  used: boolean;
}
interface Request {
  readonly tab: number;
  readonly initial: string;
  current: string;
  redirectTarget: string | undefined;
  valid: boolean;
  click: Click | undefined;
  sentUrl: string | undefined;
  decisionUrl: string | undefined;
  anonymous: boolean;
  redirects: number;
}
function resourceUrl(value: string): string | undefined {
  try {
    const parsed = directUrl(value);
    parsed.hash = "";
    return parsed.href;
  } catch {
    return undefined;
  }
}
function safeContext(details: CaptureRequest): boolean {
  return (
    details.method === "GET" &&
    details.type === "main_frame" &&
    details.frameId === 0 &&
    Number.isSafeInteger(details.tabId) &&
    details.tabId >= 0 &&
    details.incognito === false &&
    details.cookieStoreId === "firefox-default" &&
    resourceUrl(details.url) !== undefined
  );
}
function field(headers: readonly Header[], name: string): string | undefined {
  const matches = headers.filter((header) => header.name.toLowerCase() === name);
  return matches.length === 1 &&
    typeof matches[0]?.value === "string" &&
    matches[0].value.length <= 8192
    ? matches[0].value
    : undefined;
}
export function attachmentFilename(headers: readonly Header[], url: string): string | undefined {
  const disposition = field(headers, "content-disposition");
  if (!disposition || !/^attachment(?:\s*;|$)/iu.test(disposition)) return undefined;
  const utf8 = /;\s*filename\*=UTF-8''([^;]*)/iu.exec(disposition)?.[1];
  if (utf8 !== undefined) {
    try {
      return safeFilename(decodeURIComponent(utf8.trim()));
    } catch {
      return undefined;
    }
  }
  const quoted = /;\s*filename="([^"\\]*)"(?:\s*;|\s*$)/iu.exec(disposition)?.[1];
  return quoted !== undefined ? safeFilename(quoted) : suggestedFilename(url);
}

export interface CaptureOptions {
  readonly crossOriginRedirects?: boolean;
  readonly originAllowed?: (origin: string) => boolean;
}

/** Anonymous redirect chains; cross-origin is opt-in until separately qualified. */
export class CapturePolicy {
  readonly #handoff: Pick<BrowserHandoff, "capture" | "terminal">;
  readonly #enabled: () => boolean;
  readonly #now: () => number;
  readonly #crossOrigin: boolean;
  readonly #originAllowed: (origin: string) => boolean;
  readonly #clicks = new Map<number, Click>();
  readonly #requests = new Map<string, Request>();
  #overflowUntil = 0;
  constructor(
    handoff: Pick<BrowserHandoff, "capture" | "terminal">,
    enabled: () => boolean,
    now = () => performance.now(),
    options: CaptureOptions = {},
  ) {
    this.#handoff = handoff;
    this.#enabled = enabled;
    this.#now = now;
    this.#crossOrigin = options.crossOriginRedirects === true;
    this.#originAllowed = options.originAllowed ?? (() => true);
  }
  #allowed(url: string): boolean {
    try {
      return this.#originAllowed(new URL(url).origin) === true;
    } catch {
      return false;
    }
  }
  click(target: string, document: string, tab: number, trusted: boolean): void {
    const now = this.#now();
    for (const [key, click] of this.#clicks)
      if (now - click.time > 5000) {
        click.valid = false;
        this.#clicks.delete(key);
      }
    if (!this.#enabled() || !trusted || !Number.isSafeInteger(tab) || tab < 0) return;
    const url = resourceUrl(target);
    const source = resourceUrl(document);
    if (!url || !source) return;
    const prior = this.#clicks.get(tab);
    if (prior) {
      prior.valid = false; // Ambiguous rapid clicks do not lend each other authority.
      return;
    }
    if (this.#clicks.size >= 32) {
      this.#overflowUntil = now + 5000;
      return;
    }
    this.#clicks.set(tab, { target: url, document: source, time: now, valid: true, used: false });
  }
  before(details: CaptureRequest): void {
    const existing = this.#requests.get(details.requestId);
    if (existing) {
      existing.anonymous = false;
      existing.sentUrl = undefined;
      if (
        !safeContext(details) ||
        !this.#allowed(details.url) ||
        details.tabId !== existing.tab ||
        existing.decisionUrl !== undefined ||
        existing.redirectTarget !== resourceUrl(details.url) ||
        ++existing.redirects > 8
      )
        existing.valid = false;
      existing.current = resourceUrl(details.url) ?? "";
      existing.redirectTarget = undefined;
      return;
    }
    if (!this.#enabled() || !safeContext(details) || !this.#allowed(details.url)) return;
    if (this.#requests.size >= 64) {
      this.#overflowUntil = this.#now() + 5000;
      for (const value of this.#requests.values()) value.valid = false;
      return;
    }
    const click = this.#clicks.get(details.tabId);
    const correlated =
      click &&
      click.valid &&
      !click.used &&
      click.target === resourceUrl(details.url) &&
      this.#now() - click.time <= 5000
        ? click
        : undefined;
    if (correlated) correlated.used = true;
    this.#requests.set(details.requestId, {
      tab: details.tabId,
      initial: resourceUrl(details.url)!,
      current: resourceUrl(details.url)!,
      redirectTarget: undefined,
      valid: true,
      anonymous: false,
      sentUrl: undefined,
      decisionUrl: undefined,
      redirects: 0,
      click: correlated,
    });
  }
  sent(details: CaptureRequest, headers: readonly Header[] | undefined): void {
    const request = this.#requests.get(details.requestId);
    if (!request) return;
    request.sentUrl = resourceUrl(details.url);
    // Inspect names only. No cookie/Authorization/referrer value is retained or replayed.
    request.anonymous =
      safeContext(details) &&
      details.tabId === request.tab &&
      request.sentUrl === request.current &&
      request.redirectTarget === undefined &&
      headers !== undefined &&
      headers.length <= 128 &&
      headers.every((header) => ANONYMOUS_HEADERS.has(header.name.toLowerCase()));
    // A later anonymous redirect must not erase earlier session/unknown authority.
    if (!request.anonymous) request.valid = false;
  }
  headers(
    details: CaptureRequest,
    status: number,
    headers: readonly Header[] | undefined,
  ): Promise<CaptureDecision> {
    const request = this.#requests.get(details.requestId);
    if (
      !request ||
      details.tabId !== request.tab ||
      !this.#enabled() ||
      !safeContext(details) ||
      this.#now() < this.#overflowUntil
    )
      return Promise.resolve({});
    if (resourceUrl(details.url) !== request.current) return Promise.resolve({});
    const eligible = (): boolean =>
      this.#enabled() &&
      request.valid &&
      request.redirectTarget === undefined &&
      request.current === resourceUrl(details.url) &&
      this.#allowed(details.url) &&
      request.anonymous &&
      !!request.click?.valid &&
      request.click.document === resourceUrl(details.originUrl ?? "") &&
      this.#now() - request.click.time <= 5000 &&
      this.#now() >= this.#overflowUntil &&
      request.sentUrl === resourceUrl(details.url);
    if (status !== 200 || !headers || headers.length > 128 || !eligible())
      return Promise.resolve({});
    if (
      headers.some((header) =>
        ["set-cookie", "www-authenticate", "proxy-authenticate", "content-range"].includes(
          header.name.toLowerCase(),
        ),
      )
    )
      return Promise.resolve({});
    const encoding = field(headers, "content-encoding");
    if (
      headers.some((header) => header.name.toLowerCase() === "content-encoding") &&
      encoding?.toLowerCase() !== "identity"
    )
      return Promise.resolve({});
    const vary = field(headers, "vary");
    if (
      headers.some((header) => header.name.toLowerCase() === "vary") &&
      vary?.trim().toLowerCase() !== "accept-encoding"
    )
      return Promise.resolve({});
    const filename = attachmentFilename(headers, details.url);
    if (!filename) return Promise.resolve({});
    if (request.decisionUrl !== undefined) {
      request.valid = false;
      return Promise.resolve({});
    }
    request.decisionUrl = resourceUrl(details.url)!;
    return this.#handoff.capture(
      details.requestId,
      { url: resourceUrl(details.url)!, suggested_filename: filename },
      eligible,
    );
  }
  redirect(
    details: CaptureRequest,
    target: string,
    status: number,
    headers: readonly Header[] | undefined,
  ): void {
    const request = this.#requests.get(details.requestId);
    if (!request) return;
    const url = resourceUrl(target);
    const location = headers && headers.length <= 128 ? field(headers, "location") : undefined;
    let declared: string | undefined;
    try {
      if (location !== undefined) declared = resourceUrl(new URL(location, details.url).href);
    } catch {
      /* Invalid Location cannot authorize another request. */
    }
    if (
      !safeContext(details) ||
      details.tabId !== request.tab ||
      !request.valid ||
      !request.anonymous ||
      request.sentUrl !== request.current ||
      resourceUrl(details.url) !== request.current ||
      request.redirectTarget !== undefined ||
      request.decisionUrl !== undefined ||
      ![301, 302, 303, 307, 308].includes(status) ||
      !url ||
      !this.#allowed(url) ||
      declared !== url ||
      !headers ||
      headers.some((header) =>
        ["set-cookie", "www-authenticate", "proxy-authenticate", "content-range"].includes(
          header.name.toLowerCase(),
        ),
      ) ||
      (new URL(request.current).protocol === "https:" && new URL(url).protocol !== "https:") ||
      (!this.#crossOrigin && new URL(url).origin !== new URL(request.initial).origin)
    )
      request.valid = false;
    request.redirectTarget = request.valid ? url : undefined;
    request.anonymous = false;
    request.sentUrl = undefined;
  }
  terminal(details: CaptureRequest, error?: string): void {
    const request = this.#requests.get(details.requestId);
    const matchesDecision =
      request?.tab === details.tabId &&
      request.valid &&
      request.anonymous &&
      request.decisionUrl !== undefined &&
      request.decisionUrl === resourceUrl(details.url) &&
      request.sentUrl === request.decisionUrl &&
      safeContext(details);
    if (request) request.valid = false;
    this.#requests.delete(details.requestId);
    this.#handoff.terminal(details.requestId, matchesDecision ? error : undefined);
  }
}
