import { readFileSync } from "node:fs";
import { helixAllow } from "@gravionlabs/helix-lint/stylelint";

/**
 * Stylelint runs the helix-lint preset over the component styles and `src/styles.css` (#1367):
 * no hex, rgb or hsl colors, font sizes only from the type scale. Exceptions go in
 * `scripts/lint/helix-allow.json`, each with a reason.
 */
const allow = JSON.parse(
  readFileSync(new URL("./scripts/lint/helix-allow.json", import.meta.url), "utf8"),
);

export default {
  extends: ["@gravionlabs/helix-lint/stylelint"],
  ignoreFiles: ["dist/**", "site/**", ".angular/**", "node_modules/**", "src-tauri/**"],
  overrides: [
    {
      // #1365: component styles (src/app) do not style native controls; the global src/styles.css sizes
      // the Helix controls (`button.hx-button`) and lays out labels around them, so it is not covered.
      files: ["src/app/**/*.css", "src/testing/lint-probe/**/*.css"],
      rules: {
        // #1365: no control look in component styles; a control is styled by its Helix directive.
        // (Stylelint has no type-selector list rule; the regex matches the element as a compound's type.)
        "selector-disallowed-list": [
          [/(^|[\s>+~,])(button|input|select|textarea|progress)(?![\w-])/],
          {
            message:
              "A component style does not style native controls; use the Helix directive (#1365)",
          },
        ],
      },
    },
    ...allow.stylelint.map((entry) => helixAllow(entry)),
  ],
};
