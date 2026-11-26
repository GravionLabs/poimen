import { readFileSync } from "node:fs";
import { mixSrgb, toHex } from "../core/color-scheme";

/** Reads CSS tokens and resolves them the way the browser would, for the contrast specs. */
function declarations(css: string, block: RegExp): Record<string, string> {
  const out: Record<string, string> = {};
  for (const m of css.matchAll(block)) {
    for (const d of m[1].matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) out[d[1]] = d[2].trim();
  }
  return out;
}

/**
 * The variables of the light and the dark theme: helix tokens, then src/styles.css (the order of
 * angular.json), whose `:root` block has the specificity of helix's `.app-dark` and wins by position,
 * and `:root.app-dark` wins over both.
 */
export function helixTokens() {
  const helix = readFileSync("node_modules/@gravionlabs/helix-ui/tokens.css", "utf8");
  const styles = readFileSync("src/styles.css", "utf8");
  const helixLight = declarations(helix, /:root,:host\{([^}]*)\}/g);
  const helixDark = declarations(helix, /\.app-dark\{([^}]*)\}/g);
  const appLight = declarations(styles, /:root\s*\{([^}]*)\}/g);
  const appDark = declarations(styles, /:root\.app-dark\s*\{([^}]*)\}/g);
  return {
    light: { ...helixLight, ...appLight } as Record<string, string>,
    dark: { ...helixLight, ...helixDark, ...appLight, ...appDark } as Record<string, string>,
  };
}

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
export function paint(value: string, vars: Record<string, string>, ground: string): string {
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
