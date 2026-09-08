import { copyFile, mkdir } from "node:fs/promises";

import { build } from "esbuild";

await mkdir("extension/dist", { recursive: true });

await build({
  bundle: true,
  entryPoints: ["extension/src/background.ts", "extension/src/manager.ts"],
  format: "iife",
  legalComments: "none",
  minify: false,
  outdir: "extension/dist",
  platform: "browser",
  sourcemap: true,
  target: "firefox128",
});

await copyFile("extension/src/manifest.json", "extension/dist/manifest.json");

for (const asset of ["manager.html", "manager.css"])
  await copyFile(`extension/src/${asset}`, `extension/dist/${asset}`);
