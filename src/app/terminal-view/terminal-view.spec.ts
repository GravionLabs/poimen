// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { Pane } from "../core/herdr";
import { SettingsStore, DEFAULT_SETTINGS, type Settings } from "../core/settings";
import { TerminalStreamService } from "../core/terminal-stream";

const fake = vi.hoisted(() => ({ cols: 80, rows: 24, nextCols: 80 }));

vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    options: Record<string, unknown> = {};
    get cols() {
      return fake.cols;
    }
    get rows() {
      return fake.rows;
    }
    loadAddon() {}
    open() {}
    onData() {}
    attachCustomWheelEventHandler() {}
    focus() {}
    write() {}
    dispose() {}
  },
}));
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit() {
      fake.cols = fake.nextCols;
    }
  },
}));
vi.mock("@xterm/addon-webgl", () => ({ WebglAddon: class {} }));

import { TerminalView } from "./terminal-view";

const pane: Pane = { pane_id: "w1:p1", tab_id: "w1:t1", workspace_id: "w1", agent_status: "idle" };

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
    errorOnUnknownElements: false,
  });
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
});

afterEach(() => TestBed.resetTestingModule());

async function setup(mode: "control" | "observe") {
  fake.cols = 80;
  fake.nextCols = 80;
  const settings = signal<Settings>(DEFAULT_SETTINGS);
  const session = { open: true, resize: vi.fn(), close: vi.fn(), input: vi.fn(), scroll: vi.fn() };
  const open = vi.fn<(opts: unknown) => Promise<typeof session>>(async () => session);
  TestBed.configureTestingModule({
    providers: [
      { provide: SettingsStore, useValue: { settings } },
      { provide: TerminalStreamService, useValue: { open } },
    ],
  });
  TestBed.overrideComponent(TerminalView, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      imports: [],
      template: readFileSync("src/app/terminal-view/terminal-view.html", "utf8"),
    },
  });
  const fixture = TestBed.createComponent(TerminalView);
  // JIT without the Angular compiler transform cannot resolve signal queries: supply the element.
  const body = document.createElement("div");
  // The same goes for signal inputs.
  Object.assign(fixture.componentInstance, {
    body: () => ({ nativeElement: body }),
    pane: signal(pane),
    mode: signal(mode),
  });
  const sized: { cols: number; rows: number }[] = [];
  fixture.componentInstance.sized.subscribe((s) => sized.push(s));
  fixture.detectChanges();
  await fixture.whenStable();
  await new Promise((resolve) => setTimeout(resolve)); // the stream open resolves
  return { fixture, settings, session, open, sized };
}

describe("TerminalView font change", () => {
  it("resizes the control stream and emits sized when the font changes cols", async () => {
    const { fixture, settings, session, sized } = await setup("control");
    expect(sized).toEqual([{ cols: 80, rows: 24 }]);
    fake.nextCols = 100;
    settings.set({ ...DEFAULT_SETTINGS, fontSize: 10 });
    fixture.detectChanges();
    expect(sized.at(-1)).toEqual({ cols: 100, rows: 24 });
    expect(session.resize).toHaveBeenCalledWith(100, 24);
  });

  it("reopens the observe stream with the new size", async () => {
    const { fixture, settings, open } = await setup("observe");
    fake.nextCols = 100;
    settings.set({ ...DEFAULT_SETTINGS, fontSize: 10 });
    fixture.detectChanges();
    await fixture.whenStable();
    expect(open).toHaveBeenCalledTimes(2);
    expect(open.mock.calls[1][0]).toMatchObject({ cols: 100, rows: 24 });
  });

  it("does nothing when the font change keeps cols and rows", async () => {
    const { fixture, settings, session, sized } = await setup("control");
    settings.set({ ...DEFAULT_SETTINGS, fontSize: 14 });
    fixture.detectChanges();
    expect(sized).toHaveLength(1);
    expect(session.resize).not.toHaveBeenCalled();
  });
});
