import { rm } from "node:fs/promises";

await Promise.all([
  rm("coverage", { force: true, recursive: true }),
  rm("extension/dist", { force: true, recursive: true }),
]);
