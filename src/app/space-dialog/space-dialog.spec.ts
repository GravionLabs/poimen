// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { SpacesStore } from "../core/spaces-store";
import { SpaceActions } from "../core/space-actions";
import { TauriBridge } from "../core/tauri";
import { SpaceDialog } from "./space-dialog";

const bridge = { invoke: vi.fn(), pickDirectory: vi.fn() };
const snapshot = {
  spaces: [
    { space_id: "w1", label: "One", number: 1, agent_status: "idle" },
    { space_id: "w2", label: "Two", number: 2, agent_status: "idle" },
  ],
  tabs: [],
  terminals: [
    {
      terminal_id: "w1:p1",
      tab_id: "w1:t1",
      space_id: "w1",
      agent_status: "working",
      agent: "claude",
    },
  ],
};
const store = {
  refresh: vi.fn(async () => undefined),
  snapshot: () => snapshot,
  spaceOrder: () => [],
};

async function setup() {
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      { provide: TauriBridge, useValue: bridge },
      { provide: SpacesStore, useValue: store },
    ],
  });
  TestBed.overrideComponent(SpaceDialog, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/space-dialog/space-dialog.html", "utf8"),
    },
  });
  const actions = TestBed.inject(SpaceActions);
  const fixture = TestBed.createComponent(SpaceDialog);
  const root = fixture.nativeElement as HTMLElement;
  const settle = async () => {
    fixture.detectChanges();
    await fixture.whenStable();
    fixture.detectChanges();
  };
  const type = async (selector: string, value: string) => {
    const input = root.querySelector<HTMLInputElement>(selector)!;
    input.value = value;
    input.dispatchEvent(new Event("input"));
    await settle();
  };
  return { actions, root, settle, type };
}

