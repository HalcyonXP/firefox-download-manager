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

test("startup diagnostics run only after failed Rust gates and cannot qualify a candidate", () => {
  for (const [job, step, artifact] of [
    [quality, "rust_tests", "quality"],
    [packaging, "candidate", "package"],
  ]) {
    assert.match(job, new RegExp(`id: ${step}\\b`, "u"));
    const condition = `failure() && steps.${step}.outcome == 'failure'`;
    assert.equal(job.split(condition).length - 1, 2);
    assert.ok(
      job.includes(
        "./scripts/diagnose-private-startup.ps1 -Report artifacts/private-startup-diagnostics.json",
      ),
    );
    assert.ok(job.includes(`name: private-startup-${artifact}-diagnostics`));
  }
  const probe = readFileSync(new URL("./diagnose-private-startup.ps1", import.meta.url), "utf8");
  assert.match(probe, /qualification = \$false; version = 2/u);
  assert.ok(probe.includes('$bootstrap + "`nexit 1`ntry {`n" + $body'));
  assert.ok(probe.includes("startup-probe-refuse`nC:\\synthetic-unused`n"));
  assert.match(probe, /Length -gt 65536/u);
  assert.match(probe, /Read-Source 'private_file\.rs'/u);
  assert.match(probe, /Read-Source "private_\$kind\.ps1"/u);
  assert.equal(probe.match(/foreach \(\$kind in @\('file', 'directory'\)\)/gu)?.length, 2);
  assert.equal(probe.match(/\$cases \+=/gu)?.length, 3);
  for (const mode of ["'open'", "'closed'", "'after-marker'", "'prefed'"])
    assert.ok(probe.includes(`input = ${mode}`));
  assert.match(probe, /\$count -lt 6\) \{ 6 - \$count \} else \{ 1 \}/u);
  assert.match(probe, /\$process\.TotalProcessorTime\.TotalMilliseconds/u);
  assert.match(probe, /program_sha256 =/u);
  assert.match(probe, /if \(\$joinFailed\) \{ throw/u);
  assert.match(probe, /\[IO\.FileMode\]::CreateNew/u);
  assert.match(probe, /\[Environment\]::SystemDirectory/u);
  assert.match(probe, /5000 - \$clock\.ElapsedMilliseconds/u);
  assert.match(probe, /\[byte\[\]\]::new\(7\)/u);
  assert.match(probe, /\[byte\[\]\]::new\(512\)/u);
  assert.match(probe, /\$process\.Kill\(\)/u);
  assert.match(probe, /\$process\.WaitForExit\(\)/u);
  assert.match(probe, /\$pending\.GetAwaiter\(\)\.GetResult\(\)/u);
  assert.match(probe, /\$errors\.GetAwaiter\(\)\.GetResult\(\)/u);
  assert.doesNotMatch(
    probe,
    /Get-Process|Stop-Process|taskkill|SetAccessControl|Set-ExecutionPolicy|DllImport|Add-Type|Get-ChildItem|ReadToEnd/u,
  );
});

test("failed-child resource diagnostics stay query-only and test-only", () => {
  const source = readFileSync(
    new URL("../crates/setup/src/private_file.rs", import.meta.url),
    "utf8",
  );
  const query = source.match(/#\[cfg\(test\)\]\nfn child_resources\([\s\S]*?\n\}/u)?.[0];
  assert.ok(query);
  assert.match(query, /winsafe::HPROCESS::OpenProcess\(/u);
  assert.match(query, /winsafe::co::PROCESS::QUERY_LIMITED_INFORMATION,\s+false,\s+child\.id\(\)/u);
  assert.match(query, /process\.GetProcessId\(\)\? != child\.id\(\)/u);
  assert.match(query, /process\.GetProcessTimes\(\)/u);
  assert.match(query, /process\.GetProcessHandleCount\(\)/u);
  assert.doesNotMatch(
    query,
    /unsafe|from_ptr|as_raw_handle|TERMINATE|VM_READ|QueryFullProcessImageName/u,
  );
  assert.match(source, /#\[cfg\(test\)\]\s+trace_child_resources\(&self\.child\)/u);
  assert.match(source, /const EXECUTION_LIMIT: Duration = Duration::from_secs\(5\)/u);
  assert.match(source, /fn bootstrap_and_retained_resource_query_need_no_filesystem_access/u);
  assert.match(source, /fn pre_marker_stall_is_observed_and_retired_without_file_access/u);
});
