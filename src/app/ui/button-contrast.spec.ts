import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { appearance, bundled, contrast, deriveTokens, mixSrgb, toHex } from "../core/color-scheme";

/**
 * Buttons in every bundled scheme (#844): the label of each variant the app uses reaches AA in its base,
 * hover and active state, and the focus ring and the disabled state are visible. Token values come from
 * helix-ui's tokens.css, `src/styles.css` (overrides) and the scheme's derived chrome, resolved the way
 * the browser would.
 */
const helix = readFileSync("node_modules/@gravionlabs/helix-ui/tokens.css", "utf8");
const styles = readFileSync("src/styles.css", "utf8");
const helixCss = readFileSync("node_modules/@gravionlabs/helix-ui/styles.css", "utf8");

function declarations(css: string, block: RegExp): Record<string, string> {
  const out: Record<string, string> = {};
  for (const m of css.matchAll(block)) {
    for (const d of m[1].matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) out[d[1]] = d[2].trim();
  }
  return out;
}

// Order of the stylesheets (angular.json): helix tokens, then src/styles.css, whose `:root` block has the
// specificity of helix's `.app-dark` and wins by position, and `:root.app-dark` wins over both.
const helixLight = declarations(helix, /:root,:host\{([^}]*)\}/g);
const helixDark = declarations(helix, /\.app-dark\{([^}]*)\}/g);
const appLight = declarations(styles, /:root\s*\{([^}]*)\}/g);
const appDark = declarations(styles, /:root\.app-dark\s*\{([^}]*)\}/g);
const light = { ...helixLight, ...appLight };
const dark = { ...helixLight, ...helixDark, ...appLight, ...appDark };

function resolve(value: string, vars: Record<string, string>, depth = 0): string {
  if (depth > 12) throw new Error(`cycle in ${value}`);
  const v = value.replace(/var\((--[\w-]+)\)/g, (_, name: string) => {
    const next = vars[name];
    if (next === undefined) throw new Error(`${name} undefined`);
    return resolve(next, vars, depth + 1);
  });
  return v.replace(/\s+/g, " ");
}

/** A color over `ground`: `#rrggbb`, or `color-mix(in srgb, X [p%], transparent)` (what the tokens use). */
function paint(value: string, vars: Record<string, string>, ground: string): string {
  const text = resolve(value, vars).replace(/\( /g, "(").replace(/ \)/g, ")");
  if (text === "transparent") return ground;
  const rgba = text.match(/^rgba\((\d+),(\d+),(\d+),([\d.]+)\)$/);
  if (rgba) {
    return mixSrgb(toHex([+rgba[1], +rgba[2], +rgba[3]]), ground, Number(rgba[4]));
  }
  const m = text.match(
    /^color-mix\(in srgb, (#[0-9a-f]{6})(?: (\d+)%)?, (?:transparent(?: (\d+)%)?|(#[0-9a-f]{6}))\)$/i,
  );
  if (!m) {
    if (/^#[0-9a-f]{6}$/i.test(text)) return text.toLowerCase();
    throw new Error(`cannot read ${text}`);
  }
  const share = m[2] ? Number(m[2]) / 100 : m[3] ? 1 - Number(m[3]) / 100 : 0.5;
  return mixSrgb(m[1], ground, share);
}

// Filled primary and danger, outlined secondary (the rule in AGENTS.md); text and outlined for all three.
const KINDS = [
  ["filled", "primary"],
  ["filled", "danger"],
  ["outlined", "primary"],
  ["outlined", "secondary"],
  ["outlined", "danger"],
  ["text", "primary"],
  ["text", "secondary"],
  ["text", "danger"],
] as const;
const SEVERITIES = ["primary", "danger"] as const;
const STATES = ["", "-hover", "-active"] as const;

describe.each([...bundled.keys()])("buttons in %s", (name) => {
  const scheme = bundled.get(name)!;
  const derived = deriveTokens(scheme);
  const base = appearance(scheme) === "dark" ? dark : light;
  const vars = { ...base, ...derived };
  const ground = derived["--h-content-background"];

  for (const [variant, severity] of KINDS) {
    {
      for (const state of STATES) {
        it(`${variant} ${severity}${state || " base"}: label at AA`, () => {
          let fg: string;
          let bg: string;
          if (variant === "filled") {
            fg = paint(`var(--h-button-${severity}${state}-color)`, vars, ground);
            bg = paint(`var(--h-button-${severity}${state}-background)`, vars, ground);
          } else {
            const kind = variant === "outlined" ? "outlined" : "text";
            fg = paint(`var(--h-button-${kind}-${severity}-color)`, vars, ground);
            bg =
              state === ""
                ? ground
                : paint(`var(--h-button-${kind}-${severity}${state}-background)`, vars, ground);
          }
          expect(contrast(fg, bg)).toBeGreaterThanOrEqual(4.5);
        });
      }
    }
  }

  it("shows the focus ring of filled buttons against the surface", () => {
    for (const severity of SEVERITIES) {
      const ring = paint(`var(--h-button-${severity}-focus-ring-color)`, vars, ground);
      expect(contrast(ring, ground), severity).toBeGreaterThanOrEqual(3);
    }
  });
});

describe("button states", () => {
  const rule = (selector: string): string => {
    const at = helixCss.indexOf(`\n  ${selector}`);
    if (at < 0) throw new Error(`${selector} missing in helix-ui`);
    return helixCss.slice(at, helixCss.indexOf("}", at));
  };

  it("has a focus-visible outline, a hover and a dimmed disabled state", () => {
    expect(rule(".hx-button:focus-visible")).toContain("outline:");
    expect(rule(".hx-button:not(:disabled, [aria-disabled=true]):hover")).toContain("background");
    expect(rule(".hx-button:disabled,\n  .hx-button[aria-disabled=true]")).toContain("opacity");
  });

  it("sizes buttons with the control token that fields use", () => {
    expect(styles).toMatch(/--control-height:\s*28px/);
    expect(styles).toMatch(/button\.hx-button\s*\{[^}]*min-height:\s*var\(--control-height\)/);
  });
});
