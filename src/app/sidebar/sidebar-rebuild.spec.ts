// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { SpaceNode } from "../core/herdr";
import { HerdrStore } from "../core/herdr-store";
import { SpaceRebuild, type RebuildMark } from "../core/space-rebuild";
import { SplitPane } from "../ui/helix-candidates/split-pane/split-pane";
import { SplitPaneStub } from "../ui/helix-candidates/split-pane/split-pane.testing";
import { Sidebar } from "./sidebar";

const space = (id: string, label: string, panes: number): SpaceNode => ({
  workspace: { space_id: id, label, number: 1, agent_status: "unknown", cwd: `/work/${label}` },
  rows: Array.from({ length: panes }, (_, i) => ({
    pane: { terminal_id: `${id}:p${i}`, tab_id: `${id}:t${i}`, space_id: id, agent_status: "idle" },
    tab: "shell",
    name: "shell",
    worktree: false,
  })),
});

async function setup(marks: Record<string, RebuildMark>, spaces: SpaceNode[]) {
  const rebuild = vi.fn(async () => {});
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      {
        provide: HerdrStore,
        useValue: {
          spaces: signal(spaces),
          disconnected: signal<string | null>(null),
          ensureBackend: vi.fn(),
        },
      },
      {
        provide: SpaceRebuild,
        useValue: { marks: signal(new Map(Object.entries(marks))), rebuild },
      },
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
  fixture.detectChanges();
  await fixture.whenStable();
  return { root: fixture.nativeElement as HTMLElement, rebuild };
}

describe("Sidebar rebuild of an empty space", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  });

  it("offers Rebuild with the reasons on an empty space that needs consent", async () => {
    const { root, rebuild } = await setup(
      { s1: { kind: "consent", reasons: ['pane "agent" starts an agent: claude'] } },
      [space("s1", "app", 0)],
    );
    const button = root.querySelector<HTMLButtonElement>("button.rebuild");
    expect(button?.getAttribute("aria-label")).toBe("Rebuild space app");
    expect(button?.title).toBe('Rebuild the layout: pane "agent" starts an agent: claude');
    button!.click();
    expect(rebuild).toHaveBeenCalledWith("s1");
  });

  it("shows a hint for a space whose directory is gone", async () => {
    const { root } = await setup({ s1: { kind: "missing", reason: "the directory is gone" } }, [
      space("s1", "app", 0),
    ]);
    const hint = root.querySelector<HTMLElement>(".rebuild-hint");
    expect(hint?.textContent?.trim()).toBe("Directory gone");
    expect(hint?.title).toBe("the directory is gone");
    expect(root.querySelector("button.rebuild")).toBeNull();
  });

  it("shows nothing once the space has terminals again", async () => {
    const { root } = await setup({ s1: { kind: "consent", reasons: [] } }, [space("s1", "app", 1)]);
    expect(root.querySelector("button.rebuild")).toBeNull();
  });
});
