import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH } from "./core/ui-state";

const html = readFileSync("src/app/app.html", "utf8");
const ts = readFileSync("src/app/app.ts", "utf8");

describe("app shell (#930)", () => {
  it("lays the frame out with one hx-app-shell and its slots", () => {
    expect(html.match(/<hx-app-shell\b/g)).toHaveLength(1);
    for (const slot of ["toolbar", "activity", "sidebar", "status"]) {
      expect(html).toContain(`slot="${slot}"`);
    }
  });

  it("keeps the limits and steps of the sidebar and persists through UiState", () => {
    expect(SIDEBAR_MIN_WIDTH).toBe(180);
    expect(SIDEBAR_MAX_WIDTH).toBe(480);
    expect(html).toContain('[minWidth]="minWidth"');
    expect(html).toContain('[maxWidth]="maxWidth"');
    expect(html).toContain('[step]="10"');
    expect(html).toContain('(sidebarChange)="onSidebarChange($event)"');
  });

  it("creates the sidebar view only while the sidebar is expanded", () => {
    expect(html).toMatch(
      /@if \(!ui\.sidebarCollapsed\(\)\) \{\s*@if \(ui\.activeView\(\) === "explorer"\)/,
    );
  });

  it("has no own resizer", () => {
    expect(html).not.toContain("resizer");
    expect(ts).not.toMatch(/dragStart|dragMove|dragEnd|resizeKey/);
  });
});
