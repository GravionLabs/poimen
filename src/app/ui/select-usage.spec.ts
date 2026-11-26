import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * Selects are `hx-select` in an `hx-field`, `size="small"` and with an accessible name (#1361,
 * `docs/agents/frontend.md`), and no component styles a `select`. A native `<select>` needs an entry
 * in `NATIVE` with the reason.
 */
const NATIVE: Record<string, { count: number; reason: string }> = {
  "src/app/bottom-panel/bottom-panel.html": {
    count: 1,
    reason: "the run picker groups the runs by subject (optgroup); hx-select has no option groups",
  },
};

function sources(dir: string, pattern: RegExp): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sources(path, pattern);
    return pattern.test(name) && !name.endsWith(".spec.ts") ? [path] : [];
  });
}

const templates = sources("src/app", /\.(html|ts)$/).map(
  (file) => [file, readFileSync(file, "utf8")] as const,
);

describe("selects", () => {
  it("are hx-select, or listed with a reason", () => {
    const native = templates
      .map(([file, text]) => [file, (text.match(/<select[\s>]/g) ?? []).length] as const)
      .filter(([, n]) => n > 0);
    expect(Object.fromEntries(native)).toEqual(
      Object.fromEntries(Object.entries(NATIVE).map(([file, { count }]) => [file, count])),
    );
  });

  it("sit in an hx-field, are small and have an accessible name", () => {
    const bad = templates.flatMap(([file, text]) => {
      // The `<hx-select>` elements outside every `<hx-field>`.
      const outside = text.replace(/<hx-field[\s>].*?<\/hx-field>/gs, "");
      const loose = (outside.match(/<hx-select[\s>]/g) ?? []).map(() => `${file}: no hx-field`);
      const tags = text.match(/<hx-select\s(?:[^>"']|"[^"]*"|'[^']*')*>/g) ?? [];
      return [
        ...loose,
        ...tags.flatMap((tag) => [
          ...(tag.includes('size="small"') ? [] : [`${file}: not size="small"`]),
          ...(/\[?ariaLabel(ledby)?\]?=/.test(tag) ? [] : [`${file}: no accessible name`]),
        ]),
      ];
    });
    expect(bad).toEqual([]);
  });

  it("are not styled by a component", () => {
    // A type selector `select` in a stylesheet or in inline `styles`.
    const rule = /(^|[\s,>+~}(])select(?=[\s]*[,{]|:[a-z-]+[^;]*\{)/m;
    const styled = [
      ...sources("src/app", /\.css$/).map((f) => [f, readFileSync(f, "utf8")] as const),
      ["src/styles.css", readFileSync("src/styles.css", "utf8")] as const,
      ...templates.flatMap(([file, text]) => {
        const styles = /styles: `([^`]*)`/.exec(text);
        return styles ? [[file, styles[1]] as const] : [];
      }),
    ]
      .filter(([, css]) => rule.test(css.replace(/\/\*.*?\*\//gs, "")))
      .map(([file]) => file);
    expect(styled).toEqual([]);
  });
});
