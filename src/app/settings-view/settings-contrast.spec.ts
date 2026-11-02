import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { bundled, contrast, deriveTokens, mixSrgb } from "../core/color-scheme";

// settings-view.css: hover and selected states mix the text color over the content background.
const css = readFileSync("src/app/settings-view/settings-view.css", "utf8");
const share = (selector: string): number => {
  const at = css.indexOf(selector);
  if (at < 0) throw new Error(`${selector} missing`);
  const block = css.slice(at);
  const m = block.match(/color-mix\(in srgb, var\(--h-text-color\) (\d+)%, transparent\)/);
  if (!m) throw new Error(`${selector}: no text-color tint`);
  return Number(m[1]) / 100;
};

describe.each(["poimen-light", "poimen-dark", "dracula", "one-light", "tokyo-night"])(
  "settings selection in %s",
  (name) => {
    const scheme = bundled.get(name);
    if (!scheme) throw new Error(`scheme ${name} missing`);
    const t = deriveTokens(scheme);
    const text = t["--h-text-color"];
    const base = t["--h-content-background"];

    it.each([".option.on", ".option:focus-visible", ".choice.on"])(
      "%s keeps the label at AA",
      (selector) => {
        const tint = mixSrgb(text, base, share(selector));
        expect(contrast(text, tint)).toBeGreaterThanOrEqual(4.5);
      },
    );

    it("uses no surface step for hover or selection", () => {
      expect(css).not.toMatch(/--h-surface-/);
    });
  },
);
