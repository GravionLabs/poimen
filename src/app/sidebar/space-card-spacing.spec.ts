import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// #1079: the first row must not touch the separator line under the card header.
describe("space card body spacing", () => {
  const css = readFileSync("src/app/sidebar/space-card.css", "utf8");
  const body = /^\.body \{([^}]*)\}/m.exec(css)?.[1] ?? "";

  it("pads the body at the top and the bottom by the same amount", () => {
    expect(body).toContain("border-top");
    expect(body).toMatch(/padding-block:\s*calc\(var\(--h-space-1\) \* 1\.5\)/);
    expect(body).not.toMatch(/padding-bottom/);
  });
});
