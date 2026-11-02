import catppuccin from "../themes/catppuccin.json";
import dracula from "../themes/dracula.json";
import gruvbox from "../themes/gruvbox.json";
import gruvboxLight from "../themes/gruvbox-light.json";
import kanagawa from "../themes/kanagawa.json";
import kanagawaLotus from "../themes/kanagawa-lotus.json";
import oneDark from "../themes/one-dark.json";
import oneLight from "../themes/one-light.json";
import poimenDark from "../themes/poimen-dark.json";
import poimenLight from "../themes/poimen-light.json";
import rosePine from "../themes/rose-pine.json";
import rosePineDawn from "../themes/rose-pine-dawn.json";
import solarized from "../themes/solarized.json";
import solarizedLight from "../themes/solarized-light.json";
import tokyoNight from "../themes/tokyo-night.json";
import tokyoNightDay from "../themes/tokyo-night-day.json";
import vesper from "../themes/vesper.json";

/** A terminal color scheme; the chrome is derived from it (`deriveTokens`). */
export interface ColorScheme {
  /** Identifier used by `colorSchema`. */
  name: string;
  label: string;
  /** `#rrggbb` colors. */
  background: string;
  foreground: string;
  cursor: string;
  selectionBackground: string;
  selectionForeground: string;
  /** The 16 ANSI colors, `ansi[5]` is magenta (the accent of the chrome). */
  ansi: string[];
  /** `--h-*` / `--status-*` token values that replace a derived token where the derivation fails. */
  chrome?: Record<string, string>;
}

export type Appearance = "light" | "dark";

/** The scheme used when `colorSchema` is missing or unknown: poimen's own light and dark colors. */
export const DEFAULT_SCHEME = "poimen";

const list: ColorScheme[] = [
  poimenLight,
  poimenDark,
  catppuccin,
  tokyoNight,
  tokyoNightDay,
  dracula,
  gruvbox,
  gruvboxLight,
  oneDark,
  oneLight,
  solarized,
  solarizedLight,
  kanagawa,
  kanagawaLotus,
  rosePine,
  rosePineDawn,
  vesper,
];

/** The bundled schemes by name; user schemes are merged over it (same name wins). */
export const bundled: ReadonlyMap<string, ColorScheme> = new Map(list.map((s) => [s.name, s]));

/**
 * The scheme a `colorSchema` name selects. `null` means the default look: `null`, `poimen`, or an
 * unknown name (which also warns), so the CSS tokens of `styles.css` stay in charge.
 */
export function resolveScheme(
  name: string | null,
  registry: ReadonlyMap<string, ColorScheme> = bundled,
  warn: (message: string) => void = (m) => console.warn(m),
): ColorScheme | null {
  if (name === null || name === DEFAULT_SCHEME) return null;
  const scheme = registry.get(name);
  if (scheme) return scheme;
  warn(`poimen settings: unknown colorSchema "${name}"; using ${DEFAULT_SCHEME}`);
  return null;
}

// --- color math ------------------------------------------------------------------------------

type Rgb = [number, number, number];
type Lab = [number, number, number];

