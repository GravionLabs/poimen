// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it } from "vitest";
import { keptCardIds } from "../../core/card-marks";
import { Machines, type MachineStatus } from "../../core/machines";
import { buildSpaces, buildTree, qualifySnapshot, type Snapshot } from "../../core/snapshot";
import { SpacesStore } from "../../core/spaces-store";
import { UI_STATE_KEY, UI_STORAGE, UiState } from "../../core/ui-state";
import { spaceNodes } from "../sidebar";
import type { SidebarMenuAction } from "../sidebar-menu";
import { SpaceCard } from "../space-card";
import { MachinesSection } from "./machines-section";

const box = { name: "box", ssh: "box", reposDir: "~/Source/{owner}/{repo}" };
const snap: Snapshot = {
  spaces: [{ space_id: "s1", label: "api", number: 1, agent_status: "idle" }],
  tabs: [
    {
      tab_id: "tab1",
      space_id: "s1",
      label: "shell",
      number: 1,
      agent_status: "idle",
    },
  ],
  terminals: [{ terminal_id: "t1", tab_id: "tab1", space_id: "s1", agent_status: "idle" }],
};

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized
  }
});

async function setup(status: MachineStatus, stored?: string) {
  const blocked = status.state === "connected" ? null : `box is ${status.state}`;
  const tree = buildSpaces(qualifySnapshot("box", snap, status.state !== "connected"), new Map());
  const storage = new Map<string, string>(stored ? [[UI_STATE_KEY, stored]] : []);
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      {
        provide: Machines,
        useValue: {
          machines: signal([status]),
          checks: signal({}),
          blockedReason: (id: string) => (id.startsWith("box::") ? blocked : null),
        },
      },
      {
        provide: SpacesStore,
        useValue: {
          remoteTree: () => buildTree(qualifySnapshot("box", snap, status.state !== "connected")),
          remoteSpaces: () => tree,
          spaces: signal([]),
          pinnedSpaces: signal([]),
        },
      },
      {
        provide: UI_STORAGE,
        useValue: {
          getItem: (k: string) => storage.get(k) ?? null,
          setItem: (k: string, v: string) => void storage.set(k, v),
        },
      },
    ],
  });
  TestBed.overrideComponent(SpaceCard, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/sidebar/space-card.html", "utf8"),
    },
  });
  const fixture = TestBed.createComponent(MachinesSection);
  fixture.componentInstance.nodes = new Map([["box", spaceNodes(tree)]]);
  const picked: string[] = [];
  const acts: SidebarMenuAction[] = [];
  fixture.componentInstance.selected.subscribe((t) => picked.push(t.terminal_id));
  fixture.componentInstance.cardAct.subscribe((a) => acts.push(a));
  for (let i = 0; i < 3; i++) {
    fixture.detectChanges();
    await fixture.whenStable();
  }
  return {
    fixture,
    el: fixture.nativeElement as HTMLElement,
    picked,
    acts,
    storage,
  };
}

describe("MachinesSection cards (#938)", () => {
  it("shows a connected machine's space as a card that routes by the qualified id", async () => {
    const { el, picked, acts } = await setup({
      ...box,
      state: "connected",
      serverVersion: "0.17.0",
    });
    expect(el.querySelectorAll("app-space-card")).toHaveLength(1);
    expect(el.querySelector(".card")?.getAttribute("aria-label")).toBe("api, Machine box");
    el.querySelector<HTMLElement>(".row")!.click();
    expect(picked).toEqual(["box::t1"]);
    el.querySelector<HTMLButtonElement>("[aria-label='New terminal in api']")!.click();
    expect(acts).toEqual([{ kind: "workspace", id: "box::s1", action: "new-terminal" }]);
  });

  it("offers no configure button, pin mark or drag for a remote card", async () => {
    const { el } = await setup({
      ...box,
      state: "connected",
      serverVersion: "0.17.0",
    });
    expect(el.querySelector("[aria-label='Configure space api']")).toBeNull();
    expect(el.querySelector(".head")?.getAttribute("draggable")).toBeNull();
    expect(el.querySelector(".row")?.getAttribute("draggable")).toBeNull();
  });

  it("greys the cards of a machine that is not connected and disables the actions with the reason", async () => {
    const { el, picked, acts } = await setup({
      ...box,
      state: "offline",
      error: "x",
    });
    expect(el.querySelector(".card")?.classList.contains("offline")).toBe(true);
    const close = el.querySelector<HTMLButtonElement>("[aria-label='New terminal in api']")!;
    expect(close.disabled).toBe(true);
    expect(close.title).toBe("box is offline");
    expect(
      el.querySelector<HTMLButtonElement>("[aria-label='More actions for api']")!.disabled,
    ).toBe(true);
    el.querySelector<HTMLElement>(".row")!.click();
    close.click();
    expect(picked).toEqual([]);
    expect(acts).toEqual([]);
  });

  it("persists the collapsed state under the qualified id", async () => {
    const { el, fixture, storage } = await setup({
      ...box,
      state: "connected",
      serverVersion: "0.17.0",
    });
    el.querySelector<HTMLButtonElement>(".fold")!.click();
    fixture.detectChanges();
    await fixture.whenStable();
    expect(el.querySelector(".card")?.classList.contains("collapsed")).toBe(true);
    expect(TestBed.inject(UiState).collapsedCards()).toEqual(["box::s1"]);
    expect(storage.get(UI_STATE_KEY)).toContain("box::s1");
  });

  it("keeps the fold mark of a machine that lists no space yet", async () => {
    expect(keptCardIds(["s1"], ["s1", "box::s1"])).toEqual(["s1", "box::s1"]);
    expect(keptCardIds(["s1", "box::s2"], ["box::s1"])).toEqual(["s1", "box::s2"]);
  });
});
