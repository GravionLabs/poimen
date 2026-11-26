import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

/** The status bar keeps its warn and error colors across helix-desktop updates (#1360). */
describe("status bar tones", () => {
  const css = readFileSync("src/styles.css", "utf8");
  const helix = readFileSync("node_modules/@gravionlabs/helix-desktop/styles.css", "utf8");

  it.each([
    ["warn", "#b45309"],
    ["error", "#b91c1c"],
  ])("sets the %s color that helix-desktop reads", (tone, color) => {
    expect(css).toContain(`--h-${tone}-color: ${color};`);
    expect(helix).toContain(`var(--h-${tone}-color`);
  });
});
