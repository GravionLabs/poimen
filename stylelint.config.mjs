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
  overrides: allow.stylelint.map((entry) => helixAllow(entry)),
};
