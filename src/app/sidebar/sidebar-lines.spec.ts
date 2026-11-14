// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { SpaceNode } from "../core/snapshot";
import { SpacesStore } from "../core/spaces-store";
import { DEFAULT_SETTINGS, SettingsStore } from "../core/settings";
import { TauriBridge } from "../core/tauri";
import { UI_STORAGE } from "../core/ui-state";
import { SplitPane } from "../ui/helix-candidates/split-pane/split-pane";
import { SplitPaneStub } from "../ui/helix-candidates/split-pane/split-pane.testing";
import { Sidebar } from "./sidebar";
import { DEFAULT_SIDEBAR_ROWS, type SidebarRows } from "./spaces";

const space: SpaceNode = {
  workspace: { space_id: "w1", label: "One", number: 1, agent_status: "idle" },
  rows: [
    {
      pane: { terminal_id: "w1:p1", tab_id: "w1:t1", space_id: "w1", agent_status: "idle" },
      tab: "t",
      name: "shell",
      worktree: false,
      branch: "feat/x",
    },
  ],
};

async function render(
  workspace: SidebarRows["workspace"],
  opts: { onBase?: boolean; showBase?: boolean } = {},
): Promise<HTMLElement> {
  const node: SpaceNode = {
    ...space,
    rows: space.rows.map((r) => ({ ...r, onBase: opts.onBase ?? false })),
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
            sidebarRows: { ...DEFAULT_SIDEBAR_ROWS, workspace },
            sidebarShowBaseBranch: opts.showBase ?? false,
          }),
          writeError: signal(null),
        },
      },
      { provide: TauriBridge, useValue: { openUrl: vi.fn() } },
      { provide: UI_STORAGE, useValue: null },
    ],
  });
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

const lineClasses = (root: HTMLElement) =>
  [...root.querySelectorAll(".space-main > .line")].map((l) => l.classList.contains("under-dot"));

describe("Sidebar space lines", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  });

  it("indents the lines below a first line that starts with the status dot", async () => {
    const root = await render(DEFAULT_SIDEBAR_ROWS.workspace);
    expect(lineClasses(root)).toEqual([false, true]);
  });

  it("does not indent when the first line has no leading dot", async () => {
    const root = await render([
      [{ token: "name" }, { token: "state_icon" }],
      [{ token: "branch" }],
    ]);
    expect(lineClasses(root)).toEqual([false, false]);
  });

  it("shows the branch token of a space off the base branch", async () => {
    const root = await render(DEFAULT_SIDEBAR_ROWS.workspace);
    expect(root.querySelector(".space-main")!.textContent).toContain("feat/x");
  });

  it("leaves the branch token out on the base branch unless showBaseBranch is set", async () => {
    const hidden = await render(DEFAULT_SIDEBAR_ROWS.workspace, { onBase: true });
    expect(hidden.querySelector(".space-main")!.textContent).not.toContain("feat/x");
    const shown = await render(DEFAULT_SIDEBAR_ROWS.workspace, { onBase: true, showBase: true });
    expect(shown.querySelector(".space-main")!.textContent).toContain("feat/x");
  });
});
