import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

describe("silent status dot (#1149)", () => {
  it("the tab dot gets the silent class only for a pane with agent_silent_since", () => {
    const html = readFileSync("src/app/app.html", "utf8");
    expect(html).toContain('[class.silent]="p?.agent_silent_since !== undefined"');
  });

  it("the silent dot is hollow and static, from the working token", () => {
    const css = readFileSync("src/styles.css", "utf8");
    const rule = /\.status-working\.silent \{([^}]*)\}/.exec(css)?.[1] ?? "";
    expect(rule).toContain("background: transparent");
    expect(rule).toContain("var(--status-working)");
    expect(rule).toContain("animation: none");
  });

  it("the tab no longer draws a text mark", () => {
    const html = readFileSync("src/app/ui/helix-candidates/tab-bar/tab-bar.html", "utf8");
    const css = readFileSync("src/app/ui/helix-candidates/tab-bar/tab-bar.css", "utf8");
    expect(html).not.toContain("tab.mark");
    expect(css).not.toMatch(/^\.(mark|attention-mark)\b/m);
  });
});