describe("SpaceDialog", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
      teardown: { destroyAfterEach: true },
    });
  });

  beforeEach(() => {
    bridge.invoke.mockReset();
    bridge.pickDirectory.mockReset();
  });

  it("renders nothing until asked", async () => {
    const { root, settle } = await setup();
    await settle();
    expect(root.querySelector("form")).toBeNull();
  });

  it("defaults the label to the directory name and creates the space", async () => {
    const { actions, root, settle, type } = await setup();
    bridge.invoke.mockImplementation(async (command: string) =>
      command === "create_space" ? { space_id: "w2", created: true } : [],
    );
    actions.ask();
    await settle();
    await type(".path", "/work/app");
    expect(root.querySelector<HTMLInputElement>("input[name=label]")!.value).toBe("app");
    root.querySelector("form")!.dispatchEvent(new Event("submit"));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith("create_space", {
      cwd: "/work/app",
      label: "app",
      decision: null,
      command: null,
    });
    expect(root.querySelector("form")).toBeNull();
  });

  it("offers completions and fills the path from the folder picker", async () => {
    const { actions, root, settle, type } = await setup();
    bridge.invoke.mockResolvedValue(["/work/app", "/work/apps"]);
    bridge.pickDirectory.mockResolvedValue("/picked/dir");
    actions.ask();
    await settle();
    await type(".path", "/work/ap");
    expect(root.querySelectorAll("datalist option")).toHaveLength(2);
    root.querySelector<HTMLButtonElement>(".browse")!.click();
    await vi.waitFor(() => expect(bridge.pickDirectory).toHaveBeenCalled());
    await new Promise((resolve) => setTimeout(resolve));
    await settle();
    expect(root.querySelector<HTMLInputElement>(".path")!.value).toBe("/picked/dir");
  });

  describe("template hint", () => {
    const hint = (root: HTMLElement) =>
      root.querySelector("[data-testid=template-hint]")?.textContent?.replace(/\s+/g, " ").trim();

    it("shows nothing until a directory is typed", async () => {
      const { actions, root, settle } = await setup();
      bridge.invoke.mockResolvedValue(null);
      actions.ask();
      await settle();
      expect(hint(root)).toBeUndefined();
    });

    it("names the template and its agent for the typed directory", async () => {
      const { actions, root, settle, type } = await setup();
      bridge.invoke.mockImplementation(async (command: string) =>
        command === "resolve_space_template" ? { name: "GravionLabs", agent: "claude" } : [],
      );
      actions.ask();
      await settle();
      await type(".path", "/work/app");
      expect(bridge.invoke).toHaveBeenCalledWith("resolve_space_template", { cwd: "/work/app" });
      expect(hint(root)).toBe("Template: GravionLabs · Claude");
    });

    it("says no template when none applies", async () => {
      const { actions, root, settle, type } = await setup();
      bridge.invoke.mockResolvedValue(null);
      actions.ask();
      await settle();
      await type(".path", "/elsewhere");
      expect(hint(root)).toBe("No template");
    });

    it("marks the default template", async () => {
      const { actions, root, settle, type } = await setup();
      bridge.invoke.mockImplementation(async (command: string) =>
        command === "resolve_space_template"
          ? { name: "default", agent: "claude", default: true }
          : [],
      );
      actions.ask();
      await settle();
      await type(".path", "/elsewhere");
      expect(hint(root)).toBe("Template: default (default) · Claude");
    });

    it("says no template when the lookup fails", async () => {
      const { actions, root, settle, type } = await setup();
      bridge.invoke.mockImplementation(async (command: string) => {
        if (command === "resolve_space_template") throw "boom";
        return [];
      });
      actions.ask();
      await settle();
      await type(".path", "/work/app");
      expect(hint(root)).toBe("No template");
    });

    it("looks up the picked directory, and only the newest answer counts", async () => {
      const { actions, root, settle } = await setup();
      let release: (v: unknown) => void = () => undefined;
      bridge.invoke.mockImplementation(async (command: string, args: { cwd?: string }) => {
        if (command !== "resolve_space_template") return [];
        if (args.cwd === "/slow") return new Promise((r) => (release = r));
        return { name: "Picked", agent: "copilot" };
      });
      bridge.pickDirectory.mockResolvedValue("/picked");
      actions.ask();
      await settle();
      const slow = actions.lookupTemplate("/slow");
      root.querySelector<HTMLButtonElement>(".browse")!.click();
      await vi.waitFor(() => expect(bridge.pickDirectory).toHaveBeenCalled());
      await new Promise((resolve) => setTimeout(resolve));
      release({ name: "Old", agent: "claude" });
      await slow;
      await settle();
      expect(hint(root)).toBe("Template: Picked · Copilot");
    });
  });

  it("shows the error in the dialog and keeps it open", async () => {
    const { actions, root, settle, type } = await setup();
    bridge.invoke.mockImplementation(async (command: string) => {
      if (command === "create_space") throw "/nope is not a directory.";
      return [];
    });
    actions.ask();
    await settle();
    await type(".path", "/nope");
    root.querySelector("form")!.dispatchEvent(new Event("submit"));
    await settle();
    expect(root.querySelector("[role=alert]")?.textContent).toContain("is not a directory");
    expect(root.querySelector("form")).not.toBeNull();
  });

  it("asks before closing a space with panes, then closes it and reports the panes", async () => {
    const { actions, root, settle } = await setup();
    bridge.invoke.mockResolvedValue(undefined);
    const closed = vi.fn();
    void actions.requestClose("w1");
    await settle();
    const dialog = root.querySelector("[role=dialog]")!;
    expect(dialog.textContent).toContain("Close space One?");
    expect(dialog.textContent).toContain("1 pane, 1 agent among them");
    expect(bridge.invoke).not.toHaveBeenCalled();
    const paneIds = actions.confirmClose().then(closed);
    await paneIds;
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith("space_close", { spaceId: "w1", focus: "w2" });
    expect(closed).toHaveBeenCalledWith(["w1:p1"]);
    expect(root.querySelector("[role=dialog]")).toBeNull();
  });

  it("closes an empty space without a dialog", async () => {
    const { actions, root, settle } = await setup();
    bridge.invoke.mockResolvedValue(undefined);
    await actions.requestClose("w2");
    await settle();
    expect(root.querySelector("[role=dialog]")).toBeNull();
    expect(bridge.invoke).toHaveBeenCalledWith("space_close", { spaceId: "w2", focus: "w1" });
  });

  it("keeps the space and shows herdr's error when the close fails", async () => {
    const { actions, root, settle } = await setup();
    bridge.invoke.mockRejectedValue("herdr: cannot close");
    await actions.requestClose("w2");
    await settle();
    expect(root.querySelector("[role=alert]")?.textContent).toContain("herdr: cannot close");
  });
});
