import { isAbsolute, relative, sep } from "node:path";

// Callers resolve symlinks with realpath before comparing locations.
export function outsideCheckout(checkout, input) {
  const path = relative(checkout, input);
  return path === ".." || path.startsWith(`..${sep}`) || isAbsolute(path);
}

export function originalCommitAbsent(response, commit) {
  // 401/403/404, failed transport, or a missing repository do not prove erasure.
  return (
    /^[0-9a-f]{40}$/u.test(commit) &&
    response.status === 422 &&
    response.body?.message === `No commit found for SHA: ${commit}`
  );
}
