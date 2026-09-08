import { execFileSync } from "node:child_process";
import { lstat, readFile } from "node:fs/promises";

import { allowedEmail, inspectText, sensitivePath } from "./privacy-policy.mjs";

function git(...arguments_) {
  return execFileSync("git", arguments_, {
    encoding: "utf8",
    maxBuffer: 32 * 1024 * 1024,
    stdio: ["ignore", "pipe", "pipe"],
  });
}

async function main() {
  const files = git("ls-files", "-z").split("\0").filter(Boolean);
  let failures = 0;
  for (const [index, path] of files.entries()) {
    // File indexes and line numbers are intentional: even a path can contain PII.
    if (sensitivePath(path)) {
      console.error(`Tracked file #${index + 1}: disallowed runtime/private artifact`);
      failures += 1;
    }
    if ((await lstat(path)).isSymbolicLink()) {
      console.error(
        `Tracked file #${index + 1}: symlink skipped rather than following outside the checkout`,
      );
      failures += 1;
      continue;
    }
    const text = await readFile(path, "utf8");
    for (const finding of inspectText(text)) {
      console.error(`Tracked file #${index + 1}, line ${finding.line}: ${finding.category}`);
      failures += 1;
    }
  }

  let commits = 0;
  if (process.argv.includes("--history")) {
    if (git("rev-parse", "--is-shallow-repository").trim() === "true") {
      console.error(
        "Full history unavailable: fetch full history before claiming this check passed.",
      );
      failures += 1;
    } else {
      const messages = git("log", "HEAD", "--format=%ae%x00%ce%x00%B%x1e").split("\x1e");
      for (const message of messages) {
        if (!message.trim()) continue;
        commits += 1;
        const [author, committer, body = ""] = message.trim().split("\0");
        if (
          !allowedEmail(author, true) ||
          !allowedEmail(committer, true) ||
          inspectText(body, { history: true }).length
        ) {
          console.error(
            `Commit #${commits}: non-public metadata or sensitive message (values withheld)`,
          );
          failures += 1;
        }
      }
    }
  }
  if (failures) {
    console.error(
      `Privacy check failed: ${failures} finding(s). Inspect locally; do not paste sensitive values into GitHub.`,
    );
    process.exitCode = 1;
  } else {
    console.log(
      `Privacy patterns passed for ${files.length} tracked files${commits ? ` and ${commits} reachable commits` : ""}. This does not scan GitHub PR caches, logs, or arbitrary identifying prose.`,
    );
  }
}

main().catch(() => {
  console.error(
    "Privacy check could not complete. Inspect locally; error details are withheld to avoid leaking paths or data.",
  );
  process.exitCode = 1;
});
