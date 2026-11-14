// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { SpacesStore } from "../core/spaces-store";
import { SpaceActions } from "../core/space-actions";
import { TauriBridge } from "../core/tauri";
import { TrustDialog } from "./trust-dialog";

const bridge = { invoke: vi.fn(), pickDirectory: vi.fn() };
const store = { refresh: vi.fn(async () => undefined), snapshot: () => ({}) };

const request = {
  root: "/work/app",
  files: ["/work/app/.poimen/config.json"],
  hash: "h1",
  commands: [
    { what: 'layout: pane "agent"', command: "claude" },
    { what: "setup", command: "pnpm install" },
  ],
};
const created = { space_id: "w2", created: true };

async function setup() {
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      { provide: TauriBridge, useValue: bridge },
      { provide: SpacesStore, useValue: store },
    ],
  });
  TestBed.overrideComponent(TrustDialog, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/trust-dialog/trust-dialog.html", "utf8"),
    },
  });
  const actions = TestBed.inject(SpaceActions);
  const fixture = TestBed.createComponent(TrustDialog);
  const root = fixture.nativeElement as HTMLElement;
  const settle = async () => {
    fixture.detectChanges();
    await fixture.whenStable();
    fixture.detectChanges();
  };
  // The first create_space answers with the request; later ones with the created space.
  bridge.invoke.mockImplementation(async (command: string, args?: { decision?: unknown }) => {
    if (command !== "create_space") return [];
    return args?.decision ? created : { space_id: "", created: false, trust: request };
  });
  await actions.create("/work/app", "app");
  await settle();
  const click = async (selector: string) => {
    root.querySelector<HTMLButtonElement>(selector)!.click();
    await settle();
  };
  const createCalls = () => bridge.invoke.mock.calls.filter(([c]) => c === "create_space");
  return { actions, root, click, createCalls };
}

describe("TrustDialog", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
      teardown: { destroyAfterEach: true },
    });
  });

  beforeEach(() => bridge.invoke.mockReset());

  it("lists every command and creates nothing before the answer", async () => {
    const { root, createCalls } = await setup();
    const items = [...root.querySelectorAll(".commands li")].map((li) => li.textContent);
    expect(items).toHaveLength(2);
    expect(items[0]).toContain("claude");
    expect(items[1]).toContain("pnpm install");
    expect(root.textContent).toContain("/work/app");
    expect(createCalls()).toHaveLength(1);
    expect(createCalls()[0][1].decision).toBeNull();
  });

  it("Trust and run sends the hash it listed", async () => {
    const { root, click, createCalls } = await setup();
    await click(".confirm");
    expect(createCalls()[1][1]).toEqual({
      cwd: "/work/app",
      label: "app",
      decision: { kind: "trust", hash: "h1" },
      command: null,
    });
    expect(root.querySelector("[role=dialog]")).toBeNull();
  });

  it("Run without project commands sends the without answer", async () => {
    const { root, click, createCalls } = await setup();
    await click(".without");
    expect(createCalls()[1][1].decision).toEqual({ kind: "without" });
    expect(root.querySelector("[role=dialog]")).toBeNull();
  });

  it("Cancel creates nothing and closes the dialog", async () => {
    const { root, click, createCalls } = await setup();
    await click(".cancel");
    expect(createCalls()).toHaveLength(1);
    expect(root.querySelector("[role=dialog]")).toBeNull();
  });

  it("asks again when the file changed before the answer", async () => {
    const { root, click, createCalls } = await setup();
    // The backend sees another hash: it answers with a new request instead of creating.
    bridge.invoke.mockImplementation(async (command: string) =>
      command === "create_space"
        ? { space_id: "", created: false, trust: { ...request, hash: "h2" } }
        : [],
    );
    await click(".confirm");
    expect(createCalls()).toHaveLength(2);
    expect(root.querySelector("[role=dialog]")).not.toBeNull();
  });
});
