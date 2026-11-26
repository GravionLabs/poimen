import { describe, expect, it } from "vitest";
import { appearance, bundled, contrast, deriveTokens } from "../core/color-scheme";
import { helixTokens, paint } from "./css-paint.mock";

/**
 * The states of `hx-list-row` (#1363) in every bundled scheme: text reaches AA on the base, hover,
 * selected and current backgrounds, and the focus ring is visible. Values come from helix-ui's
 * tokens.css, `src/styles.css` and the scheme's derived chrome.
 */
const { light, dark } = helixTokens();

const PAIRS = [
  ["base label", "color", null],
  ["base meta", "meta-color", null],
  ["hover", "hover-color", "hover-background"],
  ["current", "current-color", "current-background"],
  ["selected", "selected-color", "selected-background"],
  ["selected hover", "selected-hover-color", "selected-hover-background"],
] as const;

describe.each([...bundled.keys()])("list rows in %s", (name) => {
  const scheme = bundled.get(name)!;
  const derived = deriveTokens(scheme);
  const vars = { ...(appearance(scheme) === "dark" ? dark : light), ...derived };
  const ground = derived["--h-content-background"];
  const token = (key: string, on: string) => paint(`var(--h-list-row-${key})`, vars, on);

  it.each(PAIRS)("%s: text at AA", (_, fg, bg) => {
    const back = bg ? token(bg, ground) : ground;
    expect(contrast(token(fg, back), back)).toBeGreaterThanOrEqual(4.5);
  });

  it("shows the focus ring against the surface", () => {
    expect(contrast(token("focus-ring-color", ground), ground)).toBeGreaterThanOrEqual(3);
  });
});
