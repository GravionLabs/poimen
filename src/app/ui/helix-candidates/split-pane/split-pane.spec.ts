import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { paneSizes } from "./split-pane";

describe("paneSizes", () => {
  it("uses the given sizes when they match the count", () => {
    expect(paneSizes([0.7, 0.3], 2)).toEqual([0.7, 0.3]);
  });

  it("falls back to equal shares", () => {
    expect(paneSizes(undefined, 4)).toEqual([0.25, 0.25, 0.25, 0.25]);
    expect(paneSizes([1], 2)).toEqual([0.5, 0.5]);
  });
});

describe("divider style", () => {
  const css = readFileSync(join(__dirname, "split-pane.css"), "utf8");

  it("draws the gutter as two 1px lines in the border color", () => {
    expect(css).toMatch(/\.divider \{[^}]*border-left: 1px solid var\(--divider-color\)/);
    expect(css).toMatch(/\.divider \{[^}]*border-right: 1px solid var\(--divider-color\)/);
    expect(css).toMatch(/> \.divider \{[^}]*border-top: 1px solid var\(--divider-color\)/);
    expect(css).toMatch(/> \.divider \{[^}]*border-bottom: 1px solid var\(--divider-color\)/);
  });

  it("keeps the panes apart (no negative margin)", () => {
    expect(css).not.toMatch(/margin:\s*-/);
  });

  it("changes only the --divider-color variable on hover, focus and drag", () => {
    // Lines and grip read the variable, so no direction-specific rule can out-rank the state.
    expect(css).toMatch(
      /\.divider:hover,\s*\.divider:focus-visible,\s*\.divider\.dragging \{[^}]*--divider-color: var\(--status-working\)/,
    );
    expect(css).not.toMatch(/\.divider[^{]*(hover|dragging)[^{]*\{[^}]*border-color/);
    // Derived from the terminal tokens: 50% foreground on --term-bg gives >= 3:1 in both bundled schemes.
    expect(css).toMatch(
      /\.divider \{[^}]*--divider-color: color-mix\(in srgb, var\(--term-fg\) 50%, var\(--term-bg\)\)/,
    );
    expect(css).toMatch(/> \.divider \{[^}]*border-top: 1px solid var\(--divider-color\)/);
    expect(css).toMatch(/> \.divider \{[^}]*border-bottom: 1px solid var\(--divider-color\)/);
    expect(css).toMatch(/\.divider::after \{[^}]*background: var\(--divider-color\)/);
    expect(css).toMatch(/\.divider::after \{[^}]*border-radius/);
  });
});