export function parseHex(hex: string): Rgb {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) throw new Error(`invalid color ${hex}`);
  const n = parseInt(m[1], 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

export function toHex(rgb: Rgb): string {
  return (
    "#" +
    rgb
      .map((c) =>
        Math.min(255, Math.max(0, Math.round(c)))
          .toString(16)
          .padStart(2, "0"),
      )
      .join("")
  );
}

const toLinear = (c: number) => {
  const v = c / 255;
  return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
};
const fromLinear = (v: number) =>
  255 * (v <= 0.0031308 ? 12.92 * v : 1.055 * Math.max(v, 0) ** (1 / 2.4) - 0.055);

function toLab(hex: string): Lab {
  const [r, g, b] = parseHex(hex).map(toLinear);
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
}

function fromLab([L, a, b]: Lab): string {
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
  return toHex([
    fromLinear(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
    fromLinear(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
    fromLinear(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s),
  ]);
}

/** Interpolates from `a` to `b` in OKLab (`t` 0 gives `a`, 1 gives `b`). */
export function mix(a: string, b: string, t: number): string {
  if (t <= 0) return a.toLowerCase();
  if (t >= 1) return b.toLowerCase();
  const [x, y] = [toLab(a), toLab(b)];
  return fromLab([x[0] + (y[0] - x[0]) * t, x[1] + (y[1] - x[1]) * t, x[2] + (y[2] - x[2]) * t]);
}

/** Mixes in sRGB like CSS `color-mix(in srgb, ...)`: `share` of `a` over `b`. */
export function mixSrgb(a: string, b: string, share: number): string {
  const [x, y] = [parseHex(a), parseHex(b)];
  return toHex([0, 1, 2].map((i) => x[i] * share + y[i] * (1 - share)) as Rgb);
}

/** WCAG relative luminance. */
export function luminance(hex: string): number {
  const [r, g, b] = parseHex(hex).map(toLinear);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG contrast ratio. */
export function contrast(a: string, b: string): number {
  const [x, y] = [luminance(a), luminance(b)];
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
}

/** Light when the background is brighter than the luminance where black and white tie. */
export function appearance(scheme: ColorScheme): Appearance {
  return luminance(scheme.background) > 0.179 ? "light" : "dark";
}

/** `color`, moved toward white (dark backgrounds) or black until it reaches `min` on all `backgrounds`. */
function ensureContrast(color: string, backgrounds: string[], min: number): string {
  const ok = (c: string) => backgrounds.every((bg) => contrast(c, bg) >= min);
  if (ok(color)) return color.toLowerCase();
  const mean = backgrounds.reduce((sum, bg) => sum + luminance(bg), 0) / backgrounds.length;
  const toward = mean > 0.179 ? "#000000" : "#ffffff";
  for (let t = 0.02; t < 1; t += 0.02) {
    const c = mix(color, toward, t);
    if (ok(c)) return c;
  }
  return toward;
}

// --- chrome derivation -----------------------------------------------------------------------

export const SURFACE_STEPS = [0, 50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950] as const;
/** Share of the way from the lighter to the darker end of the ramp. */
const SURFACE_SHARE: Record<number, number> = {
  0: 0,
  50: 0.04,
  100: 0.08,
  200: 0.14,
  300: 0.28,
  400: 0.48,
  500: 0.68,
  600: 0.82,
  700: 0.88,
  800: 0.93,
  900: 0.97,
  950: 1,
};
const PRIMARY_STEPS = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950] as const;
/** Share of the accent that is mixed toward the light end (below 500) or the dark end (above). */
const PRIMARY_SHARE: Record<number, number> = {
  50: 0.88,
  100: 0.78,
  200: 0.6,
  300: 0.4,
  400: 0.2,
  500: 0,
  600: 0.15,
  700: 0.3,
  800: 0.45,
  900: 0.6,
  950: 0.75,
};

/** The share of the sidebar's active-row tint (`sidebar.css`): primary over the content background. */
const ACTIVE_TINT = 0.28;
const AA = 4.5;

/** Background of the tinted active sidebar row. */
export function activeRowTint(tokens: Record<string, string>): string {
  return mixSrgb(tokens["--h-primary-color"], tokens["--h-content-background"], ACTIVE_TINT);
}

/**
 * The CSS variables of a scheme: the helix surface ramp and accent, text, the status colors, and
 * the terminal colors. Pure and deterministic; `scheme.chrome` replaces derived tokens.
 *
 * The ramp always runs from the lightest (`--h-surface-0`) to the darkest step (`-950`), as in
 * helix, so a dark scheme starts at its foreground and ends at its background.
 */
export function deriveTokens(scheme: ColorScheme): Record<string, string> {
  const dark = appearance(scheme) === "dark";
  const [lightEnd, darkEnd] = dark
    ? [scheme.foreground, scheme.background]
    : [scheme.background, scheme.foreground];
  const chrome = scheme.chrome ?? {};
  const t: Record<string, string> = {};

  for (const step of SURFACE_STEPS)
    t[`--h-surface-${step}`] = mix(lightEnd, darkEnd, SURFACE_SHARE[step]);

  const accent = scheme.ansi[5];
  for (const step of PRIMARY_STEPS) {
    const share = PRIMARY_SHARE[step];
    t[`--h-primary-${step}`] =
      step < 500 ? mix(accent, lightEnd, share) : mix(accent, darkEnd, share);
  }
  const [main, hover, active] = dark ? [400, 300, 200] : [600, 700, 800];
  t["--h-primary-color"] = t[`--h-primary-${main}`];
  t["--h-primary-hover-color"] = t[`--h-primary-${hover}`];
  t["--h-primary-active-color"] = t[`--h-primary-${active}`];
  t["--h-content-background"] = dark ? t["--h-surface-900"] : t["--h-surface-0"];
  t["--app-ground"] = dark ? t["--h-surface-950"] : t["--h-surface-50"];
  Object.assign(t, chrome);

  const primary = t["--h-primary-color"];
  t["--h-primary-contrast-color"] =
    contrast(scheme.background, primary) >= contrast(scheme.foreground, primary)
      ? scheme.background
      : scheme.foreground;
  const tint = activeRowTint(t);
  const grounds = [t["--h-content-background"], t["--app-ground"], tint];
  t["--h-text-color"] = ensureContrast(scheme.foreground, grounds, AA);
  t["--h-text-hover-color"] = t["--h-text-color"];
  t["--h-text-muted-color"] = ensureContrast(
    mix(scheme.background, scheme.foreground, 0.62),
    grounds,
    AA,
  );

  t["--status-idle"] = t["--h-surface-400"];
  t["--status-unknown"] = dark ? t["--h-surface-600"] : t["--h-surface-300"];
  const status = { working: scheme.ansi[4], blocked: scheme.ansi[3], done: scheme.ansi[2] };
  for (const [name, color] of Object.entries(status)) {
    t[`--status-${name}`] = color;
    t[`--status-${name}-on-active`] = ensureContrast(color, [tint], AA);
  }

  t["--term-bg"] = scheme.background;
  t["--term-fg"] = scheme.foreground;
  t["--term-cursor"] = scheme.cursor;
  t["--term-selection-bg"] = scheme.selectionBackground;
  t["--term-selection-fg"] = scheme.selectionForeground;
  scheme.ansi.forEach((c, i) => (t[`--term-ansi-${i}`] = c));

  return { ...t, ...chrome };
}
