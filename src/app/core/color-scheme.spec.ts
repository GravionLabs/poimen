import { readFileSync } from "node:fs";
import { describe, expect, it, vi } from "vitest";
import {
  activeRowTint,
  appearance,
  bundled,
  contrast,
  luminance,
  deriveTokens,
  mix,
  resolveScheme,
  type ColorScheme,
} from "./color-scheme";

const HERDR = [
  "catppuccin",
  "tokyo-night",
  "tokyo-night-day",
  "dracula",
  "gruvbox",
  "gruvbox-light",
  "one-dark",
  "one-light",
  "solarized",
  "solarized-light",
  "kanagawa",
  "kanagawa-lotus",
  "rose-pine",
  "rose-pine-dawn",
  "vesper",
];
const LIGHT = [
  "poimen-light",
  "tokyo-night-day",
  "gruvbox-light",
  "one-light",
  "solarized-light",
  "kanagawa-lotus",
  "rose-pine-dawn",
];

const dracula = bundled.get("dracula")!;

describe("bundled schemes", () => {
  it("has poimen and the 15 herdr schemes", () => {
    expect([...bundled.keys()].sort()).toEqual(["poimen-dark", "poimen-light", ...HERDR].sort());
  });

  it.each([...bundled.values()])("$name is well formed", (s) => {
    expect(s.ansi).toHaveLength(16);
    for (const c of [s.background, s.foreground, s.cursor, s.selectionBackground, ...s.ansi])
      expect(c).toMatch(/^#[0-9a-f]{6}$/);
  });

  it("keeps the canonical upstream values", () => {
    expect(dracula).toMatchObject({ background: "#282a36", foreground: "#f8f8f2" });
    expect(dracula.ansi[5]).toBe("#ff79c6");
    expect(bundled.get("tokyo-night")?.ansi[4]).toBe("#7aa2f7");
    expect(bundled.get("gruvbox-light")?.background).toBe("#fbf1c7");
  });

  it("matches the --term-* tokens of styles.css for poimen", () => {
    const css = readFileSync("src/styles.css", "utf8");
    const split = css.indexOf(":root.app-dark");
    const read = (block: string) => {
      const out = new Map<string, string>();
      for (const d of block.matchAll(/(--term-[\w-]+):\s*(#[0-9a-f]{6});/gi)) out.set(d[1], d[2]);
      return out;
    };
    for (const [name, block] of [
      ["poimen-light", css.slice(0, split)],
      ["poimen-dark", css.slice(split, css.indexOf("* {"))],
    ] as const) {
      const tokens = deriveTokens(bundled.get(name)!);
      for (const [token, value] of read(block))
        expect(tokens[token], `${name} ${token}`).toBe(value);
    }
  });
});

describe("appearance", () => {
  it.each([...bundled.values()])("detects $name from its background", (s) => {
    expect(appearance(s)).toBe(LIGHT.includes(s.name) ? "light" : "dark");
  });

  it("treats the boundary colors as dark below and light above", () => {
    const s = (background: string): ColorScheme => ({ ...dracula, background });
    expect(appearance(s("#000000"))).toBe("dark");
    expect(appearance(s("#ffffff"))).toBe("light");
    expect(appearance(s("#707070"))).toBe("dark");
    expect(appearance(s("#808080"))).toBe("light");
  });
});

describe("deriveTokens", () => {
  it("is deterministic", () => {
    expect(deriveTokens(dracula)).toEqual(deriveTokens({ ...dracula }));
  });

  it("copies the terminal colors and the ANSI palette", () => {
    const t = deriveTokens(dracula);
    expect(t["--term-bg"]).toBe("#282a36");
    expect(t["--term-fg"]).toBe("#f8f8f2");
    expect(t["--term-cursor"]).toBe("#f8f8f2");
    expect(t["--term-selection-bg"]).toBe("#44475a");
    expect(t["--term-ansi-0"]).toBe("#21222c");
    expect(t["--term-ansi-15"]).toBe("#ffffff");
  });

  it("runs the surface ramp from the lightest to the darkest step", () => {
    const dark = deriveTokens(dracula);
    expect(dark["--h-surface-950"]).toBe("#282a36");
    expect(dark["--h-surface-0"]).toBe("#f8f8f2");
    expect(dark["--app-ground"]).toBe(dark["--h-surface-950"]);
    const light = deriveTokens(bundled.get("one-light")!);
    expect(light["--h-surface-0"]).toBe("#fafafa");
    expect(light["--h-surface-950"]).toBe("#383a42");
    expect(light["--app-ground"]).toBe(light["--h-surface-50"]);
  });

  it("derives the accent from ANSI magenta", () => {
    const t = deriveTokens(dracula);
    expect(t["--h-primary-500"]).toBe("#ff79c6");
    // Steps run from light to dark in every scheme.
    expect(luminance(t["--h-primary-50"])).toBeGreaterThan(luminance(t["--h-primary-500"]));
    expect(luminance(t["--h-primary-500"])).toBeGreaterThan(luminance(t["--h-primary-950"]));
  });

  it("takes the status colors from ANSI blue, yellow and green", () => {
    const t = deriveTokens(dracula);
    expect(t["--status-working"]).toBe("#bd93f9");
    expect(t["--status-blocked"]).toBe("#f1fa8c");
    expect(t["--status-done"]).toBe("#50fa7b");
  });

  it("lets chrome overrides replace derived tokens", () => {
    const t = deriveTokens({
      ...dracula,
      chrome: { "--app-ground": "#010203", "--status-done-on-active": "#ffffff" },
    });
    expect(t["--app-ground"]).toBe("#010203");
    expect(t["--status-done-on-active"]).toBe("#ffffff");
    expect(t["--h-surface-950"]).toBe("#282a36");
  });

  it("mixes in OKLab between the end points", () => {
    expect(mix("#000000", "#ffffff", 0)).toBe("#000000");
    expect(mix("#000000", "#ffffff", 1)).toBe("#ffffff");
    expect(mix("#000000", "#ffffff", 0.5)).toBe("#636363");
  });
});

describe.each([...bundled.values()])("$name contrast", (scheme) => {
  const t = deriveTokens(scheme);
  const tint = activeRowTint(t);
  const grounds = [
    ["content", t["--h-content-background"]],
    ["ground", t["--app-ground"]],
    ["active row", tint],
  ];

  it.each(grounds)("text reaches AA on the %s background", (_n, bg) => {
    expect(contrast(t["--h-text-color"], bg)).toBeGreaterThanOrEqual(4.5);
  });

  it.each(grounds)("muted text reaches AA on the %s background", (_n, bg) => {
    expect(contrast(t["--h-text-muted-color"], bg)).toBeGreaterThanOrEqual(4.5);
  });

  it.each(["working", "blocked", "done"])("%s state text reaches AA on the active row", (n) => {
    expect(contrast(t[`--status-${n}-on-active`], tint)).toBeGreaterThanOrEqual(4.5);
  });

  it("keeps the active-row tint visible", () => {
    expect(contrast(tint, t["--h-content-background"])).toBeGreaterThan(1.05);
  });
});

describe("resolveScheme", () => {
  it("returns the bundled scheme of a known name", () => {
    expect(resolveScheme("dracula")?.name).toBe("dracula");
  });

  it("returns null for the default without a warning", () => {
    const warn = vi.fn();
    expect(resolveScheme(null, bundled, warn)).toBeNull();
    expect(resolveScheme("poimen", bundled, warn)).toBeNull();
    expect(warn).not.toHaveBeenCalled();
  });

  it("warns and falls back to poimen for an unknown name", () => {
    const warn = vi.fn();
    expect(resolveScheme("nope", bundled, warn)).toBeNull();
    expect(warn).toHaveBeenCalledOnce();
    expect(warn.mock.calls[0][0]).toContain('"nope"');
  });

  it("lets a user scheme of the same name win", () => {
    const mine = { ...dracula, label: "Mine" };
    expect(resolveScheme("dracula", new Map([["dracula", mine]]))?.label).toBe("Mine");
  });
});
