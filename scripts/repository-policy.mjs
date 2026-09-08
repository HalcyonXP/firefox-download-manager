export const repository = "HalcyonXP/firefox-download-manager";
export const repositoryUrl = `https://github.com/${repository}`;

// Assemble the retired identifier so the guard's own source contains no stale link.
const retired = ["HalcyonXP", "download-manager"].join("/").toLowerCase();

export function repositoryFindings(text) {
  return text
    .split(/\r?\n/u)
    .flatMap((line, index) =>
      line.toLowerCase().includes(retired)
        ? [{ line: index + 1, category: "retired-repository-reference" }]
        : [],
    );
}
