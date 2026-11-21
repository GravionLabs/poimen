import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const css = readFileSync("src/styles.css", "utf8");

describe("reduced motion (#1041)", () => {
  const block = css.slice(css.indexOf("@media (prefers-reduced-motion: reduce)"));

  it("stops the working pulse and shows a static ring", () => {
    expect(block).toMatch(/\.status-working\s*{[^}]*animation:\s*none[^}]*box-shadow/s);
  });

  it("does not animate transforms", () => {
    const rule = block.match(/transition-property:([^;]*)!important/)?.[1] ?? "";
    expect(rule).not.toContain("transform");
    expect(rule).toContain("color");
  });
});
