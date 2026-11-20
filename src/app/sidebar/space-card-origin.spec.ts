// @vitest-environment jsdom
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import type { HxTreeNode } from "@gravionlabs/helix-ui";
import { beforeAll, describe, expect, it } from "vitest";
import { Machines, type MachineStatus } from "../core/machines";
import { SpacesStore } from "../core/spaces-store";
import { UI_STORAGE } from "../core/ui-state";
import type { SpaceData } from "./sidebar";
import { SpaceCard, originColor } from "./space-card";
import { inlineSpaceCard } from "./space-card.mock";

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized
  }
});

const status = (name: string, state: MachineStatus["state"] = "connected"): MachineStatus =>
  ({
    name,
    ssh: name,
    reposDir: "~",
    state,
    serverVersion: "1",
    error: "",
  }) as MachineStatus;

function node(id: string): HxTreeNode<SpaceData> {
  return {
    key: id,
    label: "api",
    data: {
      kind: "workspace",
      status: "idle",
      lines: [[{ kind: "name", text: "api" }]],
      workspace: {
        space_id: id,
        label: "api",
        number: 1,
        agent_status: "idle",
      },
      member: false,
    },
    children: [],
  } as unknown as HxTreeNode<SpaceData>;
}

function setup(id: string, machines: MachineStatus[], blocked: string | null = null) {
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      { provide: Machines, useValue: { machines: signal(machines) } },
      {
        provide: SpacesStore,
        useValue: {
          spaces: signal([]),
          remoteSpaces: () => [],
          pinnedSpaces: signal([]),
        },
      },
      {
        provide: UI_STORAGE,
        useValue: { getItem: () => null, setItem: () => undefined },
      },
    ],
  });
  inlineSpaceCard();
  const fixture = TestBed.createComponent(SpaceCard);
  fixture.componentInstance.node = node(id);
  fixture.componentInstance.blocked = blocked;
  fixture.detectChanges();
  const el = fixture.nativeElement as HTMLElement;
  return {
    el,
    card: el.querySelector(".card")!,
    mark: el.querySelector(".origin-mark"),
  };
}

describe("space card origin (#1074)", () => {
  it("shows the computer symbol on a local card, without a stripe", () => {
    const { card, mark } = setup("s1", [status("box")]);
    expect(mark?.getAttribute("aria-label")).toBe("This computer");
    expect(mark?.querySelector("title")?.textContent).toBe("This computer");
    expect(card.getAttribute("aria-label")).toBe("api, This computer");
    expect(card.hasAttribute("data-origin")).toBe(false);
  });

  it("shows the directory symbol also while no machine is configured, without naming the computer", () => {
    const { card, mark } = setup("s1", []);
    expect(mark?.getAttribute("aria-label")).toBe("This computer");
    expect(card.getAttribute("aria-label")).toBe("api");
  });

  it("shows the machine symbol, name and stripe on a remote card", () => {
    const { card, mark } = setup("box::s1", [status("box")]);
    expect(mark?.getAttribute("aria-label")).toBe("Machine box");
    expect(mark?.querySelector("title")?.textContent).toBe("Machine box");
    expect(mark?.getAttribute("role")).toBe("img");
    expect(card.getAttribute("data-origin")).toBe(String(originColor("box")));
    expect(card.getAttribute("aria-label")).toBe("api, Machine box");
  });

  it("is information: not a button, not focusable", () => {
    const { mark } = setup("box::s1", [status("box")]);
    expect(mark?.closest("button")).toBeNull();
    expect(mark?.hasAttribute("tabindex")).toBe(false);
  });

  it("gives the reason as the tooltip of a machine that is not connected", () => {
    const { card, mark } = setup(
      "box::s1",
      [status("box", "offline")],
      "box is offline: timed out",
    );
    expect(card.classList.contains("offline")).toBe(true);
    expect(mark?.getAttribute("aria-label")).toBe("Machine box");
    expect(mark?.querySelector("title")?.textContent).toBe(
      "Machine box: box is offline: timed out",
    );
  });

  it("follows a renamed machine: new name, label and stable color per name", () => {
    const old = setup("box::s1", [status("box")]);
    const renamed = setup("lab::s1", [status("lab")]);
    expect(renamed.mark?.getAttribute("aria-label")).toBe("Machine lab");
    expect(renamed.card.getAttribute("data-origin")).toBe(String(originColor("lab")));
    expect(originColor("lab")).toBe(originColor("lab"));
    expect(old.mark?.getAttribute("aria-label")).toBe("Machine box");
  });
});
