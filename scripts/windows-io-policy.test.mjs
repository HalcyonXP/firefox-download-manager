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
