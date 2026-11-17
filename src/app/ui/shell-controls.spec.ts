import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

/**
 * #849: in the shell, actions are `hx-button`s and information is not. The files are read as text so
 * the rules hold without a browser (hover is a CSS state and never carries the information).
 */
const read = (path: string): string => readFileSync(path, "utf8");
const tags = (html: string, re: RegExp): string[] => html.match(re) ?? [];

describe("status bar", () => {
  const html = read("src/app/status-bar/status-bar.html");
  const css = read("src/app/status-bar/status-bar.css");

  it("every clickable item is an hx-button, every information item a span", () => {
    const buttons = tags(html, /<button\b[^>]*>/g);
    expect(buttons.length).toBeGreaterThan(5);
    for (const tag of buttons) expect(tag).toContain("hx-button");
    for (const tag of tags(html, /<span\b[^>]*class="item[^>]*>/g)) {
      expect(tag).not.toContain("(click)");
    }
  });

  it("clickable items have hover, focus and pointer; information items none; disabled looks disabled", () => {
    expect(css).toMatch(/button\.item:hover:not\(:disabled\)/);
    expect(css).toMatch(/button\.item:focus-visible/);
    expect(css).toMatch(/button\.item \{[^}]*cursor: pointer/);
    expect(css).toMatch(/button\.item:disabled \{[^}]*opacity: 0\.5/);
    expect(css).not.toMatch(/(^|\n)\.item:hover/);
  });

  it("the waiting item is disabled at zero", () => {
    expect(html).toMatch(/class="item waiting"[\s\S]*?\[disabled\]="waiting\(\) === 0"/);
  });
});

describe("sidebar", () => {
  const html = read("src/app/sidebar/sidebar.html");
  const css = read("src/app/sidebar/sidebar.css");

  it("the toggles, +, Retry and Rebuild are hx-buttons; the toggles show their pressed state", () => {
    for (const name of ["layout-toggle", "add", "retry", "rebuild"]) {
      const tag = tags(html, /<button\b[^>]*>/g).find((t) => t.includes(`class="${name}`));
      expect(tag, name).toBeDefined();
    }
    const toggles = html.match(/<button[^>]*class="layout-toggle[\s\S]*?>/g) ?? [];
    expect(toggles).toHaveLength(3);
    for (const t of toggles) {
      expect(t).toContain("hx-button");
      expect(t).toContain("aria-pressed");
    }
    expect(css).toMatch(/\.layout-toggle\[aria-pressed="true"\]/);
  });

  it("the pane-kind mark has no border and the pin is an icon, not an emoji", () => {
    expect(css).toMatch(/\.icon \{[^}]*border: 0/);
    expect(html).not.toContain("📌");
    expect(html).toMatch(/<svg\s+class="pin-mark"/);
  });
});

describe("notifications and space overview", () => {
  it("Mark all read is a text hx-button", () => {
    expect(read("src/app/notifications/notifications.html")).toMatch(
      /hx-button\s+variant="text"[^>]*class="mark"/,
    );
  });

  it("the overview has helix controls, a primary Save, a danger Delete and reasons on disabled actions", () => {
    const html = read("src/app/space-overview/space-overview.html");
    for (const tag of tags(html, /<button\b[^>]*>/g)) expect(tag).toContain("hx-button");
    for (const tag of tags(html, /<(input|textarea)\b[^>]*>/g)) expect(tag).toContain("hx-input");
    expect(html).toMatch(/severity="danger"[^>]*class="act-delete"/);
    expect(html).toMatch(/size="small"\s+class="cfg-save"/);
    expect(html).toMatch(/size="small"\s+class="cfg-trust"/);
    expect(html).toMatch(/act-explorer[\s\S]*?\[attr\.title\]/);
    expect(html).toMatch(/act-pr[\s\S]*?\[attr\.title\]/);
  });

  it("the status dots keep the global colors", () => {
    expect(read("src/app/space-overview/space-overview.css")).not.toMatch(
      /\.dot \{[^}]*background/,
    );
  });
});
