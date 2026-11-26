import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * Component styles (#1365, `docs/agents/frontend.md`): font sizes are steps of the type scale, no
 * rule styles a native control by its element, and colors come from the tokens. A component style is
 * a `.css` file under `src/app`, the `styles` of a component and a CodeMirror theme in TypeScript.
 * `src/styles.css` defines the tokens and the shared control rules, so only its sizes are checked.
 */
function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return files(path);
    return /\.(css|ts)$/.test(name) && !/\.(spec|mock)\.ts$/.test(name) ? [path] : [];
  });
}

const withoutComments = (css: string): string => css.replace(/\/\*.*?\*\//gs, "");

/** The stylesheets: `.css` files, and the `styles` templates of components. */
const sheets = files("src/app").flatMap((file) => {
  const text = readFileSync(file, "utf8");
  if (file.endsWith(".css")) return [[file, withoutComments(text)] as const];
  return [...text.matchAll(/\bstyles: `([^`]*)`/g)].map(
    (m) => [file, withoutComments(m[1])] as const,
  );
});

/** CodeMirror themes: style objects with camel-case properties. */
const themes = files("src/app")
  .filter((file) => file.endsWith(".ts"))
  .map((file) => [file, readFileSync(file, "utf8")] as const)
  .filter(([, text]) => /EditorView\.(baseT|t)heme|ThemeRules/.test(text));

/** The subject of each selector: its last compound, without what is inside `()`. */
function subjects(css: string): string[] {
  return [...css.matchAll(/([^{};]+)\{/g)]
    .map((m) => m[1].trim())
    .filter((selector) => !selector.startsWith("@"))
    .flatMap((selector) => {
      let flat = selector;
      while (/\([^()]*\)/.test(flat)) flat = flat.replace(/\([^()]*\)/g, "");
      return flat.split(",").map(
        (part) =>
          part
            .trim()
            .split(/[\s>+~]+/)
            .pop() ?? "",
      );
    });
}

const sizes = (text: string): string[] =>
  [...text.matchAll(/(?<![-\w])(?:font-size|fontSize)\s*:\s*"?([^;"}\n,]+)/g)].map((m) =>
    m[1].trim(),
  );

describe("component styles", () => {
  it("find the stylesheets", () => {
    expect(sheets.length).toBeGreaterThan(40);
    expect(themes.length).toBeGreaterThan(0);
  });

  it("take every font size from the type scale", () => {
    const shared = ["src/styles.css", withoutComments(readFileSync("src/styles.css", "utf8"))];
    const bad = [...sheets, ...themes, shared as [string, string]].flatMap(([file, text]) =>
      sizes(text)
        .filter((size) => size !== "inherit" && !size.includes("var(--h-font-size-"))
        .map((size) => `${file}: ${size}`),
    );
    expect(bad).toEqual([]);
  });

  it("set no size through the font shorthand", () => {
    const bad = sheets.flatMap(([file, text]) =>
      [...text.matchAll(/(?<![-\w])font\s*:\s*([^;}]+)/g)]
        .filter((m) => m[1].trim() !== "inherit")
        .map((m) => `${file}: font: ${m[1].trim()}`),
    );
    expect(bad).toEqual([]);
  });

  it("have no rule for a button, input, select or textarea", () => {
    const bad = sheets.flatMap(([file, css]) =>
      subjects(css)
        .filter((subject) => /^(button|input|select|textarea)(?![\w-])/.test(subject))
        .map((subject) => `${file}: ${subject}`),
    );
    expect(bad).toEqual([]);
  });

  it("have no hex, rgb() or hsl() color", () => {
    const bad = [...sheets, ...themes].flatMap(([file, text]) => {
      // In TypeScript only the quoted values count: `#123` in a comment is an issue number.
      const values = file.endsWith(".ts")
        ? [...text.matchAll(/"([^"\n]*)"/g)].map((m) => m[1]).join("\n")
        : text;
      return [
        ...values.matchAll(/#(?:[0-9a-f]{8}|[0-9a-f]{6}|[0-9a-f]{3,4})\b|\b(?:rgb|hsl)a?\(/gi),
      ]
        .map((m) => `${file}: ${m[0]}`)
        .filter(Boolean);
    });
    expect(bad).toEqual([]);
  });
});
