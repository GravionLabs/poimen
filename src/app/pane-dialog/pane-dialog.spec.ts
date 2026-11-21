// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { SpacesStore } from "../core/spaces-store";
import { PaneActions } from "../core/pane-actions";
import { TauriBridge } from "../core/tauri";
import { PaneDialog } from "./pane-dialog";
import { expectFocusInside, focusOpener, press } from "../ui/dialog-shell/dialog-contract";

async function setup(
  harnesses: unknown[] = [],
  ask: (a: PaneActions) => void = (a) => a.askStartAgent("p1"),
) {
  const store = {
    pane: (id: string) => (id === "p9" ? { terminal_id: "p9" } : undefined),
    snapshot: () => ({ tabs: [], spaces: [], terminals: [] }),
    refresh: vi.fn(async () => undefined),
  };
  const bridge = {
    invoke: vi.fn(async (command: string): Promise<unknown> =>
      command === "get_harnesses" ? harnesses : undefined,
    ),
  };
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      { provide: TauriBridge, useValue: bridge },
      { provide: SpacesStore, useValue: store },
    ],
  });
  TestBed.overrideComponent(PaneDialog, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/pane-dialog/pane-dialog.html", "utf8"),
    },
  });
  const actions = TestBed.inject(PaneActions);
  const fixture = TestBed.createComponent(PaneDialog);
  ask(actions);
  fixture.detectChanges();
  await fixture.whenStable();
  fixture.detectChanges();
  const root = fixture.nativeElement as HTMLElement;
  const options = [...root.querySelectorAll<HTMLOptionElement>('select[name="kind"] option')].map(
    (o) => o.value,
  );
  return { options, bridge, root, actions, fixture };
}

describe("PaneDialog start agent (#410)", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
      teardown: { destroyAfterEach: true },
    });
  });

  it("offers only the kinds with hooks", async () => {
    const { options } = await setup();
    expect(options).toEqual(["claude", "copilot", "gemini"]);
  });

  it("starts a harness without hooks with its own program and arguments, not its id", async () => {
    const { bridge, root } = await setup([
      { id: "aider", command: ["aider-bin", "--no-auto-commits"], available: true },
    ]);
    const form = root.querySelector<HTMLFormElement>("form:has(select[name='kind'])")!;
    form.querySelector<HTMLSelectElement>("select[name='kind']")!.value = "aider";
    form.querySelector<HTMLInputElement>("[name='args']")!.value = "--yes";
    form.requestSubmit();
    await vi.waitFor(() =>
      expect(bridge.invoke).toHaveBeenCalledWith("agent_start", {
        terminalId: "p1",
        kind: "aider-bin",
        name: null,
        args: ["--no-auto-commits", "--yes"],
      }),
    );
  });

  it("start agent: the first field has the focus, Escape cancels and the focus goes back", async () => {
    const opener = focusOpener();
    const { root, actions, fixture } = await setup();
    expectFocusInside(root);
    expect(document.activeElement?.getAttribute("name")).toBe("kind");
    press("Escape");
    fixture.detectChanges();
    expect(actions.dialog()).toBeNull();
    expect(document.activeElement).toBe(opener);
  });

  it("new pane: the type has the focus, Terminal is the default and Enter creates it (#1049)", async () => {
    const { root, bridge } = await setup([], (a) => a.askNew("w1"));
    expectFocusInside(root);
    expect(root.querySelector("input[name='directory'], input[type='text']")).toBeNull();
    const checked = root.querySelector<HTMLInputElement>("input[name='type']:checked");
    expect(checked?.value).toBe("terminal");
    root.querySelector<HTMLFormElement>("form")!.requestSubmit();
    await vi.waitFor(() =>
      expect(bridge.invoke).toHaveBeenCalledWith("terminal_create", {
        spaceId: "w1",
        cwd: null,
      }),
    );
    expect(bridge.invoke).not.toHaveBeenCalledWith("agent_start", expect.anything());
  });

  it("new pane: a space without installed agents offers only Terminal", async () => {
    const { root } = await setup([{ id: "claude", command: ["claude"], available: false }], (a) =>
      a.askNew("w1"),
    );
    const types = [...root.querySelectorAll<HTMLInputElement>("input[name='type']")];
    expect(types.map((t) => t.value)).toEqual(["terminal"]);
  });

  it("new pane: an agent type starts the agent in the new pane", async () => {
    const { root, bridge, fixture } = await setup(
      [
        { id: "claude", command: ["claude"], available: true },
        {
          id: "aider",
          command: ["aider-bin", "--no-auto-commits"],
          available: true,
        },
        { id: "gemini", command: ["gemini"], available: false },
      ],
      (a) => a.askNew("w1"),
    );
    const types = [...root.querySelectorAll<HTMLInputElement>("input[name='type']")];
    expect(types.map((t) => t.value)).toEqual(["terminal", "claude", "aider"]);
    bridge.invoke.mockImplementation(async (command: string): Promise<unknown> =>
      command === "terminal_create" ? "p9" : undefined,
    );
    types[2]!.checked = true;
    root.querySelector<HTMLFormElement>("form")!.requestSubmit();
    await vi.waitFor(() =>
      expect(bridge.invoke).toHaveBeenCalledWith("agent_start", {
        terminalId: "p9",
        kind: "aider-bin",
        name: null,
        args: ["--no-auto-commits"],
      }),
    );
    fixture.detectChanges();
  });
});
