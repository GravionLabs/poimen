import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

/** Classes the detail templates use that `github-badges.css` defines. */
const SHARED = ["icon-btn", "badge"];

function imports(css: string): string[] {
  return [...css.matchAll(/@import\s+"([^"]+)"/g)].map((m) => m[1]);
}

describe("detail panes share the badge and icon button styles", () => {
  const panes = ["issue-detail/issue-detail", "pull-detail/pull-detail"];

  for (const pane of panes) {
    it(`${pane} imports github-badges.css and uses its classes`, () => {
      const css = readFileSync(`src/app/github/${pane}.css`, "utf8");
      const html = readFileSync(`src/app/github/${pane}.html`, "utf8");
      expect(imports(css)).toContain("../github-badges.css");
      for (const name of SHARED) expect(html).toContain(name);
    });
  }

  it("the shared sheet defines the classes", () => {
    const css = readFileSync("src/app/github/github-badges.css", "utf8");
    for (const name of SHARED) expect(css).toContain(`.${name}`);
  });
});
