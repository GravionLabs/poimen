import { describe, expect, it } from "vitest";
import { cssAnsi, diffHighlight, highlightRules, languageFor, loadLanguage } from "./diff-language";

describe("languageFor", () => {
  it.each([
    ["src/app/a.ts", "TypeScript"],
    ["src-tauri/src/lib.rs", "Rust"],
    ["package.json", "JSON"],
    ["docs/README.md", "Markdown"],
    ["scripts/Format-Changed.ps1", "PowerShell"],
  ])("finds the language of %s", (path, name) => {
    expect(languageFor(path)?.name).toBe(name);
  });

  it("is null for an unknown extension or no name", () => {
    expect(languageFor("data.unknownext")).toBeNull();
    expect(languageFor("dir/")).toBeNull();
  });
});

describe("loadLanguage", () => {
  it("loads the support of a known file lazily", async () => {
    expect(await loadLanguage("a.json")).not.toBeNull();
  });

  it("falls back to plain text for an unknown file", async () => {
    expect(await loadLanguage("a.unknownext")).toBeNull();
  });
});

describe("highlight style", () => {
  const scheme = (index: number) => `#00${index.toString(16).padStart(2, "0")}00`;

  it("takes its colors from the scheme palette", () => {
    const rules = highlightRules(scheme);
    const colors = new Set(rules.map((rule) => rule.color).filter(Boolean));
    expect(colors).toContain("#000200"); // ANSI green for strings
    expect(colors).toContain("#000500"); // ANSI magenta for keywords
    expect(colors).toContain("#000800"); // bright black for comments
    expect([...colors].every((c) => /^#00[0-9a-f]{2}00$/.test(c as string))).toBe(true);
  });

  it("uses the CSS color tokens by default, so light and dark follow the scheme", () => {
    expect(cssAnsi(2)).toBe("var(--term-ansi-2)");
    expect(
      highlightRules().every((rule) => !rule.color || rule.color.startsWith("var(--term-ansi-")),
    ).toBe(true);
    expect(diffHighlight()).toBeDefined();
  });
});
