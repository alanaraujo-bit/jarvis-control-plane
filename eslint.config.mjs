// Fast lint tier for the TypeScript in this workspace: no type information,
// so it stays quick. Type-aware async rules live in eslint.typed.config.mjs.
//
// Adopted from vibe-coding-toolkit (templates/eslint) and adapted: no Next.js,
// no Drizzle, no import boundaries (the UI talks to the core over Tauri IPC,
// not through imports). Rules that had legacy violations when adopted start at
// "warn" — see docs/quality.md for the counts and the promotion plan.
import js from "@eslint/js";
import { defineConfig, globalIgnores } from "eslint/config";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";

import quality from "./eslint-rules/index.cjs";

const sources = "{apps/desktop/src,apps/relay/src,apps/relay/api,apps/relay/test,apps/social-api/src,packages/*/src}/**/*.{ts,tsx}";
const browserSources = "{apps/desktop/src,packages/editor/src}/**/*.{ts,tsx}";
const nodeSources = "{apps/relay,apps/social-api,packages/i18n,packages/protocol}/**/*.ts";

export default defineConfig([
  {
    files: [sources],
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
  },
  { files: [browserSources], languageOptions: { globals: globals.browser } },
  { files: [nodeSources], languageOptions: { globals: globals.node } },
  {
    files: ["{apps/desktop/src,packages/editor/src}/**/*.tsx", "{apps/desktop/src,packages/editor/src}/**/*.ts"],
    extends: [reactHooks.configs.flat.recommended],
    rules: {
      // React Compiler-era rules with legacy violations at adoption
      // (2026-10-18: set-state-in-effect 24, refs 8, purity 2, immutability 1).
      // Warn until each count reaches zero, then delete the override.
      "react-hooks/set-state-in-effect": "warn",
      "react-hooks/refs": "warn",
      "react-hooks/purity": "warn",
      "react-hooks/immutability": "warn",
    },
  },
  {
    files: [sources],
    plugins: { quality },
    rules: {
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_", caughtErrors: "none" },
      ],
      // 350-line budget; 15 files over it at adoption. Warn, never baseline-ignore.
      "quality/max-lines": "warn",
    },
  },
  {
    // The i18n catalogues are data, not logic; their length is the number of
    // strings the product has.
    files: ["packages/i18n/src/{en,pt-BR}.ts"],
    rules: { "quality/max-lines": "off" },
  },
  {
    files: ["eslint-rules/**/*.cjs"],
    extends: [js.configs.recommended],
    languageOptions: { sourceType: "commonjs", globals: globals.node },
  },
  {
    files: ["scripts/**/*.mjs", "eslint*.config.mjs"],
    extends: [js.configs.recommended],
    languageOptions: { globals: globals.node },
  },
  globalIgnores([
    "**/node_modules/**",
    "**/dist/**",
    "**/target/**",
    "apps/desktop/src-tauri/**",
    "apps/docs/**",
    ".agents/**",
    ".claude/**",
    ".codex/**",
    "graphify-out/**",
    "**/*.d.ts",
  ]),
]);
