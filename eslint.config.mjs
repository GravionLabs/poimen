import tseslint from "typescript-eslint";
import angular from "angular-eslint";
import { readFileSync } from "node:fs";
import helix, { helixAllow } from "@gravionlabs/helix-lint/eslint";

// Exceptions to the helix-lint preset, each with a reason (#1367): scripts/lint/helix-allow.json.
const allow = JSON.parse(
  readFileSync(new URL("./scripts/lint/helix-allow.json", import.meta.url), "utf8"),
);

/**
 * ESLint lints the Angular templates only (oxlint cannot parse them yet, #1052). TypeScript,
 * including the Angular class rules, is linted by oxlint (`.oxlintrc.json`). Every product
 * template is a `.html` file (#1051), so no inline-template processor is needed.
 */
export default tseslint.config(
  {
    ignores: [
      "dist/**",
      "site/**",
      ".angular/**",
      "node_modules/**",
      "out-tsc/**",
      ".agents/**",
      ".claude/**",
      "src-tauri/**",
      // The lint probe holds deliberate violations; src/testing/lint-probe.spec.ts lints it.
      "src/testing/lint-probe/**",
    ],
  },
  {
    files: ["**/*.html"],
    extends: [...angular.configs.templateRecommended, ...angular.configs.templateAccessibility],
  },
  // helix/no-raw-control (#1367): no native button, input, select, textarea or progress in a template.
  ...helix,
  ...allow.eslint.map((entry) => helixAllow(entry)),
);
