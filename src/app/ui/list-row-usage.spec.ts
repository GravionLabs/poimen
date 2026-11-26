import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * Rows are `hx-list-row` in an `hx-list` (#1363, `docs/agents/frontend.md`): the palettes and the
 * pickers build no row styles of their own. A sidebar row that Helix cannot express needs an entry in
 * `HAND_BUILT` with the reason.
 */
const LISTS = [
  "src/app/command-palette/command-palette",
  "src/app/jump-palette/jump-palette",
  "src/app/agent-picker/agent-picker",
  "src/app/github/chip-picker/chip-picker",
];

const HAND_BUILT: Record<string, { count: number; reason: string }> = {
  "src/app/sidebar/space-card.html": {
    count: 2,
    reason:
      "the card header and the pane row are treeitems of one tree: they hold buttons (pull request, " +
      "records, rename field), which a button row cannot nest, they are multi-line and draggable, and " +
      "the tree owns ArrowLeft/ArrowRight and the roving tab stop, which hx-list does not",
  },
};

function templates(dir: string): string[] {
  return readdirSync(dir, { recursive: true, encoding: "utf8" })
    .filter((name) => name.endsWith(".html"))
    .map((name) => join(dir, name));
}

describe("list rows", () => {
  it.each(LISTS)("%s renders its rows with hx-list and hx-list-row", (path) => {
    const html = readFileSync(`${path}.html`, "utf8");
    expect(html).toMatch(/<hx-list\b/);
    expect(html).toMatch(/<button\s+hx-list-row\b/);
    expect(html).not.toMatch(/<li\b(?![^>]*class="(chip|none)")/);
    const ts = readFileSync(`${path}.ts`, "utf8");
    expect(ts).toMatch(/imports: \[[^\]]*HxList,\s*HxListRow\b/);
  });

  it("the chip picker has no row styles", () => {
    const css = readFileSync("src/app/github/chip-picker/chip-picker.css", "utf8");
    expect(css).not.toMatch(/hx-list-row|li button|\.options button/);
  });

  it("lists every hand-built sidebar row with a reason", () => {
    const found = templates("src/app/sidebar")
      .map((file) => {
        const text = readFileSync(file, "utf8");
        return [file, (text.match(/role="(treeitem|option)"/g) ?? []).length] as const;
      })
      .filter(([, n]) => n > 0);
    expect(Object.fromEntries(found)).toEqual(
      Object.fromEntries(Object.entries(HAND_BUILT).map(([file, { count }]) => [file, count])),
    );
  });
});
