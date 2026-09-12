// Development-only boundary until Firefox download-protection integration is qualified.
// A non-binary filename does not bypass Firefox's post-download URL blocklist checks.
export function candidateOriginAllowed(origin: string): boolean {
  try {
    const url = new URL(origin);
    return url.origin === origin && url.protocol === "http:" && url.hostname === "127.0.0.1";
  } catch {
    return false;
  }
}
