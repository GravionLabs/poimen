// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { SpacesStore } from "../core/spaces-store";
import { PaneActions } from "../core/pane-actions";
import type { BackendKind } from "../core/settings";
import { TauriBridge } from "../core/tauri";
import { PaneDialog } from "./pane-dialog";

async function setup(backend: BackendKind, harnesses: unknown[] = []) {
  const store = {
    backendKind: signal(backend).asReadonly(),
    pane: () => undefined,
    snapshot: () => ({ tabs: [] }),
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
  actions.askStartAgent("p1");
  fixture.detectChanges();
  await fixture.whenStable();
  fixture.detectChanges();
  const root = fixture.nativeElement as HTMLElement;
  const options = [...root.querySelectorAll<HTMLOptionElement>('select[name="kind"] option')].map(
    (o) => o.value,
  );
  return { options, bridge, root };
}

describe("PaneDialog start agent (#410)", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
      teardown: { destroyAfterEach: true },
    });
  });

  it("offers only the kinds with hooks on the poimen backend", async () => {
    const { options } = await setup("poimen");
    expect(options).toEqual(["claude", "copilot", "gemini"]);
  });

  it("offers herdr's full list on the herdr backend", async () => {
    const { options } = await setup("herdr");
    expect(options).toHaveLength(24);
    expect(options).toContain("codex");
    expect(options).toContain("gemini");
  });

  it("starts a harness without hooks with its own program and arguments, not its id", async () => {
    const { bridge, root } = await setup("poimen", [
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
});
