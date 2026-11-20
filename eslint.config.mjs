import tseslint from "typescript-eslint";
import angular from "angular-eslint";

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
);
