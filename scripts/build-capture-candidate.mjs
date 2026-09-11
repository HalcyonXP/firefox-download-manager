// Separate development artifact. Never changes extension/dist or package selection.
import { mkdir, lstat, readFile, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { extensionLicenses } from "./extension-licenses.mjs";
import { candidatePayloads, validateCaptureCandidate } from "./capture-candidate-policy.mjs";

const output = path.resolve(process.argv[2] ?? ".");
const relative = path.relative(path.resolve("artifacts"), output);
if (
  process.argv.length !== 3 ||
  !relative ||
  relative.startsWith("..") ||
  path.isAbsolute(relative)
)
  throw new Error("A new directory beneath artifacts is required");
for (let parent = path.dirname(output); ; parent = path.dirname(parent)) {
  const stat = await lstat(parent);
  if (!stat.isDirectory() || stat.isSymbolicLink())
    throw new Error("Candidate parent is not ordinary");
  if (parent === path.dirname(parent)) break;
}
const git = (...args) =>
  execFileSync("git", args, { encoding: "utf8", maxBuffer: 1024 * 1024, windowsHide: true });
const revision = git("rev-parse", "HEAD").trim();
if (!/^[a-f0-9]{40}$/u.test(revision)) throw new Error("Candidate revision unavailable");
const initiallyDirty = git("status", "--porcelain").length !== 0;
const manifest = JSON.parse(await readFile("extension/candidate/manifest.json", "utf8"));
validateCaptureCandidate(manifest);
await mkdir(output); // Exclusive; failed builds are preserved, never adopted.
process.env.ESBUILD_WORKER_THREADS = "0";
process.env.ESBUILD_MAX_BUFFER = "16777216";
delete process.env.ESBUILD_BINARY_PATH;
const { buildSync } = await import("esbuild");
const payloads = {};
for (const [name, entry] of Object.entries({
  "background.js": "extension/src/automatic-background.ts",
  "click.js": "extension/src/capture-click.ts",
  "manager.js": "extension/src/manager.ts",
})) {
  const result = buildSync({
    bundle: true,
    entryPoints: [entry],
    format: "iife",
    write: false,
    legalComments: "none",
    platform: "browser",
    target: "firefox156",
    sourcemap: false,
  });
  if (result.outputFiles.length !== 1) throw new Error("Candidate bundle shape refused");
  payloads[name] = result.outputFiles[0].contents;
}
payloads["manifest.json"] = Buffer.from(JSON.stringify(manifest));
for (const name of ["manager.html", "manager.css"])
  payloads[name] = await readFile(`extension/src/${name}`);
for (const [name, text] of Object.entries(await extensionLicenses()))
  payloads[name] = Buffer.from(text);
if (Object.keys(payloads).sort().join() !== candidatePayloads.slice().sort().join())
  throw new Error("Candidate inventory refused");
const hashes = {};
for (const [name, data] of Object.entries(payloads)) {
  if (!data.length || data.length > 1024 * 1024)
    throw new Error("Candidate payload bound exceeded");
  await writeFile(path.join(output, name), data, { flag: "wx" });
  hashes[name] = createHash("sha256").update(data).digest("hex");
}
if (git("rev-parse", "HEAD").trim() !== revision)
  throw new Error("Source revision changed during build");
await writeFile(
  path.join(output, "BUILD.json"),
  JSON.stringify({
    version: 1,
    candidate: true,
    qualification: false,
    source_commit: revision,
    source_dirty: initiallyDirty || git("status", "--porcelain").length !== 0,
    files: hashes,
  }),
  { flag: "wx" },
);
