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
  assert.match(rust, /if !matches!\(operation, "create" \| "verify"\)/u);
  assert.match(rust, /validate_text\(path\)\?;/u);
  assert.match(rust, /format!\("\{operation\}\\n\{\}\\n", path\.to_str\(\)\.ok_or\(ERROR\)\?\)/u);
  assert.match(rust, /if &consumed != b"input\\n"/u);
  assert.doesNotMatch(rust + script, /ConvertFrom-Json|Get-Acl|Set-Acl|Import-Module/u);
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
  assert.doesNotMatch(script, /\$request\b/iu);
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
  const diagnostics = [...rust.matchAll(/^\s+trace_phase\(([^;]+)\);/gmu)];
  assert.ok(diagnostics.length > 0);
  assert.ok(diagnostics.every((match) => /^"[a-z ]+"$/u.test(match[1])));
  assert.equal(rust.match(/#\[cfg\(test\)\]\s+trace_phase\(/gu)?.length, diagnostics.length);
  assert.match(rust, /#\[cfg\(test\)\]\s+fn trace_phase\(phase: &'static str\)/u);
  assert.match(
    rust,
    /let elapsed = START\.get_or_init\(Instant::now\)\.elapsed\(\)\.as_millis\(\);/u,
  );
  assert.deepEqual(
    [...rust.matchAll(/eprintln!\(([^;]+)\);/gu)].map((match) => match[1]),
    ['"private adapter: {phase} at +{elapsed} ms"'],
  );
  // Structural regression guard only, not ACL/cross-account/runtime proof.
});

test("protected-directory creation keeps exclusive ownership and whole-ACL SDK inputs", () => {
  const rust = text("crates/setup/src/private_directory.rs").split(
    "\n#[cfg(test)]\nmod tests {",
  )[0];
  const script = text("crates/setup/src/private_directory.ps1");
  assert.match(rust, /Self::create_with_script\(parent, SCRIPT\)/u);
  assert.match(
    rust,
    /winsafe::CreateDirectory\(path\.to_str\(\)\.ok_or\(ERROR\)\?, Some\(&attributes\)\)\.map_err/u,
  );
  assert.match(rust, /std::ptr::from_mut\(&mut acl\)\.cast::<winsafe::ACL>\(\)/u);
  assert.doesNotMatch(
    rust,
    /from_mut\(&mut acl\.header\)|unsafe\s*\{|remove_dir_all|create_dir_all/u,
  );
  assert.match(rust, /DACL_PRESENT \| winsafe::co::SE::DACL_PROTECTED/u);
  assert.match(rust, /let retained = DirectoryLease::open\(&path\)\?;/u);
  const creation = rust.indexOf("create_readonly(&path, &user)?");
  const retention = rust.indexOf("let retained =");
  const grant = rust.indexOf('Adapter::start_script("create"');
  assert.ok(creation >= 0 && creation < retention && retention < grant);
  assert.match(script, /0x500d0152/u);
  assert.match(script, /0x500d0112/u);
  assert.match(script, /0x120089/u);
  assert.match(script, /0x1f01ff/u);
  assert.match(script, /DiscretionaryAclProtected/u);
  assert.match(script, /AccessControlSections\]::Access\)/u);
  assert.doesNotMatch(
    script,
    /ConvertFrom-Json|Get-Acl|Set-Acl|Import-Module|DllImport|Marshal|Add-Type|Invoke-Expression|Start-Process|Start-Job|Set-ExecutionPolicy/u,
  );
});

test("installed runtime binding remains read-only, bounded and separate from engine authority", () => {
  const image = text("crates/setup/src/installed_image.rs").split("\n#[cfg(test)]")[0];
  const record = text("crates/setup/src/runtime_record.rs").split("\n#[cfg(test)]")[0];
  const lock = text("crates/setup/src/files.rs")
    .split("pub(crate) fn open_existing")[1]
    .split("pub(crate) fn acquire")[0];
  assert.match(image, /std::env::current_exe\(\)/u);
  assert.match(image, /SetupLock::open_existing/u);
  assert.match(image, /let mut immutable = vec!\[receipt_file, helper, extension, manifest\]/u);
  assert.match(image, /immutable.push\(link\)/u);
  assert.match(image, /_files: immutable/u);
  assert.doesNotMatch(image, /replace_if_unchanged|fs::write|create_dir_all|create_new/u);
  assert.doesNotMatch(lock, /\.create\(|\.create_new\(|\.truncate\(|\.write\(/u);
  assert.match(record, /server: Server/u);
  assert.match(record, /let server = Server::bind/u);
  assert.match(record, /pub const fn server\(&self\) -> &Server/u);
  assert.match(record, /MAX_ROOT_ENTRIES: usize = 64/u);
  assert.match(record, /stored.binding != image.binding/u);
  assert.match(record, /stored.endpoint != endpoint.id\(\)/u);
  assert.match(record, /bytes.len\(\) > LIMIT/u);
  assert.doesNotMatch(record, /eprintln!|println!|remove_dir_all|Command::new|EngineOwner::open/u);
  // Source guards complement actual file/pipe/mutation tests, not installed proof.
});

test("paired setup keeps application entry checking ahead of normal-state launch", () => {
  const ui = readFileSync(
    new URL("../crates/setup/src/application_ui.rs", import.meta.url),
    "utf8",
  );
  const probe = ui.indexOf("probe_application(&image.executable(), &config.local)?;");
  const launch = ui.indexOf("Command::new(image.executable())");
  assert.ok(probe >= 0 && launch > probe);
  assert.ok(ui.includes("require_apps_closed()?;"));
  assert.ok(ui.indexOf("drop(session);") < probe);
  assert.doesNotMatch(ui, /\.kill\(/u);
  assert.ok(
    ui.indexOf("self.launched.borrow_mut().push(child);") <
      ui.indexOf("Owned Manager process: {id}"),
  );
  assert.match(ui, /if joined && self\.launched\.borrow\(\)\.is_empty\(\)/u);
  assert.match(ui, /Manager exit observed; retained child joined\./u);
});
