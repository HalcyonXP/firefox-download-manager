// Remote Git/platform audit. Private comparison inputs must remain outside the checkout.
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtemp, readFile, realpath, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { allowedEmail, inspectText, sensitivePath } from "./privacy-policy.mjs";
import { originalCommitAbsent, outsideCheckout } from "./publication-policy.mjs";
import { repository as target } from "./repository-policy.mjs";

const maxBuffer = 64 * 1024 * 1024;
let stage = "inputs";
function requireCondition(value) {
  if (!value) throw new Error("Publication verification condition failed");
}
function command(program, args, options = {}) {
  return execFileSync(program, args, { encoding: "utf8", maxBuffer, stdio: "pipe", ...options });
}
function api(endpoint) {
  const result = spawnSync("gh", ["api", "--include", endpoint], {
    encoding: "utf8",
    maxBuffer,
    stdio: "pipe",
  });
  const sections = (result.stdout ?? "").split(/\r?\n\r?\n/u);
  const status = Number(sections.shift()?.match(/^HTTP\/[\d.]+ (\d+)/u)?.[1]);
  requireCondition(Number.isInteger(status) && status >= 200);
  const body = JSON.parse(sections.join("\n\n"));
  return { status, body };
}
function get(endpoint) {
  const response = api(endpoint);
  requireCondition(response.status === 200);
  return response.body;
}
function pages(endpoint, key) {
  const records = [];
  for (let page = 1; page <= 100; page += 1) {
    const separator = endpoint.includes("?") ? "&" : "?";
    const value = get(`${endpoint}${separator}per_page=100&page=${page}`);
    const batch = key ? value[key] : value;
    requireCondition(Array.isArray(batch));
    records.push(...batch);
    if (batch.length < 100) return records;
  }
  throw new Error("Pagination exceeded audit bound");
}

