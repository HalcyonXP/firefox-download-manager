import { readFile } from "node:fs/promises";

// Fixed, reviewed inputs only. Package/XPI redistribution must retain both notices.
export async function extensionLicenses() {
  const normalize = (text) => text.replaceAll("\r\n", "\n");
  return {
    "LICENSE.txt": normalize(await readFile("LICENSE", "utf8")),
    "THIRD-PARTY-NOTICES.txt":
      "Firefox Download Manager extension — third-party notices\n\n" +
      "First-party code is MIT licensed; see LICENSE.txt. Third-party terms remain independent.\n\n" +
      "esbuild generated-code/build-tool notice (MIT):\n\n" +
      normalize(await readFile("node_modules/esbuild/LICENSE.md", "utf8")),
  };
}
