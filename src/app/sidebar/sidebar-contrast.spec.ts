import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// Reads the real helix tokens and checks the text pairs of the tinted active row (sidebar.css).
const tokens = readFileSync("node_modules/@gravionlabs/helix-ui/tokens.css", "utf8").replace(
  /\/\*[\s\S]*?\*\//g,
  "",
);
const light = new Map<string, string>();
const dark = new Map<string, string>();
for (const rule of tokens.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
  const target = rule[1].includes("app-dark") ? dark : light;
  for (const d of rule[2].matchAll(/(--[\w-]+)\s*:\s*([^;]+);?/g)) target.set(d[1], d[2].trim());
}

function resolve(theme: Map<string, string>, name: string): string {
  let value = theme.get(name) ?? light.get(name);
  const ref = value?.match(/^var\((--[\w-]+)\)$/);
  if (ref) value = resolve(theme, ref[1]);
  if (value === undefined) throw new Error(`token ${name} not found`);
  return value;
}

function rgb(hex: string): number[] {
  return [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
}

function luminance(c: number[]): number {
  const [r, g, b] = c.map((x) => {
    const v = x / 255;
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: number[], b: number[]): number {
  const [x, y] = [luminance(a), luminance(b)];
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
}

// The "-on-active" status tokens of styles.css: the var() target per theme.
const css = readFileSync("src/styles.css", "utf8");
function statusToken(name: string, dk: boolean): string {
  const split = css.indexOf(":root.app-dark");
  const block = dk ? css.slice(split) : css.slice(0, split);
  const m = block.match(new RegExp(`--status-${name}-on-active:\\s*var\\((--[\\w-]+)\\)`));
  if (!m) throw new Error(`--status-${name}-on-active missing`);
  return m[1];
}

describe.each([
  ["light", false],
  ["dark", true],
])("active sidebar row, %s theme", (_name, dk) => {
  const theme = dk ? new Map([...light, ...dark]) : light;
  const hex = (n: string) => rgb(resolve(theme, n));
  const base = hex("--h-content-background");
  const primary = hex("--h-primary-color");
  // sidebar.css: color-mix(in srgb, primary 28%, transparent) over the content background.
  const tint = base.map((b, i) => primary[i] * 0.28 + b * 0.72);

  it("keeps the tint visible against the plain row background", () => {
    expect(contrast(tint, base)).toBeGreaterThan(1.15);
  });

  it.each([
    ["name, line 2 and idle state", "--h-text-color"],
    ["working state", statusToken("working", dk)],
    ["blocked state", statusToken("blocked", dk)],
    ["done state", statusToken("done", dk)],
  ])("%s reaches AA on the tint", (_label, token) => {
    expect(contrast(hex(token), tint)).toBeGreaterThanOrEqual(4.5);
  });
});

// The ringing card of the cards layout (space-card.css `.card.attention`, #1085): a bar of the blocked
// color at the left edge (a graphic, 3:1) on the card background, which also carries the text.
describe.each([
  ["light", false],
  ["dark", true],
])("ringing space card, %s theme", (_name, dk) => {
  const theme = dk ? new Map([...light, ...dark]) : light;
  const hex = (n: string) => rgb(resolve(theme, n));
  const base = hex("--h-content-background");
  const cssBlock = dk ? css.slice(css.indexOf(":root.app-dark")) : css;
  const status = (name: string) => {
    const re = new RegExp(`--status-${name}:\\s*var\\((--[\\w-]+)\\)`);
    const m = cssBlock.match(re) ?? css.match(re);
    if (!m) throw new Error(`--status-${name} missing`);
    return m[1];
  };
  const blocked = rgb(resolve(theme, status("blocked-bar")));
  const text = hex("--h-text-color");
  // The card background of space-card.css: 5% of the text color over the content background.
  const card = base.map((b, i) => text[i] * 0.05 + b * 0.95);

  it("shows the bar against the card", () => {
    expect(contrast(blocked, card)).toBeGreaterThanOrEqual(3);
  });

  it.each([
    ["name and idle state", "--h-text-color", 4.5],
    ["muted lines", "--h-text-muted-color", 4.5],
    ["blocked state and silent mark", statusToken("blocked", dk), 4.5],
    ["working state", statusToken("working", dk), 4.5],
    ["done state", statusToken("done", dk), 4.5],
  ])("%s stays legible on the card", (_label, token, min) => {
    expect(contrast(hex(token), card)).toBeGreaterThanOrEqual(min);
  });
});

// The origin stripe and symbol of the space cards (#1074): graphics, so 3:1 against the card.
describe.each([
  ["light", false],
  ["dark", true],
])("space card origin colors, %s theme", (_name, dk) => {
  const theme = dk ? new Map([...light, ...dark]) : light;
  const base = rgb(resolve(theme, "--h-content-background"));
  const cssBlock = dk
    ? css.slice(css.indexOf(":root.app-dark"))
    : css.slice(0, css.indexOf(":root.app-dark"));

  it.each([0, 1, 2, 3, 4, 5])("origin color %i reaches 3:1 on the card", (n) => {
    const m = cssBlock.match(new RegExp(`--origin-${n}:\\s*var\\((--[\\w-]+)\\)`));
    if (!m) throw new Error(`--origin-${n} missing`);
    expect(contrast(rgb(resolve(theme, m[1])), base)).toBeGreaterThanOrEqual(3);
  });
});
