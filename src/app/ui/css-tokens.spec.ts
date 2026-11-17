import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * Every `var(--name)` of the stylesheets and component styles names a custom property that exists
 * (#856): an undefined one makes the declaration invalid (no border, normal text color), and a hex
 * fallback beside it ignores the color scheme. Defined = declared or set somewhere in `src/` or by
 * helix-ui (`tokens.css`, `styles.css`, `core.css`).
 */
const HELIX = "node_modules/@gravionlabs/helix-ui";
const SOURCE = /\.(css|ts|html)$/;

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? files(path) : SOURCE.test(name) ? [path] : [];
  });
}

const sources = files("src").filter((f) => !f.endsWith(".spec.ts"));
const text = (f: string): string => readFileSync(f, "utf8");

function used(): Map<string, string[]> {
  const found = new Map<string, string[]>();
  for (const file of sources) {
    for (const m of text(file).matchAll(/var\(\s*(--[A-Za-z0-9_-]+)(?![A-Za-z0-9_-]|\$\{)/g)) {
      found.set(m[1], [...(found.get(m[1]) ?? []), file]);
    }
  }
  return found;
}

function defined(): Set<string> {
  const names = new Set<string>();
  const all = [
    ...sources.map(text),
    ...["tokens.css", "styles.css", "core.css"].map((f) => text(join(HELIX, f))),
  ];
  for (const content of all) {
    for (const m of content.matchAll(/(--[A-Za-z0-9_-]+)\s*:/g)) names.add(m[1]);
    for (const m of content.matchAll(/["'`](--[A-Za-z0-9_-]+)["'`]/g)) names.add(m[1]);
  }
  return names;
}

describe("css custom properties", () => {
  it("every var(--name) is defined", () => {
    const known = defined();
    const missing = [...used()]
      .filter(([name]) => !known.has(name))
      .map(([name, where]) => `${name}: ${[...new Set(where)].join(", ")}`);
    expect(missing).toEqual([]);
  });

  it("scheme tones have no hex fallback", () => {
    const tone = /var\(--(status-[a-z-]+|term-ansi-\d+)\s*,\s*#/;
    const offenders = sources.filter((f) => tone.test(text(f)));
    expect(offenders).toEqual([]);
  });
});
