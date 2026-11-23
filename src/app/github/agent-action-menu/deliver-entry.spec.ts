// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { tauri } from "../../core/tauri.mock";
import { DeliverLauncher } from "../../deliver/deliver-launcher";
import { AgentActions, type ActionTarget } from "../agent-actions";
import { AgentActionMenu } from "./agent-action-menu";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../../core/tauri.mock")).tauri.listen,
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by the shared setup
  }
});

const target = (over: Partial<ActionTarget>): ActionTarget => ({
  kind: "issue",
  repo: "o/app",
  root: "/work/app",
  number: 1,
  title: "t",
  url: "",
  branch: "",
  base: "",
  level: null,
  labels: [],
  hasChildren: false,
  checks: null,
  conflicting: false,
  crossRepo: false,
  state: "open",
  review: null,
  ...over,
});

async function openMenu(targets: ActionTarget[]) {
  tauri.reset();
  tauri.answer("get_agent_actions", () => ({ actions: [] }));
  tauri.answer("task_modes", () => ({ modes: [] }));
  TestBed.resetTestingModule();
  TestBed.overrideComponent(AgentActionMenu, {
    set: {
      templateUrl: undefined,
      template: readFileSync("src/app/github/agent-action-menu/agent-action-menu.html", "utf8"),
    },
  });
  const fixture = TestBed.createComponent(AgentActionMenu);
  TestBed.inject(AgentActions).openMenu(targets, 10, 10);
  fixture.detectChanges();
  await fixture.whenStable();
  fixture.detectChanges();
  return fixture.nativeElement as HTMLElement;
}

describe("Deliver… in the row menu of the issues tree (#746)", () => {
  beforeEach(() => tauri.reset());

  it("opens the dialog for the row's issue", async () => {
    const el = await openMenu([target({ number: 12 })]);
    const item = el.querySelector<HTMLElement>("[data-testid=menu-deliver]")!;
    expect(item.textContent).toContain("Deliver…");
    item.click();
    expect(TestBed.inject(DeliverLauncher).opened()).toMatchObject({
      root: "/work/app",
      target: { kind: "issues", numbers: [12] },
    });
    expect(TestBed.inject(AgentActions).menu()).toBeNull();
  });

  it("opens it for all marked issues of the repository", async () => {
    const el = await openMenu([target({ number: 12 }), target({ number: 15 })]);
    el.querySelector<HTMLElement>("[data-testid=menu-deliver]")!.click();
    expect(TestBed.inject(DeliverLauncher).opened()?.target).toEqual({
      kind: "issues",
      numbers: [12, 15],
    });
  });

  it("is not offered for a pull request, a closed issue or issues of two repositories", async () => {
    for (const targets of [
      [target({ kind: "pr" })],
      [target({ state: "closed" })],
      [target({}), target({ number: 2, root: "/work/other" })],
    ]) {
      const el = await openMenu(targets);
      expect(el.querySelector("[data-testid=menu-deliver]")).toBeNull();
    }
  });
});
