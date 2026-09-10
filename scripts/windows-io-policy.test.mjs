import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import test from "node:test";

const root = fileURLToPath(new URL("../", import.meta.url));
const text = (path) => readFileSync(join(root, path), "utf8");

test("ADR0015 compiler exception remains confined to the reviewed Windows boundary", () => {
  assert.match(text("Cargo.toml"), /\[workspace\.lints\.rust\]\s+unsafe_code = "forbid"/u);
  for (const entry of readdirSync(join(root, "crates"), { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const manifest = text(`crates/${entry.name}/Cargo.toml`);
    if (entry.name === "windows-io") {
      assert.match(manifest, /\[lints\.rust\]\s+unsafe_code = "deny"/u);
    } else {
      assert.match(manifest, /\[lints\]\s+workspace = true/u, entry.name);
    }
  }
  const boundary = text("crates/windows-io/src/lib.rs");
  assert.equal(boundary.match(/#\[allow\(unsafe_code\)\]/gu)?.length, 1);
  assert.equal(boundary.match(/\bunsafe\s*\{/gu)?.length, 1);
  assert.match(
    boundary,
    /unsafe\s*\{\s*CancelIoEx\(handle\.as_raw_handle\(\), std::ptr::null\(\)\)\s*\}/u,
  );
  assert.match(boundary, /handle: BorrowedHandle<'_>/u);
  // Structural guard, not a proof of memory safety or completed cancellation.
  assert.match(text("crates/local-ipc/src/windows.rs"), /request_cancellation\(handle\)/u);
  assert.match(text("crates/local-ipc/src/windows.rs"), /io\.assume_flushed\(\)/u);
});

test("private-file adapter keeps secrets in Rust and preserves the OS boundary", () => {
  const rust = text("crates/setup/src/private_file.rs").split("\n#[cfg(test)]\nmod tests {")[0];
  const script = text("crates/setup/src/private_file.ps1");
  const request = rust.match(/serde_json::json!\(\{([\s\S]*?)\}\)/u)?.[1];
  assert.ok(request);
  assert.deepEqual(
    [...request.matchAll(/"([a-z_]+)"\s*:/gu)].map((match) => match[1]),
    ["operation", "path"],
  );
  assert.match(rust, /Self::start_script\(operation, path, SCRIPT\)/u);
  assert.match(rust, /winsafe::GetSystemDirectory\(\)/u);
  assert.match(rust, /Self::start_program\(operation, path, &script\)/u);
  assert.match(rust, /\.args\(\["-NoProfile", "-NonInteractive", "-Command", script\]\)/u);
  assert.match(rust, /\[IO\.StreamReader\]::new\(\[Console\]::OpenStandardInput\(\)/u);
  assert.match(rust, /if &start != b"start\\n"/u);
  assert.ok(rust.indexOf('if &start != b"start\\n"') < rust.indexOf("stdin.write_all(&input)"));
  assert.doesNotMatch(script, /\[Console\]::(?:In\b|InputEncoding)/u);
  assert.match(rust, /\.creation_flags\(0x0800_0000\)/u);
  assert.match(rust, /writer\.write_all\(bytes\)/u);
  assert.match(rust, /writer\.sync_all\(\)/u);
  assert.match(rust, /worker\.join\(\)/u);
  assert.match(rust, /self\.child\.wait\(\)/u);
  assert.doesNotMatch(script, /\$request\.(?!operation\b|path\b)\w+/iu);
  assert.doesNotMatch(
    script,
    /\b(?:DllImport|Marshal|Add-Type|Invoke-Expression|Invoke-Command|Start-Process|Start-Job|Set-ExecutionPolicy|WebClient|HttpClient)\b/iu,
  );
  assert.match(script, /\[IO\.FileMode\]::CreateNew/u);
  assert.match(script, /D:P\(A;;FA;;;/u);
  assert.match(script, /0x500d0156/u);
  assert.match(script, /DiscretionaryAclProtected/u);
  assert.match(script, /\[IO\.File\]::GetAccessControl/u);
  assert.match(script, /\[IO\.Directory\]::GetAccessControl/u);
  assert.match(script, /\$dmInput\.ReadLine\(\) -cne 'close'/u);
  const diagnostics = [...rust.matchAll(/eprintln!\(([^;]+)\);/gu)];
  assert.ok(diagnostics.length > 0);
  assert.ok(diagnostics.every((match) => /^"private adapter: [a-z ]+"$/u.test(match[1])));
  assert.equal(
    rust.match(/#\[cfg\(test\)\]\s+eprintln!/gu)?.length,
    diagnostics.length,
    "fixed phase diagnostics must remain test-only",
  );
  // Structural regression guard only, not ACL/cross-account/runtime proof.
});
