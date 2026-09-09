import { isAbsolute, relative, sep } from "node:path";

// Callers resolve symlinks with realpath before comparing locations.
export function outsideCheckout(checkout, input) {
  const path = relative(checkout, input);
  return path === ".." || path.startsWith(`..${sep}`) || isAbsolute(path);
}

export function platformCoverageObservation(issues, pulls, totals, graphErrors) {
  const bounded = (value) =>
    Number.isSafeInteger(value) && value >= 0 && value <= 10000 ? value : "invalid";
  const usable = Array.isArray(issues) && issues.every((row) => row && typeof row === "object");
  return {
    issueEndpointRecords: bounded(Array.isArray(issues) ? issues.length : undefined),
    regularIssueRecords: usable
      ? bounded(issues.filter((row) => !Object.hasOwn(row, "pull_request")).length)
      : "invalid",
    fullPullRecords: bounded(Array.isArray(pulls) ? pulls.length : undefined),
    expectedIssues: bounded(totals?.issues),
    expectedPulls: bounded(totals?.pullRequests),
    graphErrors: graphErrors ? "present" : "absent",
  };
}

// The issues endpoint may omit PR records. Read pulls independently and reconcile
// against independent repository totals rather than blessing a short response.
export function platformRecordCounts(issues, pulls, totals) {
  const fail = () => {
    throw new Error("Platform issue/PR coverage is incomplete");
  };
  const numbers = (rows) => {
    if (!Array.isArray(rows)) fail();
    const result = new Set();
    for (const row of rows) {
      if (!Number.isSafeInteger(row?.number) || row.number <= 0 || result.has(row.number)) fail();
      result.add(row.number);
    }
    return result;
  };
  numbers(issues);
  const pullNumbers = numbers(pulls);
  const regular = numbers(issues.filter((row) => !Object.hasOwn(row, "pull_request")));
  if (
    !Number.isSafeInteger(totals?.issues) ||
    !Number.isSafeInteger(totals?.pullRequests) ||
    totals.issues < 0 ||
    totals.pullRequests < 0 ||
    regular.size !== totals.issues ||
    pullNumbers.size !== totals.pullRequests ||
    [...regular].some((number) => pullNumbers.has(number)) ||
    issues.some((row) => Object.hasOwn(row, "pull_request") && !pullNumbers.has(row.number))
  )
    fail();
  return {
    issues: regular.size,
    pullRequests: pullNumbers.size,
    issueAndPRRecords: regular.size + pullNumbers.size,
  };
}

export function originalCommitAbsent(response, commit) {
  // 401/403/404, failed transport, or a missing repository do not prove erasure.
  return (
    /^[0-9a-f]{40}$/u.test(commit) &&
    response.status === 422 &&
    response.body?.message === `No commit found for SHA: ${commit}`
  );
}
