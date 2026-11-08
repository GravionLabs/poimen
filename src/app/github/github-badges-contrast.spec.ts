import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { bundled, contrast, deriveTokens, mixSrgb } from "../core/color-scheme";

// github-badges.css: a badge tints its tone color over the content background; the label keeps the text color.
const css = readFileSync("src/app/github/github-badges.css", "utf8");
const m = css.match(/color-mix\(in srgb, var\(--badge-tone, var\([^)]*\)\) (\d+)%, transparent\)/);
if (!m) throw new Error("badge tint missing");
const tint = Number(m[1]) / 100;
const icon = css.match(
  /\.check > \[aria-hidden\][^}]*color-mix\(in srgb, var\(--check-tone\) (\d+)%/,
);
if (!icon) throw new Error("check icon mix missing");
const iconShare = Number(icon[1]) / 100;

describe.each(["poimen-light", "poimen-dark", "dracula", "one-light", "tokyo-night"])(
  "badge labels in %s",
  (name) => {
    const scheme = bundled.get(name);
    if (!scheme) throw new Error(`scheme ${name} missing`);
    const t = deriveTokens(scheme);

    // Tones as the CSS resolves them: --h-primary-color, muted text, and the ANSI colors
    // (--term-ansi-2 success, -1 danger, -3 warning, -5 epic).
    const tones: Record<string, string> = {
      primary: t["--h-primary-color"],
      muted: t["--h-text-muted-color"],
      success: t["--term-ansi-2"],
      danger: t["--term-ansi-1"],
      warning: t["--term-ansi-3"],
      epic: t["--term-ansi-5"],
    };

    it.each(Object.keys(tones))("keeps AA for the label on a %s tint", (tone) => {
      const bg = mixSrgb(tones[tone], t["--h-content-background"], tint);
      expect(contrast(t["--h-text-color"], bg)).toBeGreaterThanOrEqual(4.5);
    });

    it.each(["success", "danger", "warning"])("keeps the %s check icon at 3:1", (tone) => {
      const icon = mixSrgb(tones[tone], t["--h-text-color"], iconShare);
      expect(contrast(icon, t["--h-content-background"])).toBeGreaterThanOrEqual(3);
    });

    it("keeps the check count in the text color at AA", () => {
      expect(contrast(t["--h-text-color"], t["--h-content-background"])).toBeGreaterThanOrEqual(
        4.5,
      );
    });

    it("hard-codes no color", () => {
      expect(css).not.toMatch(/#[0-9a-f]{3,8}\b|rgba?\(/i);
    });
  },
);
