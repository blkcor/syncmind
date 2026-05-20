import tseslint from "typescript-eslint";
import solid from "eslint-plugin-solid/configs/typescript";
import globals from "globals";

/**
 * Shared ESLint flat config for SyncMind TS/Solid apps.
 * Consumers spread this in their own `eslint.config.js`.
 */
export default [
  {
    ignores: ["dist/**", "build/**", "node_modules/**", "src-tauri/**"],
  },
  ...tseslint.configs.recommended,
  {
    ...solid,
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      ...solid.languageOptions,
      globals: { ...globals.browser },
    },
    rules: {
      ...solid.rules,
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],
      "@typescript-eslint/no-explicit-any": "warn",
    },
  },
];
