import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

// Structural guard, not a YAML interpreter or proof that hosted jobs passed.
const workflow = readFileSync(new URL("../.github/workflows/ci.yml", import.meta.url), "utf8");
const blocks = new Map(
  [
    ...workflow.matchAll(
      /^ {2}([a-z][a-z0-9-]*):\r?\n([\s\S]*?)(?=^ {2}[a-z][a-z0-9-]*:\r?\n|$(?![\s\S]))/gmu,
    ),
  ].map(([, name, body]) => [name, body]),
);
const quality = blocks.get("quality");
const packaging = blocks.get("package");

test("debug checks and package qualification have separate unchanged resource budgets", () => {
  for (const job of [quality, packaging]) {
    assert.equal(typeof job, "string");
    assert.match(job, /runs-on: windows-latest/u);
    assert.match(job, /timeout-minutes: 30/u);
    assert.match(job, /CARGO_BUILD_JOBS: "1"/u);
    assert.match(job, /fetch-depth: 0/u);
    assert.doesNotMatch(job, /continue-on-error:|needs:|--test-threads/u);
  }
  for (const command of [
    "npm run check",
    "npm audit --audit-level=high",
    "npm run licenses:js",
    "cargo fmt --all --check",
    "cargo clippy --workspace --all-targets --all-features --locked -- -D warnings",
    "cargo test --workspace --all-features --locked",
    "cargo build --workspace --all-features --locked",
    "test_build_package.py",
    "test_qualification.py",
    "1..10 | ForEach-Object",
    "cargo test -p download-manager-engine --test scheduler stop --locked",
  ])
    assert.ok(quality.includes(command), `Missing quality gate: ${command}`);
  assert.ok(!quality.includes("build-package.ps1"));
});

test("both clean builds and all exact-package checks remain together before artifact upload", () => {
  const steps = [
    "./scripts/build-package.ps1 -TestRust -Rebuild",
    "1..3 | ForEach-Object",
    "cancel_interrupts_probe_retry_sleep --exact",
    "progress_events_ --nocapture",
    "python scripts/qualification/native.py --package artifacts/package --report artifacts/native-evidence.json",
    "./scripts/build-package.ps1 -Output artifacts/repeated-package -Rebuild",
    "python scripts/compare-packages.py artifacts/package artifacts/repeated-package artifacts/reproducibility.json",
    "python scripts/test-package-install.py --package artifacts/package --report artifacts/install-evidence.json",
    "name: windows-x64-candidate",
  ];
  let previous = -1;
  for (const step of steps) {
    const position = packaging.indexOf(step);
    assert.ok(position > previous, `Missing or reordered package gate: ${step}`);
    previous = position;
  }
  assert.equal(packaging.match(/1\.\.10 \| ForEach-Object/gu)?.length, 2);
  assert.match(packaging, /if-no-files-found: error/u);
  assert.doesNotMatch(packaging, /download-artifact|always\(\)/u);
});

test("emulation waits for quality and the same-run qualified package", () => {
  const emulation = blocks.get("windows11-emulation");
  assert.match(emulation, /needs: \[quality, package\]/u);
  assert.match(emulation, /timeout-minutes: 15/u);
  assert.match(emulation, /name: windows-x64-candidate/u);
  assert.doesNotMatch(emulation, /run-id:|repository:|always\(\)|continue-on-error:/u);
  assert.match(blocks.get("rust-dependencies"), /command: check/u);
});
