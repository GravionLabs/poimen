import js from "@eslint/js";
import tseslint from "typescript-eslint";
import angular from "angular-eslint";

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
    ],
  },
  {
    files: ["**/*.ts"],
    extends: [
      js.configs.recommended,
      ...tseslint.configs.recommended,
      ...angular.configs.tsRecommended,
    ],
    processor: angular.processInlineTemplates,
    rules: {
      "@angular-eslint/directive-selector": [
        "error",
        { type: "attribute", prefix: "app", style: "camelCase" },
      ],
      "@angular-eslint/component-selector": [
        "error",
        { type: "element", prefix: "app", style: "kebab-case" },
      ],
    },
  },
  {
    // #1051: product templates live in .html files, so the template rules see every one of them.
    files: ["src/**/*.ts"],
    ignores: ["**/*.spec.ts"],
    rules: {
      "no-restricted-syntax": [
        "error",
        {
          selector:
            "Decorator > CallExpression[callee.name='Component'] > ObjectExpression > Property[key.name='template']",
          message:
            "Put the component template in a sibling .html file and use templateUrl (#1051).",
        },
      ],
    },
  },
  {
    files: ["**/*.html"],
    extends: [...angular.configs.templateRecommended, ...angular.configs.templateAccessibility],
  },
);
