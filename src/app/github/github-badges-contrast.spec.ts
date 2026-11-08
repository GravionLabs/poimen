import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { bundled, contrast, deriveTokens, mixSrgb } from "../core/color-scheme";

// github-badges.css: a badge tints its tone color over the content background; the label keeps the text color.
const css = readFileSync("src/app/github/github-badges.css", "utf8");
const m = css.match(/color-mix\(in srgb, var\(--badge-tone, var\([^)]*\)\) (\d+)%, transparent\)/);
if (!m) throw new Error("badge tint missing");
const tint = Number(m[1]) / 100;

describe.each(["poimen-light", "poimen-dark", "dracula", "one-light", "tokyo-night"])(
  "badge labels in %s",
  (name) => {
    const scheme = bundled.get(name);
    if (!scheme) throw new Error(`scheme ${name} missing`);
    const t = deriveTokens(scheme);

    it.each(["--h-primary-color", "--h-text-muted-color"])("keep AA on a %s tint", (tone) => {
      const bg = mixSrgb(t[tone], t["--h-content-background"], tint);
      expect(contrast(t["--h-text-color"], bg)).toBeGreaterThanOrEqual(4.5);
    });

    it("hard-codes no color", () => {
      expect(css).not.toMatch(/#[0-9a-f]{3,8}\b|rgba?\(/i);
    });
  },
);
