// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { Pane, StreamRecord } from "../core/herdr";
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

import { TerminalView, isControllerHeld } from "./terminal-view";

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

async function setup(wanted: "control" | "observe") {
  fake.cols = 80;
  fake.nextCols = 80;
  const settings = signal<Settings>(DEFAULT_SETTINGS);
  const session = { open: true, resize: vi.fn(), close: vi.fn(), input: vi.fn(), scroll: vi.fn() };
  const records: ((record: StreamRecord) => void)[] = [];
  const open = vi.fn(async (_opts: unknown, onRecord: (record: StreamRecord) => void) => {
    records.push(onRecord);
    return { ...session };
  });
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
    wanted: signal(wanted),
  });
  const modes: { mode: string; blocked: string | null }[] = [];
  fixture.componentInstance.modeChange.subscribe((m) => modes.push(m));
  const sized: { cols: number; rows: number }[] = [];
  fixture.componentInstance.sized.subscribe((s) => sized.push(s));
  fixture.detectChanges();
  await fixture.whenStable();
  await new Promise((resolve) => setTimeout(resolve)); // the stream open resolves
  /** Delivers a record to the n-th opened stream, as the backend event would. */
  const send = async (n: number, record: StreamRecord) => {
    records[n](record);
    await new Promise((resolve) => setTimeout(resolve));
  };
  return { fixture, settings, session, open, sized, modes, send };
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

describe("TerminalView input mode decision", () => {
  const closed = (reason: string): StreamRecord => ({ type: "closed", reason });
  const frame: StreamRecord = {
    type: "frame",
    seq: 1,
    full: true,
    width: 80,
    height: 24,
    bytes: "",
  };
  const modeOf = (call: unknown) => call as { mode: string; takeover: boolean };

  it("opens in input mode without a takeover when control succeeds", async () => {
    const { open, modes, fixture, send } = await setup("control");
    await send(0, frame);
    expect(open).toHaveBeenCalledTimes(1);
    expect(modeOf(open.mock.calls[0][0])).toMatchObject({ mode: "control", takeover: false });
    expect(modes.at(-1)).toEqual({ mode: "control", blocked: null });
    expect(fixture.componentInstance.blocked()).toBeNull();
  });

  it("falls back to read-only with the reason when another client holds the controller", async () => {
    const { open, modes, send, fixture } = await setup("control");
    await send(0, closed("terminal is controlled by another client"));
    expect(open).toHaveBeenCalledTimes(2);
    expect(modeOf(open.mock.calls[1][0])).toMatchObject({ mode: "observe", takeover: false });
    expect(modes.at(-1)).toEqual({
      mode: "observe",
      blocked: "terminal is controlled by another client",
    });
    expect(fixture.componentInstance.banner()).toBeNull();
  });

  it("never takes over by itself, only on the explicit action", async () => {
    const { open, send, fixture } = await setup("control");
    await send(0, closed("held"));
    expect(open.mock.calls.every((c) => !modeOf(c[0]).takeover)).toBe(true);
    fixture.componentInstance.reconnect(true);
    await new Promise((resolve) => setTimeout(resolve));
    expect(modeOf(open.mock.calls.at(-1)![0])).toMatchObject({ mode: "control", takeover: true });
    expect(fixture.componentInstance.blocked()).toBeNull();
  });

  it("opens read-only without trying control when input is not wanted (setting off)", async () => {
    const { open, modes } = await setup("observe");
    expect(open).toHaveBeenCalledTimes(1);
    expect(modeOf(open.mock.calls[0][0]).mode).toBe("observe");
    expect(modes.at(-1)).toEqual({ mode: "observe", blocked: null });
  });

  it("goes read-only with the banner when another client takes the controller", async () => {
    const { open, modes, send, fixture } = await setup("control");
    await send(0, frame);
    await send(0, closed("controller taken over"));
    expect(modes.at(-1)).toEqual({ mode: "observe", blocked: "controller taken over" });
    expect(fixture.componentInstance.banner()).toBe("controller taken over");
    expect(modeOf(open.mock.calls.at(-1)![0]).mode).toBe("observe");
  });

  it("does not retry control when a read-only fallback tab is resized", async () => {
    const { open, send, fixture, settings } = await setup("control");
    await send(0, closed("terminal 6 already has an attached client; retry with --takeover"));
    fake.nextCols = 100;
    settings.set({ ...DEFAULT_SETTINGS, fontSize: 10 });
    fixture.detectChanges();
    await fixture.whenStable();
    await new Promise((resolve) => setTimeout(resolve));
    expect(open).toHaveBeenCalledTimes(3);
    expect(open.mock.calls.slice(1).every((c) => modeOf(c[0]).mode === "observe")).toBe(true);
    expect(fixture.componentInstance.mode()).toBe("observe");
  });

  it("shows an error, not a held controller, when the pane is gone before the first frame", async () => {
    const { modes, send, fixture } = await setup("control");
    await send(0, closed("pane w1:p1 not found"));
    expect(modes.at(-1)).toEqual({ mode: "observe", blocked: null });
    expect(fixture.componentInstance.banner()).toBe("pane w1:p1 not found");
  });

  it("reports read-only until the first frame, then input", async () => {
    const { modes, send } = await setup("control");
    expect(modes.at(-1)).toEqual({ mode: "observe", blocked: null });
    await send(0, frame);
    expect(modes.at(-1)).toEqual({ mode: "control", blocked: null });
  });

  it("does not fall back when the stream was detached on purpose", async () => {
    const { open, send } = await setup("control");
    await send(0, closed("detached"));
    expect(open).toHaveBeenCalledTimes(1);
  });
});

describe("isControllerHeld", () => {
  it("matches herdr's held message and not other failures", () => {
    expect(
      isControllerHeld("terminal 6 already has an attached client; retry with --takeover"),
    ).toBe(true);
    expect(isControllerHeld("pane w1:p1 not found")).toBe(false);
    expect(isControllerHeld("failed to spawn herdr: No such file")).toBe(false);
  });
});
