import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { splitDetails } from "./space-card";
import type { Segment } from "./spaces";

const seg = (token: string, text: string, tip?: string): Segment => ({
  kind: "text",
  text,
  status: "unknown",
  bold: false,
  dim: false,
  sep: false,
  token,
  ...(tip ? { tip } : {}),
});

// #1085: the details block of a card.
describe("splitDetails", () => {
  const branch = seg("branch", "main");
  const dirty = seg("dirty", "●2");
  const sync = seg("sync", "↑1 ↓281");

  it("keeps a directory that repeats the name out of the chips", () => {
    const dir = seg("dir", ".../gravionlabs/gantry", "/home/u/Source/gravionlabs/gantry");
    const details = splitDetails([branch, dir, dirty], "gantry");
    expect(details?.dir).toBeUndefined();
    expect(details?.chips).toEqual([dirty]);
  });

  it("shows the directory when its last segment differs from the name", () => {
    const dir = seg("dir", ".../poimen-wt-987", "/home/u/Source/gravionlabs/poimen-wt-987");
    const details = splitDetails([branch, dir, sync], "#987 keep the machine awake");
    expect(details?.dir).toBe(dir);
    expect(details?.chips).toEqual([sync]);
  });

  it("has nothing to show for a line with only a directory that repeats the name", () => {
    const dir = seg("dir", ".../Source/gravionlabs", "/home/u/Source/gravionlabs");
    expect(splitDetails([dir], "gravionlabs")).toBeNull();
    expect(splitDetails([], "x")).toBeNull();
  });
});

describe("space card layout styles (#1085)", () => {
  const css = readFileSync("src/app/sidebar/space-card.css", "utf8");
  const sidebar = readFileSync("src/app/sidebar/sidebar.css", "utf8");
  const html = readFileSync("src/app/sidebar/space-card.html", "utf8");
  const block = (selector: string) =>
    new RegExp(`^${selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")} \\{([^}]*)\\}`, "m").exec(
      css,
    )?.[1] ?? "";

  it("separates the cards: a raised background, a visible border and a small gap", () => {
    const card = block(".card");
    expect(card).toMatch(/background:\s*color-mix/);
    expect(card).toMatch(/border:\s*1px solid color-mix/);
    expect(card).toMatch(/box-shadow:/);
    expect(/\.cards \{[^}]*gap:\s*(\d+)px/.exec(sidebar)?.[1]).toBe("8");
  });

  it("ringing is a bar at the left edge, not a frame around the card", () => {
    expect(block(".card.attention")).not.toMatch(/border|box-shadow|background/);
    const bar = block(".card.attention::after");
    expect(bar).toMatch(/inset:\s*0 auto 0 0/);
    expect(bar).toMatch(/width:\s*4px/);
    expect(bar).toContain("var(--status-blocked-bar)");
  });

  it("a ringing row has a bar at its left edge too, no frame and no glow", () => {
    const row = block(".row.attention");
    expect(row).toMatch(/box-shadow:\s*inset 3px 0 0 var\(--status-blocked-bar\)/);
    expect(row).not.toMatch(/0 0 6px/);
  });

  it("tints the details block; its toggle is an info icon in the header, before the actions", () => {
    expect(block(".details")).toMatch(/background:\s*color-mix\([^)]*--h-primary-color/);
    const head = html.slice(html.indexOf("<header"), html.indexOf("</header>"));
    expect(head).toContain('class="details-toggle"');
    expect(head.indexOf('class="details-toggle"')).toBeLessThan(head.indexOf('class="actions"'));
    // The block holds the branch with the chips; the directory is a line of its own below.
    expect(html.indexOf('class="branch"')).toBeLessThan(html.indexOf('class="dir"'));
    // Hidden, the block is not rendered at all.
    expect(html).toMatch(/@if \(!detailsHidden\(\)\) \{\s*<div class="details"/);
  });

  it("lets the name shrink before the header wraps and drops the fold chevron", () => {
    const title = block(".title");
    expect(title).toMatch(/flex:\s*1 1 0;/);
    expect(title).toMatch(/min-width:\s*3rem/);
  });

  it("keeps room between the origin icon and the status dot", () => {
    expect(block(".origin-mark")).toMatch(/margin:\s*0 6px 0 4px/);
  });

  it("has the origin icon first and the fold chevron last in the header", () => {
    const head = html.slice(html.indexOf("<header"), html.indexOf("</header>"));
    expect(head.indexOf('class="origin-mark"')).toBeGreaterThan(-1);
    expect(head.indexOf('class="origin-mark"')).toBeLessThan(head.indexOf('class="title"'));
    expect(head.indexOf('class="actions"')).toBeLessThan(head.indexOf('class="fold"'));
  });

  it("marks the details toggle with an info icon, not a chevron", () => {
    const toggle = html.slice(
      html.indexOf('class="details-toggle"'),
      html.indexOf("</button>", html.indexOf('class="details-toggle"')),
    );
    expect(toggle).toContain("details-icon");
    expect(toggle).not.toContain("chevron");
  });

  it("makes every chip small, a pull request chip too, without a leading dot", () => {
    const chips = /\.chips \.text,\s*\.chips \.pr-link \{([^}]*)\}/.exec(css)?.[1] ?? "";
    expect(chips).toMatch(/min-height:\s*0/);
    expect(chips).toMatch(/height:\s*16px/);
    expect(html).toContain('seg.sep && !chip ? "· " : ""');
  });
});
