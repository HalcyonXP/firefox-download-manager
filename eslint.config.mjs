import eslint from "@eslint/js";
import globals from "globals";
import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    ignores: ["coverage/**", "extension/dist/**", "node_modules/**", "target/**"],
  },
  eslint.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["extension/**/*.ts", "scripts/qualification/capture_probe/*.js"],
    languageOptions: {
      globals: {
        ...globals.browser,
        browser: "readonly",
      },
    },
  },
  {
    files: ["extension/protection-probe/api.js"],
    languageOptions: {
      sourceType: "script",
      globals: { ExtensionAPI: "readonly", Cc: "readonly", Ci: "readonly" },
    },
  },
  {
    files: ["extension/protection-probe/probe.js"],
    languageOptions: { sourceType: "script", globals: { ...globals.browser, browser: "readonly" } },
  },
  {
    files: ["scripts/qualification/protection_loader.js"],
    languageOptions: {
      sourceType: "script",
      globals: { ChromeUtils: "readonly", Components: "readonly", arguments: "readonly" },
    },
  },
  {
    files: ["scripts/**/*.mjs", "*.config.mjs"],
    languageOptions: {
      globals: globals.node,
    },
  },
);
