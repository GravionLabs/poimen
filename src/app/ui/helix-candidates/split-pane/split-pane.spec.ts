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
    expect(css).toMatch(/\.divider \{[^}]*border-left: 1px solid var\(--h-border-color\)/);
    expect(css).toMatch(/\.divider \{[^}]*border-right: 1px solid var\(--h-border-color\)/);
    expect(css).toMatch(/> \.divider \{[^}]*border-top: 1px solid var\(--h-border-color\)/);
    expect(css).toMatch(/> \.divider \{[^}]*border-bottom: 1px solid var\(--h-border-color\)/);
  });

  it("keeps the panes apart (no negative margin)", () => {
    expect(css).not.toMatch(/margin:\s*-/);
  });

  it("has a grip that turns --status-working on hover, focus and drag", () => {
    expect(css).toMatch(/\.divider::after \{[^}]*border-radius/);
    expect(css).toMatch(/\.divider\.dragging::after \{[^}]*var\(--status-working\)/);
    expect(css).toMatch(/\.divider\.dragging \{[^}]*border-color: var\(--status-working\)/);
  });
});
