// Type-aware tier, kept out of eslint.config.mjs because it builds a full
// TypeScript program per package. Only the two async-correctness rules from
// the toolkit's focused profile; counts and runtime are in docs/quality.md.
import tseslint from "typescript-eslint";

import defaultConfig from "./eslint.config.mjs";

export default [
  ...defaultConfig,
  {
    files: ["{apps/desktop/src,apps/relay/src,apps/relay/api,apps/social-api/src,packages/*/src}/**/*.{ts,tsx}"],
    languageOptions: {
      parser: tseslint.parser,
      parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname },
    },
    plugins: { "@typescript-eslint": tseslint.plugin },
    rules: {
      "@typescript-eslint/no-floating-promises": "warn",
      "@typescript-eslint/no-misused-promises": "warn",
    },
  },
];
