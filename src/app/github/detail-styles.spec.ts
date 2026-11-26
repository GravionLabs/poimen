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

describe("icon buttons and badge buttons have states", () => {
  const css = readFileSync("src/app/github/github-badges.css", "utf8");

  it("icon buttons are visible by default and hidden only inside list rows", () => {
    expect(css).toMatch(/\.icon-btn \{\s*opacity: 0\.7;/);
    expect(css).toMatch(/:is\(\.row, \.item, tbody tr\) \.icon-btn \{\s*opacity: 0;/);
    expect(css).toMatch(/:focus-within, \.cursor\) \.icon-btn \{\s*opacity: 1;/);
  });

  it("icon buttons and links have a disabled style", () => {
    expect(css).toMatch(/\.icon-btn:disabled,\s*\.link:disabled \{/);
    const pane = readFileSync("src/app/github/detail-pane.css", "utf8");
    expect(pane).toContain(".link:disabled");
  });

  it("the run badge has pointer, hover and focus styles and inherits the font", () => {
    expect(css).toMatch(/\.badge\.hx-button \{[^}]*font: inherit;[^}]*cursor: pointer;/);
    expect(css).toContain(".badge.hx-button:hover");
    expect(css).toContain(".badge.hx-button:focus-visible");
  });
});

describe("milestone Close button", () => {
  it("is rendered once, whether or not the milestone is ready to close", () => {
    const html = readFileSync("src/app/github/milestones-section.html", "utf8");
    expect(html.match(/data-testid="milestone-close"/g)).toHaveLength(1);
  });
});
