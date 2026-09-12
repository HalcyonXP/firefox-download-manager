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
    files: ["extension/protection-bridge/*.js"],
    languageOptions: {
      globals: { URL: "readonly", TextEncoder: "readonly", TextDecoder: "readonly" },
    },
  },
  {
    files: ["extension/protection-probe/api.js"],
    languageOptions: {
      sourceType: "script",
      globals: {
        ExtensionAPI: "readonly",
        Cc: "readonly",
        Ci: "readonly",
        ChromeUtils: "readonly",
      },
    },
  },
  {
    files: ["extension/parent-probe/api.js"],
    languageOptions: {
      globals: {
        ExtensionAPI: "readonly",
        Cu: "readonly",
        ChromeUtils: "readonly",
        Services: "readonly",
        PathUtils: "readonly",
        __OWNED_FIXTURE_NONCE__: "readonly",
        __OWNED_FIXTURE_COMMAND__: "readonly",
        __OWNED_FIXTURE_MANIFEST__: "readonly",
      },
    },
  },
  {
    files: ["extension/protection-probe/probe.js", "extension/parent-probe/background.js"],
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
    files: ["scripts/qualification/parent_observer.js"],
    languageOptions: {
      sourceType: "script",
      globals: { Services: "readonly", arguments: "readonly" },
    },
  },
  {
    files: ["scripts/**/*.mjs", "*.config.mjs"],
    languageOptions: {
      globals: globals.node,
    },
  },
);
