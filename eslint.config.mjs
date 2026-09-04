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
    files: ["extension/**/*.ts"],
    languageOptions: {
      globals: {
        ...globals.browser,
        browser: "readonly",
      },
    },
  },
  {
    files: ["scripts/**/*.mjs", "*.config.mjs"],
    languageOptions: {
      globals: globals.node,
    },
  },
);
