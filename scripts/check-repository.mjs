import { execFileSync } from "node:child_process";
import { lstat, readFile } from "node:fs/promises";
import { repositoryFindings } from "./repository-policy.mjs";

async function main() {
  const files = execFileSync("git", ["ls-files", "-z"], { encoding: "utf8", stdio: "pipe" })
    .split("\0")
    .filter(Boolean);
  let failures = 0;
  for (const [index, file] of files.entries()) {
    if ((await lstat(file)).isSymbolicLink()) throw new Error("Symlinks require review");
    for (const finding of repositoryFindings(await readFile(file, "utf8"))) {
      console.error(`Tracked file #${index + 1}, line ${finding.line}: ${finding.category}`);
      failures += 1;
    }
  }
  if (failures) process.exitCode = 1;
  else
    console.log(
      `Canonical repository references passed for ${files.length} tracked files. Historical commits are not rewritten by this check.`,
    );
}
main().catch(() => {
  console.error(
    "Repository reference check could not complete; inspect locally. Error details withheld.",
  );
  process.exitCode = 1;
});
