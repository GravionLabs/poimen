// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { ChildRecord, SpaceNode } from "../core/snapshot";
import { SpacesStore } from "../core/spaces-store";
import { DEFAULT_SETTINGS, SettingsStore } from "../core/settings";
import { TauriBridge } from "../core/tauri";
import { UI_STORAGE } from "../core/ui-state";
import { SplitPane } from "../ui/helix-candidates/split-pane/split-pane";
import { SplitPaneStub } from "../ui/helix-candidates/split-pane/split-pane.testing";
import { Sidebar } from "./sidebar";
import { DEFAULT_SIDEBAR_ROWS } from "./spaces";
import { inlineSpaceCard } from "./space-card.mock";

const space: SpaceNode = {
  workspace: { space_id: "w1", label: "One", number: 1, agent_status: "idle" },
  rows: [
    {
      pane: { terminal_id: "w1:p1", tab_id: "w1:t1", space_id: "w1", agent_status: "idle" },
      tab: "t",
      name: "shell",
      worktree: false,
    },
  ],
};

async function render(records: ChildRecord[]): Promise<HTMLElement> {
  const node: SpaceNode = {
    ...space,
    rows: space.rows.map((r) => ({ ...r, pane: { ...r.pane, records } })),
  };
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      {
        provide: SpacesStore,
        useValue: {
          spaces: signal([node]),
          disconnected: signal<string | null>(null),
          ensureBackend: vi.fn(),
        },
      },
      {
        provide: SettingsStore,
        useValue: {
          settings: signal({
            ...DEFAULT_SETTINGS,
            sidebarRows: DEFAULT_SIDEBAR_ROWS,
          }),
          writeError: signal(null),
        },
      },
      { provide: TauriBridge, useValue: { openUrl: vi.fn() } },
      { provide: UI_STORAGE, useValue: null },
    ],
  });
  inlineSpaceCard();
  TestBed.overrideComponent(Sidebar, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/sidebar/sidebar.html", "utf8"),
    },
  });
  TestBed.overrideComponent(Sidebar, {
    remove: { imports: [SplitPane] },
    add: { imports: [SplitPaneStub] },
  });
  const fixture = TestBed.createComponent(Sidebar);
  for (let i = 0; i < 3; i++) {
    fixture.detectChanges();
    await fixture.whenStable();
  }
  return fixture.nativeElement as HTMLElement;
}

const many: ChildRecord[] = Array.from({ length: 5 }, (_, i) => ({
  id: `run/i${i}`,
  status: i === 0 ? "error" : "working",
}));

describe("Sidebar child records (#684)", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  });

  it("shows a few records as rows below the terminal", async () => {
    const root = await render([
      { id: "run", status: "working", progress: 40 },
      { id: "run/a", status: "blocked", kind: "permission", message: "Allow?" },
    ]);
    const rows = [...root.querySelectorAll(".record")].map((r) => r.textContent!.trim());
    expect(rows.length).toBe(2);
    expect(rows[0]).toContain("working 40%");
    expect(rows[1]).toContain("waiting: permission");
    expect(rows[1]).toContain("Allow?");
    expect(root.querySelector(".records-toggle")).toBeNull();
  });

  it("folds more than a few records into a summary until opened", async () => {
    const root = await render(many);
    expect(root.querySelectorAll(".record").length).toBe(0);
    const toggle = root.querySelector<HTMLButtonElement>(".records-toggle")!;
    expect(toggle.textContent).toContain("5 records · 1 error · 4 working");
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
  });

  it("has no record block without records", async () => {
    const root = await render([]);
    expect(root.querySelector(".records")).toBeNull();
  });
});
