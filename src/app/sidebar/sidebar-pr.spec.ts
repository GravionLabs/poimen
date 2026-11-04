// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { BranchPull, SpaceNode } from "../core/herdr";
import { HerdrStore } from "../core/herdr-store";
import { DEFAULT_SETTINGS, SettingsStore } from "../core/settings";
import { TauriBridge } from "../core/tauri";
import { UI_STORAGE } from "../core/ui-state";
import { SplitPane } from "../ui/helix-candidates/split-pane/split-pane";
import { SplitPaneStub } from "../ui/helix-candidates/split-pane/split-pane.testing";
import { Sidebar } from "./sidebar";
import { DEFAULT_SIDEBAR_ROWS } from "./spaces";

const pull = (over: Partial<BranchPull> = {}): BranchPull => ({
  number: 123,
  title: "Add the thing",
  url: "https://github.com/o/r/pull/123",
  state: "open",
  isDraft: false,
  checks: { state: "failing", passed: 3, failed: 1, pending: 0 },
  ...over,
});

const space = (pr: BranchPull): SpaceNode => ({
  workspace: { workspace_id: "w1", label: "One", number: 1, agent_status: "idle" },
  rows: [
    {
      pane: { pane_id: "w1:p1", tab_id: "w1:t1", workspace_id: "w1", agent_status: "idle" },
      tab: "t",
      name: "shell",
      worktree: false,
      branch: "feat/x",
      pr,
    },
  ],
});

async function setup(pr: BranchPull) {
  TestBed.resetTestingModule();
  const openUrl = vi.fn(async () => undefined);
  const settings = signal({
    ...DEFAULT_SETTINGS,
    sidebarRows: {
      ...DEFAULT_SIDEBAR_ROWS,
      workspace: [[{ token: "name" }, { token: "pr" }]],
    },
  });
  TestBed.configureTestingModule({
    providers: [
      {
        provide: HerdrStore,
        useValue: {
          spaces: signal([space(pr)]),
          disconnected: signal<string | null>(null),
          ensureBackend: vi.fn(),
        },
      },
      { provide: SettingsStore, useValue: { settings, writeError: signal(null) } },
      { provide: TauriBridge, useValue: { openUrl } },
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
  const selected: string[] = [];
  fixture.componentInstance.selected.subscribe((p) => selected.push(p.pane_id));
  for (let i = 0; i < 3; i++) {
    fixture.detectChanges();
    await fixture.whenStable();
  }
  return { root: fixture.nativeElement as HTMLElement, openUrl, selected };
}

describe("Sidebar pr token", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  });

  it("renders the number and check symbol with a tooltip", async () => {
    const { root } = await setup(pull());
    const link = root.querySelector<HTMLButtonElement>(".pr-link")!;
    expect(link.textContent?.trim()).toBe("#123 ✗");
    expect(link.title).toBe("#123 Add the thing (open)\nChecks: 3 passed, 1 failed");
  });

  it("renders merged and dims a draft", async () => {
    expect(
      (await setup(pull({ state: "merged" }))).root.querySelector(".pr-link")?.textContent,
    ).toContain("merged");
    const { root } = await setup(pull({ isDraft: true }));
    expect(root.querySelector(".pr-link")?.classList.contains("dim")).toBe(true);
  });

  it("opens the pull request in the browser on a click, without selecting the row", async () => {
    const { root, openUrl, selected } = await setup(pull());
    root.querySelector<HTMLButtonElement>(".pr-link")!.click();
    expect(openUrl).toHaveBeenCalledWith("https://github.com/o/r/pull/123");
    expect(selected).toEqual([]);
  });

  it("never opens a url that is not on github.com", async () => {
    const { root, openUrl } = await setup(pull({ url: "https://evil.example/pull/1" }));
    root.querySelector<HTMLButtonElement>(".pr-link")!.click();
    expect(openUrl).not.toHaveBeenCalled();
  });
});
