import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// #850: --h-surface-200 is a light gray in dark schemes, so the selected file row with the light text
// color was unreadable. The row uses the hover background, which helix defines per theme.
const tokens = readFileSync("node_modules/@gravionlabs/helix-ui/tokens.css", "utf8");
const view = readFileSync("src/app/changes/changes-view.ts", "utf8");

describe("Changes view selected row", () => {
  it("does not use a fixed light surface", () => {
    expect(view).not.toContain("--h-surface-200");
    expect(view).toMatch(/\.file\.selected\s*\{\s*background: var\(--h-content-hover-background\)/);
  });

  it("has a hover background in the light and the dark theme", () => {
    const dark = tokens.slice(tokens.indexOf(".app-dark"));
    expect(tokens).toContain("--h-content-hover-background:");
    expect(dark).toContain("--h-content-hover-background:var(--h-surface-hover)");
  });
});
