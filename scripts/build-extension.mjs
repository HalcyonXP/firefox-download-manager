import { copyFile, mkdir } from "node:fs/promises";

import { build } from "esbuild";

await mkdir("extension/dist", { recursive: true });

await build({
  bundle: true,
  entryPoints: ["extension/src/background.ts"],
  format: "iife",
  legalComments: "none",
  minify: false,
  outfile: "extension/dist/background.js",
  platform: "browser",
  sourcemap: true,
  target: "firefox128",
});

await copyFile("extension/src/manifest.json", "extension/dist/manifest.json");