async function main() {
  const args = process.argv.slice(2);
  requireCondition(args.length === 2 || (args.length === 3 && args[2] === "--metadata-only"));
  const metadataOnly = args[2] === "--metadata-only";
  const checkout = await realpath(command("git", ["rev-parse", "--show-toplevel"]).trim());
  for (const path of args.slice(0, 2))
    requireCondition(outsideCheckout(checkout, await realpath(path)));
  const { emails, archiveRepository: source } = JSON.parse(await readFile(args[0], "utf8"));
  requireCondition(
    typeof source === "string" && /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/u.test(source),
  );
  const originals = JSON.parse(await readFile(args[1], "utf8"));
  requireCondition(Array.isArray(emails) && emails.length > 0);
  requireCondition(emails.every((value) => typeof value === "string" && value.includes("@")));
  requireCondition(Array.isArray(originals) && originals.length > 0);
  requireCondition(originals.every((value) => /^[0-9a-f]{40}$/u.test(value)));
  const knownValues = emails.map((value) => value.toLowerCase());
  function inspect(text, history = false) {
    requireCondition(inspectText(text, { history }).length === 0);
    requireCondition(!knownValues.some((value) => text.toLowerCase().includes(value)));
  }
  stage = "repository-metadata";
  const sourceMetadata = get(`repos/${source}`);
  const targetMetadata = get(`repos/${target}`);
  requireCondition(sourceMetadata.private === true);
  requireCondition(targetMetadata.fork === false);
  requireCondition(!targetMetadata.parent && sourceMetadata.id !== targetMetadata.id);
  requireCondition(targetMetadata.default_branch === "main" && !targetMetadata.has_wiki);
  inspect(JSON.stringify(targetMetadata));
  const directory = await mkdtemp(join(tmpdir(), "download-manager-publication-"));
  try {
    const clone = join(directory, "target.git");
    stage = "fresh-clone";
    const remoteUrl = `https://github.com/${target}.git`;
    const advertised = () =>
      command("git", ["ls-remote", "--refs", remoteUrl]).trim().split("\n").sort().join("\n");
    const beforeRefs = advertised();
    command("git", ["clone", "--bare", remoteUrl, clone]);
    const git = (args, options) => command("git", ["-C", clone, ...args], options);
    git([
      "fetch",
      "origin",
      "+refs/pull/*/head:refs/audit/pull/*/head",
      "+refs/pull/*/merge:refs/audit/pull/*/merge",
    ]);
    stage = "refs";
    const refs = git(["for-each-ref", "--format=%(refname)"]).trim().split("\n");
    // Dependabot can create fresh refs immediately after the first push. Audit all of them.
    requireCondition(refs.includes("refs/heads/main"));
    requireCondition(
      refs.every(
        (ref) =>
          ref.startsWith("refs/heads/") ||
          ref.startsWith("refs/tags/") ||
          ref.startsWith("refs/audit/pull/"),
      ),
    );
    const head = git(["rev-parse", "refs/heads/main"]).trim();
    const commits = git(["rev-list", "--all"]).trim().split("\n");
    stage = "commit-identities";
    const paths = new Set();
    for (const commit of commits) {
      const [author, committer, ...message] = git([
        "show",
        "-s",
        "--format=%ae%n%ce%n%an%n%cn%n%B",
        commit,
      ]).split("\n");
      requireCondition(allowedEmail(author, true) && allowedEmail(committer, true));
      inspect([author, committer, ...message].join("\n"), true);
      for (const path of git(["ls-tree", "-r", "--name-only", "-z", commit]).split("\0")) {
        if (path) paths.add(path);
      }
    }
    stage = "historical-paths";
    for (const path of paths) {
      requireCondition(!sensitivePath(path));
      inspect(path);
    }
    stage = "historical-blobs";
    const objects = git(["rev-list", "--objects", "--all", "--no-object-names"]);
    const types = git(["cat-file", "--batch-check=%(objectname) %(objecttype)"], {
      input: objects,
    });
    const blobs = types
      .trim()
      .split("\n")
      .filter((line) => line.endsWith(" blob"));
    for (const line of blobs) inspect(git(["cat-file", "blob", line.split(" ")[0]]));
    stage = "original-commit-lookups";
    // A successful known-good read prevents an authentication failure masquerading as absence.
    requireCondition(get(`repos/${target}/commits/${head}`).sha === head);
    for (const commit of originals) {
      const response = api(`repos/${target}/commits/${commit}`);
      requireCondition(originalCommitAbsent(response, commit));
    }
    stage = "platform-surfaces";
    const surfaces = {
      issues: pages(`repos/${target}/issues?state=all`),
      comments: pages(`repos/${target}/issues/comments`),
      reviewComments: pages(`repos/${target}/pulls/comments`),
      commitComments: pages(`repos/${target}/comments`),
      events: pages(`repos/${target}/events`),
      issueEvents: pages(`repos/${target}/issues/events`),
      milestones: pages(`repos/${target}/milestones?state=all`),
      releases: pages(`repos/${target}/releases`),
      deployments: pages(`repos/${target}/deployments`),
      forks: pages(`repos/${target}/forks`),
      runs: pages(`repos/${target}/actions/runs`, "workflow_runs"),
      artifacts: pages(`repos/${target}/actions/artifacts`, "artifacts"),
      caches: pages(`repos/${target}/actions/caches`, "actions_caches"),
    };
    stage = "unstarted-jobs";
    surfaces.jobs = surfaces.runs.flatMap((run) =>
      pages(`repos/${target}/actions/runs/${run.id}/jobs?filter=all`, "jobs"),
    );
    // Metadata-only mode is explicit and never reports complete archive/privacy clearance.
    if (!metadataOnly)
      requireCondition(
        surfaces.jobs.every((job) => job.status === "completed" && (job.steps ?? []).length === 0),
      );
    stage = "platform-content";
    inspect(JSON.stringify(surfaces));
    stage = "empty-initial-surfaces";
    if (!metadataOnly)
      for (const key of ["releases", "deployments", "forks", "artifacts", "caches"]) {
        requireCondition(surfaces[key].length === 0);
      }
    stage = "final-consistency";
    requireCondition(advertised() === beforeRefs);
    requireCondition(get(`repos/${target}/commits/main`).sha === head);
    requireCondition(get(`repos/${source}`).private === true);
    console.log(
      JSON.stringify(
        {
          checkedAt: new Date().toISOString(),
          repository: target,
          head,
          privateArchiveVerified: true,
          visibility: targetMetadata.private ? "private" : "public",
          independentNonFork: true,
          refs: refs.length,
          commits: commits.length,
          issueAndPRRecords: surfaces.issues.length,
          historicalBlobs: blobs.length,
          historicalPaths: paths.size,
          originalCommitLookupsRejected: originals.length,
          targetMetadataChecked: true,
          runs: surfaces.runs.length,
          artifacts: surfaces.artifacts.length,
          caches: surfaces.caches.length,
          scope: metadataOnly ? "git-and-platform-metadata-only" : "initial-empty-archive-gate",
          gitAndMetadataCheck: "passed",
          logArtifactCacheContentsReviewed: false,
          furtherArchiveReviewRequired: metadataOnly,
          privateArchivePrivacyCleared: false,
          releaseQualified: false,
        },
        null,
        2,
      ),
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}
main().catch(() => {
  console.error(`Audit stage: ${stage}`);
  console.error(
    "Remote privacy audit failed or incomplete. Inspect locally before publishing additional data; details withheld to avoid exposing private values or paths. Required inputs: external private-identifiers JSON, then original-commits JSON; optional --metadata-only never clears log/artifact contents.",
  );
  process.exitCode = 1;
});
