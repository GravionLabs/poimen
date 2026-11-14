// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { Terminal } from "../core/snapshot";
import type { StreamRecord } from "../core/backend";
import { SettingsStore, DEFAULT_SETTINGS, type Settings } from "../core/settings";
import { TauriBridge } from "../core/tauri";
import { TerminalStreamService } from "../core/terminal-stream";

const fake = vi.hoisted(() => ({
  cols: 80,
  rows: 24,
  nextCols: 80,
  /** Everything written to xterm, and its reset and resize calls, in order. */
  writes: [] as unknown[],
  calls: [] as string[],
  onData: undefined as ((data: string) => void) | undefined,
  observers: [] as (() => void)[],
  addons: [] as unknown[],
  openUrl: undefined as unknown,
  term: undefined as
    { selection: string; pasted: string[]; options: Record<string, unknown> } | undefined,
  keyHandler: undefined as ((event: KeyboardEvent) => boolean) | undefined,
  wheelHandler: undefined as ((event: WheelEvent) => boolean) | undefined,
}));

vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    options: Record<string, unknown>;
    constructor(options: Record<string, unknown> = {}) {
      this.options = { theme: { foreground: "#e4e4e0" }, ...options };
      fake.term = this;
    }
    buffer = { active: { viewportY: 0 } };
    unicode = { activeVersion: "6" };
    get cols() {
      return fake.cols;
    }
    get rows() {
      return fake.rows;
    }
    loadAddon(addon: unknown) {
      fake.addons.push(addon);
    }
    open() {}
    onData(handler: (data: string) => void) {
      fake.onData = handler;
    }
    attachCustomWheelEventHandler(handler: (event: WheelEvent) => boolean) {
      fake.wheelHandler = handler;
    }
    attachCustomKeyEventHandler(handler: (event: KeyboardEvent) => boolean) {
      fake.keyHandler = handler;
    }
    focus() {}
    selection = "";
    pasted: string[] = [];
    getSelection() {
      return this.selection;
    }
    paste(text: string) {
      this.pasted.push(text);
    }
    write(data: unknown, callback?: () => void) {
      fake.writes.push(data);
      callback?.();
    }
    reset() {
      fake.calls.push("reset");
    }
    resize(cols: number, rows: number) {
      fake.cols = cols;
      fake.calls.push(`resize ${cols}x${rows}`);
    }
    dispose() {}
  },
}));
vi.mock("@xterm/addon-unicode11", () => ({ Unicode11Addon: class {} }));
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit() {
      fake.cols = fake.nextCols;
    }
  },
}));
vi.mock("@xterm/addon-web-links", () => ({
  WebLinksAddon: class {
    constructor(
      public handler: (event: MouseEvent, uri: string) => void,
      public options: {
        hover: (event: MouseEvent, text: string, range: unknown) => void;
        leave: () => void;
      },
    ) {}
  },
}));
vi.mock("@xterm/addon-search", () => ({
  SearchAddon: class {
    findNext = vi.fn(() => true);
    findPrevious = vi.fn(() => true);
    clearDecorations = vi.fn();
    listener?: (e: { resultIndex: number; resultCount: number }) => void;
    onDidChangeResults(l: (e: { resultIndex: number; resultCount: number }) => void) {
      this.listener = l;
      return { dispose() {} };
    }
  },
}));
vi.mock("@xterm/addon-webgl", () => ({
  WebglAddon: class {
    onContextLoss = vi.fn();
  },
}));

import { TerminalSearch } from "./search";
import { REATTACHING, REATTACH_DELAYS_MS, TerminalView } from "./terminal-view";

const pane: Terminal = {
  terminal_id: "w1:p1",
  tab_id: "w1:t1",
  space_id: "w1",
  agent_status: "idle",
};
const attached: StreamRecord = { type: "controller", attached: true };

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
    errorOnUnknownElements: false,
  });
  globalThis.ResizeObserver = class {
    constructor(callback: () => void) {
      fake.observers.push(callback);
    }
    observe() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
});

afterEach(() => TestBed.resetTestingModule());

async function setup(wanted: "control" | "observe", target: Terminal = pane, emulated = false) {
  fake.cols = 80;
  fake.nextCols = 80;
  fake.observers = [];
  fake.addons = [];
  fake.writes = [];
  fake.calls = [];
  const openUrl = vi.fn(async () => undefined);
  const settings = signal<Settings>(DEFAULT_SETTINGS);
  const session = {
    open: true,
    emulated,
    resize: vi.fn(async () => undefined),
    close: vi.fn(),
    input: vi.fn(),
    scroll: vi.fn(),
    mouse: vi.fn(),
  };
  const records: ((record: StreamRecord) => void)[] = [];
  const open = vi.fn(async (_opts: unknown, onRecord: (record: StreamRecord) => void) => {
    records.push(onRecord);
    return { ...session };
  });
  TestBed.configureTestingModule({
    providers: [
      { provide: SettingsStore, useValue: { settings } },
      { provide: TerminalStreamService, useValue: { open } },
      { provide: TauriBridge, useValue: { openUrl } },
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
    pane: signal(target),
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
  return { fixture, settings, session, open, sized, modes, send, body, openUrl };
}

const GHOSTTY = {
  background: "#000001",
  foreground: "#fffffe",
  cursor: null,
  selectionBackground: null,
  selectionForeground: null,
  palette: [],
};

describe("TerminalView theme", () => {
  const css = readFileSync("src/styles.css", "utf8");
  const block = (selector: string) =>
    new RegExp(`${selector.replace(".", "\\.")}\\s*\\{([^}]*)\\}`).exec(css)![1];
  const token = (selector: string, name: string) =>
    new RegExp(`${name}:\\s*(#[0-9a-f]{6})`, "i").exec(block(selector))?.[1];

  it("defines the terminal tokens (also the tab-bar background) for light and dark", () => {
    expect(token(":root", "--term-bg")).toBe("#fbfbf9");
    expect(token(":root", "--term-fg")).toBe("#26272a");
    expect(token(":root.app-dark", "--term-bg")).toBe("#111214");
    expect(token(":root.app-dark", "--term-fg")).toBe("#e4e4e0");
  });

  it("defines 16 ANSI colors, cursor and selection tokens for light and dark", () => {
    for (const selector of [":root", ":root.app-dark"]) {
      for (let i = 0; i < 16; i++) expect(token(selector, `--term-ansi-${i}`)).toBeDefined();
      for (const name of ["--term-cursor", "--term-selection-bg", "--term-selection-fg"]) {
        expect(token(selector, name)).toBeDefined();
      }
    }
    expect(token(":root", "--term-ansi-1")).not.toBe(token(":root.app-dark", "--term-ansi-1"));
  });

  it("passes all 16 ANSI colors, cursor and selection to xterm, and re-reads them on app-dark", async () => {
    const root = document.documentElement;
    const names = [
      "black",
      "red",
      "green",
      "yellow",
      "blue",
      "magenta",
      "cyan",
      "white",
      "brightBlack",
      "brightRed",
      "brightGreen",
      "brightYellow",
      "brightBlue",
      "brightMagenta",
      "brightCyan",
      "brightWhite",
    ];
    const set = (hex: (i: number) => string, extra: string) => {
      for (let i = 0; i < 16; i++) root.style.setProperty(`--term-ansi-${i}`, hex(i));
      root.style.setProperty("--term-cursor", extra);
      root.style.setProperty("--term-selection-bg", extra);
      root.style.setProperty("--term-selection-fg", extra);
    };
    set((i) => `#0000${i.toString(16).padStart(2, "0")}`, "#111111");
    const { fixture } = await setup("observe");
    const term = (
      fixture.componentInstance as unknown as {
        term: { options: { theme: Record<string, string> } };
      }
    ).term;
    names.forEach((n, i) =>
      expect(term.options.theme[n]).toBe(`#0000${i.toString(16).padStart(2, "0")}`),
    );
    expect(term.options.theme).toMatchObject({
      cursor: "#111111",
      selectionBackground: "#111111",
      selectionForeground: "#111111",
    });
    set((i) => `#ff00${i.toString(16).padStart(2, "0")}`, "#222222");
    root.classList.add("app-dark");
    await new Promise((resolve) => setTimeout(resolve));
    names.forEach((n, i) =>
      expect(term.options.theme[n]).toBe(`#ff00${i.toString(16).padStart(2, "0")}`),
    );
    expect(term.options.theme["selectionBackground"]).toBe("#222222");
    root.classList.remove("app-dark");
    root.removeAttribute("style");
  });

  it("keeps Ghostty background and foreground while the palette still comes from the tokens", async () => {
    const root = document.documentElement;
    root.style.setProperty("--term-ansi-1", "#aa0000");
    const { fixture, settings } = await setup("observe");
    settings.set({ ...DEFAULT_SETTINGS, colors: GHOSTTY });
    fixture.detectChanges();
    const term = (
      fixture.componentInstance as unknown as {
        term: { options: { theme: Record<string, string> } };
      }
    ).term;
    expect(term.options.theme).toMatchObject({
      background: "#000001",
      foreground: "#fffffe",
      red: "#aa0000",
    });
    root.removeAttribute("style");
  });

  it("re-reads the token colors when the theme class changes", async () => {
    const root = document.documentElement;
    root.style.setProperty("--term-bg", "#fbfbf9");
    root.style.setProperty("--term-fg", "#26272a");
    const { fixture } = await setup("observe");
    const term = (
      fixture.componentInstance as unknown as {
        term: { options: { theme: { background: string } } };
      }
    ).term;
    expect(term.options.theme.background).toBe("#fbfbf9");
    root.style.setProperty("--term-bg", "#111214");
    root.style.setProperty("--term-fg", "#e4e4e0");
    root.classList.add("app-dark");
    await new Promise((resolve) => setTimeout(resolve));
    expect(term.options.theme).toMatchObject({ background: "#111214", foreground: "#e4e4e0" });
    root.classList.remove("app-dark");
    root.removeAttribute("style");
  });

  it("keeps colors from the Ghostty config over the tokens on a theme change", async () => {
    const root = document.documentElement;
    root.style.setProperty("--term-bg", "#fbfbf9");
    const { fixture, settings } = await setup("observe");
    settings.set({ ...DEFAULT_SETTINGS, colors: GHOSTTY });
    fixture.detectChanges();
    const term = (
      fixture.componentInstance as unknown as {
        term: { options: { theme: { background: string } } };
      }
    ).term;
    root.style.setProperty("--term-bg", "#111214");
    root.classList.add("app-dark");
    await new Promise((resolve) => setTimeout(resolve));
    expect(term.options.theme.background).toBe("#000001");
    root.classList.remove("app-dark");
    root.removeAttribute("style");
  });
});

describe("TerminalView Ghostty palette", () => {
  it("lays palette, cursor and selection over the scheme tokens and keeps the rest", async () => {
    const root = document.documentElement;
    root.style.setProperty("--term-ansi-1", "#aa0000");
    root.style.setProperty("--term-ansi-2", "#00aa00");
    root.style.setProperty("--term-cursor", "#123456");
    root.style.setProperty("--term-selection-bg", "#222222");
    root.style.setProperty("--term-selection-fg", "#333333");
    const { fixture, settings } = await setup("observe");
    settings.set({
      ...DEFAULT_SETTINGS,
      colors: {
        ...GHOSTTY,
        cursor: "#abcdef",
        selectionBackground: "#010203",
        selectionForeground: null,
        palette: ["#0a0a0a", null, ...Array<null>(14).fill(null)],
      },
    });
    fixture.detectChanges();
    const term = (
      fixture.componentInstance as unknown as {
        term: { options: { theme: Record<string, string> } };
      }
    ).term;
    expect(term.options.theme).toMatchObject({
      black: "#0a0a0a",
      red: "#aa0000",
      green: "#00aa00",
      cursor: "#abcdef",
      selectionBackground: "#010203",
      selectionForeground: "#333333",
    });
    root.removeAttribute("style");
  });
});

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

describe("TerminalView default font stack", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("starts with the Windows monospace fonts on Windows", async () => {
    vi.stubGlobal("navigator", { ...navigator, platform: "Win32" });
    await setup("control");
    const family = String(fake.term?.options["fontFamily"]);
    expect(family.startsWith('"Cascadia Mono", "Cascadia Code", Consolas, ')).toBe(true);
    expect(family.endsWith("Menlo, monospace")).toBe(true);
  });

  it("keeps Menlo, monospace on macOS", async () => {
    vi.stubGlobal("navigator", { ...navigator, platform: "MacIntel" });
    await setup("control");
    expect(fake.term?.options["fontFamily"]).toBe("Menlo, monospace");
  });

  it("a configured family wins on Windows", async () => {
    vi.stubGlobal("navigator", { ...navigator, platform: "Win32" });
    const { fixture, settings } = await setup("control");
    settings.set({ ...DEFAULT_SETTINGS, fontFamily: "Hack" });
    fixture.detectChanges();
    expect(fake.term?.options["fontFamily"]).toBe("Hack");
  });
});

describe("TerminalView input mode decision", () => {
  const closed = (reason: string, held = false): StreamRecord => ({
    type: "closed",
    reason,
    ...(held ? { code: "controller_held" as const } : {}),
  });
  const frame: StreamRecord = {
    type: "reset",
    cols: 80,
    rows: 24,
    bytes: "",
  };
  const modeOf = (call: unknown) => call as { mode: string; takeover: boolean };

  it("opens in input mode without a takeover when control succeeds", async () => {
    const { open, modes, fixture, send } = await setup("control");
    await send(0, frame);
    await send(0, attached);
    expect(open).toHaveBeenCalledTimes(1);
    expect(modeOf(open.mock.calls[0][0])).toMatchObject({ mode: "control", takeover: false });
    expect(modes.at(-1)).toEqual({ mode: "control", blocked: null });
    expect(fixture.componentInstance.blocked()).toBeNull();
  });

  it("falls back to read-only with the reason when another client holds the controller", async () => {
    const { open, modes, send, fixture } = await setup("control");
    await send(0, closed("terminal is controlled by another client", true));
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
    await send(0, closed("held", true));
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
    await send(0, attached);
    await send(0, closed("controller taken over", true));
    expect(modes.at(-1)).toEqual({ mode: "observe", blocked: "controller taken over" });
    expect(fixture.componentInstance.banner()).toBe("controller taken over");
    expect(modeOf(open.mock.calls.at(-1)![0]).mode).toBe("observe");
  });

  it("does not retry control when a read-only fallback tab is resized", async () => {
    const { open, send, fixture, settings } = await setup("control");
    await send(0, closed("terminal 6 already has an attached client; retry with --takeover", true));
    fake.nextCols = 100;
    settings.set({ ...DEFAULT_SETTINGS, fontSize: 10 });
    fixture.detectChanges();
    await fixture.whenStable();
    await new Promise((resolve) => setTimeout(resolve));
    expect(open).toHaveBeenCalledTimes(3);
    expect(open.mock.calls.slice(1).every((c) => modeOf(c[0]).mode === "observe")).toBe(true);
    expect(fixture.componentInstance.mode()).toBe("observe");
  });

  it("keeps the blocked state, banner and take over when a held tab is resized", async () => {
    const held = "terminal 6 already has an attached client; retry with --takeover";
    const { send, fixture, settings, modes } = await setup("control");
    await send(0, closed(held, true));
    fake.nextCols = 100;
    settings.set({ ...DEFAULT_SETTINGS, fontSize: 10 });
    fixture.detectChanges();
    await fixture.whenStable();
    await new Promise((resolve) => setTimeout(resolve));
    expect(fixture.componentInstance.blocked()).toBe(held);
    expect(modes.at(-1)).toEqual({ mode: "observe", blocked: held });
    expect(fixture.nativeElement.textContent).toContain("Take over");
  });

  it("presents a lost stream with a non-takeover reason as an error, not as held", async () => {
    const { modes, send, fixture } = await setup("control");
    await send(0, frame);
    await send(0, attached);
    await send(0, closed("pane w1:p1 not found"));
    expect(modes.at(-1)).toEqual({ mode: "observe", blocked: null });
    expect(fixture.componentInstance.banner()).toBe("pane w1:p1 not found");
  });

  it("closes a stale open that a newer restart superseded", async () => {
    const { fixture, open } = await setup("control");
    const sessions: { close: ReturnType<typeof vi.fn> }[] = [];
    let release!: () => void;
    open.mockImplementationOnce(async () => {
      await new Promise<void>((r) => (release = r));
      const s = {
        open: true,
        emulated: false,
        close: vi.fn(),
        resize: vi.fn(async () => undefined),
        input: vi.fn(),
        scroll: vi.fn(),
        mouse: vi.fn(),
      };
      sessions.push(s);
      return s;
    });
    fixture.componentInstance.reconnect(false); // slow open
    await new Promise((resolve) => setTimeout(resolve));
    fixture.componentInstance.reconnect(false); // newer one wins
    await new Promise((resolve) => setTimeout(resolve));
    release();
    await new Promise((resolve) => setTimeout(resolve));
    expect(sessions[0].close).toHaveBeenCalled();
  });

  it("shows an error, not a held controller, when the pane is gone before the first frame", async () => {
    const { modes, send, fixture } = await setup("control");
    await send(0, closed("pane w1:p1 not found"));
    expect(modes.at(-1)).toEqual({ mode: "observe", blocked: null });
    expect(fixture.componentInstance.banner()).toBe("pane w1:p1 not found");
  });

  it("reports read-only until the controller record, then input", async () => {
    const { modes, send } = await setup("control");
    expect(modes.at(-1)).toEqual({ mode: "observe", blocked: null });
    await send(0, frame);
    expect(modes.at(-1)).toEqual({ mode: "observe", blocked: null });
    await send(0, attached);
    expect(modes.at(-1)).toEqual({ mode: "control", blocked: null });
  });

  it("does not fall back when the stream was detached on purpose", async () => {
    const { open, send } = await setup("control");
    await send(0, closed("detached"));
    expect(open).toHaveBeenCalledTimes(1);
  });
});

describe("a held controller", () => {
  it("is the structured code of the closed record, not matched from its text", async () => {
    const { modes, send } = await setup("control");
    await send(0, {
      type: "closed",
      reason: "terminal 6 already has an attached client; retry with --takeover",
    });
    expect(modes.at(-1)).toEqual({ mode: "observe", blocked: null });
  });
});

describe("TerminalView fit on layout change (#80)", () => {
  it("refits after the body resized (sidebar toggle) once the layout settled", async () => {
    const { sized } = await setup("control");
    vi.useFakeTimers();
    try {
      fake.nextCols = 90;
      for (const notify of fake.observers) {
        notify();
        notify(); // a burst while the layout moves is debounced into one fit
      }
      expect(sized).toHaveLength(1);
      vi.advanceTimersByTime(200);
      expect(sized).toEqual([
        { cols: 80, rows: 24 },
        { cols: 90, rows: 24 },
      ]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps the padding off the element xterm fits into", () => {
    // FitAddon reads that element's computed width, which includes padding under border-box, so
    // padding there made the terminal wider than its content (clipped glyph).
    const css = readFileSync("src/app/terminal-view/terminal-view.css", "utf8");
    const rule = /\.view-body\s*\{([^}]*)\}/.exec(css)![1];
    expect(rule).not.toMatch(/padding[^:]*:\s*[1-9]/);
    const html = readFileSync("src/app/terminal-view/terminal-view.html", "utf8");
    expect(html).toContain('<div class="view-pad"><div #body class="view-body">');
  });
});

describe("TerminalView mouse input", () => {
  const frame: StreamRecord = {
    type: "reset",
    cols: 80,
    rows: 24,
    bytes: "",
  };
  /** 800x480 px body: 80x24 cells of 10x20 px. */
  const rect = () => ({ left: 0, top: 0, width: 800, height: 480 }) as DOMRect;
  const mouse = (type: string, x: number, y: number, init: MouseEventInit = {}) =>
    new MouseEvent(type, { clientX: x, clientY: y, bubbles: true, cancelable: true, ...init });

  async function controlling() {
    const ctx = await setup("control");
    ctx.body.getBoundingClientRect = rect;
    await ctx.send(0, frame);
    await ctx.send(0, attached); // the controller record confirms control
    return ctx;
  }

  it("forwards a click as down and up at the cell under the pointer", async () => {
    const { session, body } = await controlling();
    body.dispatchEvent(mouse("mousedown", 125, 45));
    window.dispatchEvent(mouse("mouseup", 125, 45));
    expect(session.mouse.mock.calls.map(([e]) => e)).toEqual([
      { action: "down", button: "left", column: 12, row: 2, modifiers: 0 },
      { action: "up", button: "left", column: 12, row: 2, modifiers: 0 },
    ]);
  });

  it("clamps the mouse cell to the grid of the last frame", async () => {
    const { session, body, send } = await controlling();
    await send(0, { type: "resized", cols: 60, rows: 10 });
    body.dispatchEvent(mouse("mousedown", 795, 475)); // xterm cell 79,23
    window.dispatchEvent(mouse("mouseup", 125, 45));
    expect(session.mouse.mock.calls.map(([e]) => [e.column, e.row])).toEqual([
      [59, 9],
      [12, 2],
    ]);
  });

  it("forwards a drag to a new cell once, with the pressed button and modifiers", async () => {
    const { session, body } = await controlling();
    body.dispatchEvent(mouse("mousedown", 5, 5, { button: 2, ctrlKey: true }));
    window.dispatchEvent(mouse("mousemove", 35, 5, { button: 2, ctrlKey: true }));
    window.dispatchEvent(mouse("mousemove", 36, 6, { button: 2, ctrlKey: true })); // same cell
    window.dispatchEvent(mouse("mouseup", 35, 5, { button: 2, ctrlKey: true }));
    expect(session.mouse.mock.calls.map(([e]) => e.action)).toEqual(["down", "drag", "up"]);
    expect(session.mouse.mock.calls[1][0]).toEqual({
      action: "drag",
      button: "right",
      column: 3,
      row: 0,
      modifiers: 2,
    });
  });

  it("leaves selection and the context menu to the browser while Shift is held", async () => {
    const { session, body } = await controlling();
    const down = mouse("mousedown", 5, 5, { shiftKey: true });
    body.dispatchEvent(down);
    window.dispatchEvent(mouse("mousemove", 50, 50, { shiftKey: true }));
    window.dispatchEvent(mouse("mouseup", 50, 50, { shiftKey: true }));
    const menu = mouse("contextmenu", 5, 5, { shiftKey: true });
    body.dispatchEvent(menu);
    expect(session.mouse).not.toHaveBeenCalled();
    expect(down.defaultPrevented).toBe(false);
    expect(menu.defaultPrevented).toBe(false);
    const plain = mouse("contextmenu", 5, 5);
    body.dispatchEvent(plain);
    expect(plain.defaultPrevented).toBe(true);
  });

  it("releases a held button when the tab is destroyed mid-drag and drops the listeners", async () => {
    const { session, body, fixture } = await controlling();
    body.dispatchEvent(mouse("mousedown", 125, 45));
    fixture.destroy();
    expect(session.mouse.mock.calls.map(([e]) => e.action)).toEqual(["down", "up"]);
    expect(session.mouse.mock.calls[1][0]).toMatchObject({ column: 12, row: 2 });
    body.dispatchEvent(mouse("mousedown", 5, 5));
    window.dispatchEvent(mouse("mouseup", 5, 5));
    expect(session.mouse).toHaveBeenCalledTimes(2);
  });

  it("sends nothing from a read-only tab and leaves selection alone", async () => {
    const { session, body, send } = await setup("observe");
    body.getBoundingClientRect = rect;
    await send(0, frame);
    const down = mouse("mousedown", 5, 5);
    body.dispatchEvent(down);
    body.dispatchEvent(mouse("mousemove", 50, 50));
    window.dispatchEvent(mouse("mouseup", 50, 50));
    expect(session.mouse).not.toHaveBeenCalled();
    expect(down.defaultPrevented).toBe(false);
  });

  it("forwards the wheel to a rendered stream", async () => {
    const { send, session } = await setup("control");
    await send(0, attached);
    fake.wheelHandler?.({ deltaY: -60 } as WheelEvent);
    expect(session.scroll).toHaveBeenCalledWith(true, 2);
  });

  it("does not forward the wheel to an emulated stream", async () => {
    const { send, session } = await setup("control", pane, true);
    await send(0, attached);
    expect(fake.wheelHandler?.({ deltaY: -60 } as WheelEvent)).toBe(true);
    expect(session.scroll).not.toHaveBeenCalled();
  });

  it("leaves the mouse to xterm.js in an emulated stream", async () => {
    const { session, body, send } = await setup("control", pane, true);
    body.getBoundingClientRect = rect;
    await send(0, attached);
    const down = mouse("mousedown", 5, 5);
    body.dispatchEvent(down);
    window.dispatchEvent(mouse("mouseup", 5, 5));
    expect(session.mouse).not.toHaveBeenCalled();
    expect(down.defaultPrevented).toBe(false);
  });

  it("sends nothing while another client holds the controller", async () => {
    const { session, body, send } = await setup("control");
    body.getBoundingClientRect = rect;
    await send(0, {
      type: "closed",
      reason: "terminal already has an attached client",
      code: "controller_held",
    });
    body.dispatchEvent(mouse("mousedown", 5, 5));
    expect(session.mouse).not.toHaveBeenCalled();
  });
});

describe("TerminalView links", () => {
  it("opens an http(s) link through the opener and nothing else", async () => {
    const { openUrl } = await setup("control");
    const addon = fake.addons.find((a) => a && typeof a === "object" && "handler" in a) as {
      handler: (event: MouseEvent, uri: string) => void;
    };
    addon.handler(new MouseEvent("click"), "https://github.com/GravionLabs/poimen/pull/69");
    addon.handler(new MouseEvent("click"), "javascript:alert(1)");
    expect(openUrl.mock.calls).toEqual([["https://github.com/GravionLabs/poimen/pull/69"]]);
  });

  it("opens OSC 8 hyperlinks through the opener, http(s) only", async () => {
    const { openUrl } = await setup("control");
    const options = (
      fake.term as unknown as {
        options: { linkHandler: { activate: (e: MouseEvent, uri: string) => void } };
      }
    ).options;
    options.linkHandler.activate(new MouseEvent("click"), "https://example.com/doc");
    options.linkHandler.activate(new MouseEvent("click"), "file:///etc/passwd");
    expect(openUrl.mock.calls).toEqual([["https://example.com/doc"]]);
  });

  /** Makes xterm report a hovered link, as it does on a real mouse move. */
  function hoverLink() {
    const addon = fake.addons.find((a) => a && typeof a === "object" && "options" in a) as {
      options: { hover: (e: MouseEvent, t: string, r: unknown) => void };
    };
    addon.options.hover(new MouseEvent("mousemove"), "https://x.y", {
      start: { x: 1, y: 1 },
      end: { x: 2, y: 1 },
    });
  }

  it("lets a Ctrl mousedown over a link reach xterm in input mode off macOS, but captures a plain one", async () => {
    const { session, body } = await setup("control");
    await new Promise((resolve) => setTimeout(resolve));
    hoverLink();
    const reached = vi.fn();
    body.addEventListener("mousedown", reached);
    body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, ctrlKey: true }));
    expect(reached).toHaveBeenCalledTimes(1);
    body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    expect(reached).toHaveBeenCalledTimes(1);
    expect(session.mouse).toHaveBeenCalledTimes(1); // only the plain press
  });

  it("keeps Ctrl for the pane application outside a link", async () => {
    const { session, body } = await setup("control");
    await new Promise((resolve) => setTimeout(resolve));
    body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, ctrlKey: true }));
    expect(session.mouse).toHaveBeenCalledTimes(1);
  });

  it("uses Cmd instead of Ctrl on macOS", async () => {
    vi.stubGlobal("navigator", { platform: "MacIntel" });
    try {
      const { session, body } = await setup("control");
      await new Promise((resolve) => setTimeout(resolve));
      hoverLink();
      const reached = vi.fn();
      body.addEventListener("mousedown", reached);
      body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, metaKey: true }));
      expect(reached).toHaveBeenCalledTimes(1);
      body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, ctrlKey: true }));
      expect(reached).toHaveBeenCalledTimes(1);
      expect(session.mouse).toHaveBeenCalledTimes(1); // the Ctrl press is the pane's
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("forwards Cmd to the pane on macOS outside a link and after the pointer left a link", async () => {
    vi.stubGlobal("navigator", { platform: "MacIntel" });
    try {
      const { session, body } = await setup("control");
      await new Promise((resolve) => setTimeout(resolve));
      body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, metaKey: true }));
      expect(session.mouse).toHaveBeenCalledTimes(1);
      hoverLink();
      const addon = fake.addons.find((a) => a && typeof a === "object" && "options" in a) as {
        options: { leave: () => void };
      };
      addon.options.leave();
      body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, metaKey: true }));
      expect(session.mouse).toHaveBeenCalledTimes(2);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("forwards Shift-less Ctrl drags outside a link and still bypasses on Shift over a link", async () => {
    const { session, body } = await setup("control");
    await new Promise((resolve) => setTimeout(resolve));
    body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, ctrlKey: true }));
    expect(session.mouse).toHaveBeenCalledTimes(1);
    hoverLink();
    body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, shiftKey: true }));
    expect(session.mouse).toHaveBeenCalledTimes(1);
  });
});

describe("TerminalView app shortcuts (#23, #26)", () => {
  const key = (k: string, init: KeyboardEventInit = {}) =>
    new KeyboardEvent("keydown", { key: k, ...init });

  it("keeps Cmd shortcuts from xterm so they are not sent to the pane", async () => {
    const { settings } = await setup("control");
    settings.set({
      ...DEFAULT_SETTINGS,
      shortcuts: { ...DEFAULT_SETTINGS.shortcuts, splitRight: "Cmd+\\" },
    });
    expect(fake.keyHandler?.(key("\\", { metaKey: true }))).toBe(false);
  });

  it("sends Ctrl+\\ to the pane: a focused terminal only yields shortcuts that use Cmd", async () => {
    await setup("control");
    expect(fake.keyHandler?.(key("\\", { ctrlKey: true }))).toBe(true);
    expect(fake.keyHandler?.(key("b", { ctrlKey: true }))).toBe(true);
  });

  it("lets other keys through to xterm", async () => {
    await setup("control");
    expect(fake.keyHandler?.(key("\\"))).toBe(true);
    expect(fake.keyHandler?.(key("c", { ctrlKey: true }))).toBe(true);
  });
});

describe("TerminalView search (#714)", () => {
  type FakeSearch = {
    findNext: ReturnType<typeof vi.fn>;
    findPrevious: ReturnType<typeof vi.fn>;
    clearDecorations: ReturnType<typeof vi.fn>;
    listener: (e: { resultIndex: number; resultCount: number }) => void;
  };
  const addon = () =>
    fake.addons.find((a) => typeof (a as FakeSearch).findNext === "function") as FakeSearch;
  const key = (k: string, init: KeyboardEventInit = {}) =>
    new KeyboardEvent("keydown", { key: k, cancelable: true, ...init });
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const view = (fixture: { componentInstance: unknown }) => fixture.componentInstance as any;

  it("loads the search addon with the others", async () => {
    await setup("control");
    expect(addon()).toBeDefined();
  });

  it("takes Ctrl+Shift+F off macOS and keeps it from the PTY", async () => {
    vi.stubGlobal("navigator", { platform: "Linux x86_64" });
    try {
      const { fixture } = await setup("control");
      expect(fake.keyHandler?.(key("F", { ctrlKey: true, shiftKey: true }))).toBe(false);
      expect(view(fixture).searchOpen()).toBe(true);
      expect(fake.keyHandler?.(key("f", { ctrlKey: true }))).toBe(true);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("opens in an observer too and says Screen only without xterm scrollback", async () => {
    const { fixture } = await setup("observe");
    view(fixture).openSearch();
    fixture.detectChanges();
    expect(view(fixture).searchOpen()).toBe(true);
    expect(fixture.nativeElement.textContent).toContain("Screen only");
  });

  it("does not say Screen only on an emulated stream", async () => {
    const { fixture } = await setup("control", pane, true);
    view(fixture).openSearch();
    fixture.detectChanges();
    expect(fixture.nativeElement.textContent).not.toContain("Screen only");
  });

  it("searches while typing, steps with Enter and Shift+Enter and shows the position", async () => {
    const { fixture } = await setup("control", pane, true);
    const v = view(fixture);
    v.openSearch();
    v.setQuery("err");
    expect(addon().findNext).toHaveBeenCalledWith(
      "err",
      expect.objectContaining({ caseSensitive: false, regex: false }),
    );
    addon().listener({ resultIndex: 1, resultCount: 4 });
    expect(v.searchCounter()).toBe("2 of 4");
    v.searchKey(key("Enter"));
    expect(addon().findNext).toHaveBeenCalledTimes(2);
    v.searchKey(key("Enter", { shiftKey: true }));
    expect(addon().findPrevious).toHaveBeenCalledTimes(1);
  });

  it("reports no results and passes the case and regex toggles", async () => {
    const { fixture } = await setup("control", pane, true);
    const v = view(fixture);
    v.openSearch();
    v.setQuery("x+");
    v.toggleCase();
    v.toggleRegex();
    expect(addon().findNext).toHaveBeenLastCalledWith(
      "x+",
      expect.objectContaining({ caseSensitive: true, regex: true }),
    );
    addon().listener({ resultIndex: -1, resultCount: 0 });
    expect(v.searchCounter()).toBe("No results");
  });

  it("clears highlights without a query", async () => {
    const { fixture } = await setup("control", pane, true);
    const v = view(fixture);
    v.openSearch();
    v.setQuery("");
    expect(addon().findNext).not.toHaveBeenCalled();
    expect(addon().clearDecorations).toHaveBeenCalled();
    expect(v.searchCounter()).toBe("");
  });

  it("colors the highlights from the theme tokens", async () => {
    document.documentElement.style.setProperty("--term-ansi-3", "#aabbcc");
    try {
      const { fixture } = await setup("control", pane, true);
      const v = view(fixture);
      v.openSearch();
      v.setQuery("a");
      const options = addon().findNext.mock.calls[0][1];
      expect(options.decorations.activeMatchBackground).toBe("#aabbcc");
    } finally {
      document.documentElement.style.removeProperty("--term-ansi-3");
    }
  });

  it("closes with Escape, clears the highlights and does not let the key out", async () => {
    const { fixture } = await setup("control", pane, true);
    const v = view(fixture);
    v.openSearch();
    v.setQuery("a");
    const event = key("Escape");
    const stop = vi.spyOn(event, "stopPropagation");
    v.searchKey(event);
    expect(v.searchOpen()).toBe(false);
    expect(addon().clearDecorations).toHaveBeenCalled();
    expect(stop).toHaveBeenCalled();
  });

  it("returns the focus to the terminal on close", async () => {
    const { fixture } = await setup("control", pane, true);
    const focus = vi.spyOn(fake.term as unknown as { focus(): void }, "focus");
    view(fixture).openSearch();
    view(fixture).closeSearch();
    expect(focus).toHaveBeenCalled();
  });

  it("is found by the hub only while the view lives", async () => {
    const { fixture } = await setup("control");
    const hub = TestBed.inject(TerminalSearch);
    expect(hub.open()).toBe(false);
    view(fixture).onSearchFocus();
    expect(hub.open()).toBe(true);
    expect(view(fixture).searchOpen()).toBe(true);
    fixture.destroy();
    expect(hub.open()).toBe(false);
  });
});

describe("TerminalView copy and paste (#35)", () => {
  const frame: StreamRecord = {
    type: "reset",
    cols: 80,
    rows: 24,
    bytes: "",
  };
  const clipboardEvent = (type: string, text?: string) => {
    const data = new Map<string, string>(text === undefined ? [] : [["text/plain", text]]);
    const event = new Event(type, { bubbles: true, cancelable: true }) as ClipboardEvent;
    Object.defineProperty(event, "clipboardData", {
      value: {
        getData: (k: string) => data.get(k) ?? "",
        setData: (k: string, v: string) => data.set(k, v),
      },
    });
    return { event, data };
  };

  it("copies the selection on copy in input mode", async () => {
    const { body, send } = await setup("control");
    await send(0, frame);
    await send(0, attached);
    fake.term!.selection = "hello";
    const { event, data } = clipboardEvent("copy");
    body.dispatchEvent(event);
    expect(data.get("text/plain")).toBe("hello");
    expect(event.defaultPrevented).toBe(true);
  });

  it("copies the selection in a read-only tab too", async () => {
    const { body } = await setup("observe");
    fake.term!.selection = "read only text";
    const { event, data } = clipboardEvent("copy");
    body.dispatchEvent(event);
    expect(data.get("text/plain")).toBe("read only text");
  });

  it("leaves copy alone without a selection", async () => {
    const { body } = await setup("control");
    fake.term!.selection = "";
    const { event, data } = clipboardEvent("copy");
    body.dispatchEvent(event);
    expect(data.has("text/plain")).toBe(false);
    expect(event.defaultPrevented).toBe(false);
  });

  it("pastes the clipboard text once through xterm in input mode", async () => {
    const { body, send } = await setup("control");
    await send(0, frame);
    await send(0, attached);
    const { event } = clipboardEvent("paste", "line1\nline2");
    body.dispatchEvent(event);
    expect(fake.term!.pasted).toEqual(["line1\nline2"]);
    expect(event.defaultPrevented).toBe(true);
  });

  it("pastes nothing in a read-only tab", async () => {
    const { body } = await setup("observe");
    body.dispatchEvent(clipboardEvent("paste", "text").event);
    expect(fake.term!.pasted).toEqual([]);
  });

  it("pastes nothing from an empty clipboard", async () => {
    const { body, send } = await setup("control");
    await send(0, frame);
    await send(0, attached);
    body.dispatchEvent(clipboardEvent("paste", "").event);
    expect(fake.term!.pasted).toEqual([]);
  });

  it("stops listening when the view is destroyed", async () => {
    const { body, fixture, send } = await setup("control");
    await send(0, frame);
    await send(0, attached);
    fixture.destroy();
    body.dispatchEvent(clipboardEvent("paste", "late").event);
    expect(fake.term!.pasted).toEqual([]);
  });

  it("takes Ctrl+Shift+C and Ctrl+Shift+V off macOS and leaves Ctrl+C and Ctrl+V to the pane", async () => {
    const writeText = vi.fn(async () => undefined);
    vi.stubGlobal("navigator", { platform: "Linux x86_64", clipboard: { writeText } });
    try {
      await setup("control");
      fake.term!.selection = "sel";
      const key = (k: string, init: KeyboardEventInit) =>
        new KeyboardEvent("keydown", { key: k, ...init });
      expect(fake.keyHandler?.(key("C", { ctrlKey: true, shiftKey: true }))).toBe(false);
      expect(writeText).toHaveBeenCalledWith("sel");
      expect(fake.keyHandler?.(key("V", { ctrlKey: true, shiftKey: true }))).toBe(false);
      expect(fake.keyHandler?.(key("c", { ctrlKey: true }))).toBe(true);
      expect(fake.keyHandler?.(key("v", { ctrlKey: true }))).toBe(true);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("does not take Ctrl+Shift+C on macOS (Cmd+C is the menu's)", async () => {
    vi.stubGlobal("navigator", { platform: "MacIntel" });
    try {
      await setup("control");
      const event = new KeyboardEvent("keydown", { key: "C", ctrlKey: true, shiftKey: true });
      expect(fake.keyHandler?.(event)).toBe(true);
    } finally {
      vi.unstubAllGlobals();
    }
  });
});

describe("TerminalView prompt bar (#293)", () => {
  const bar = (fixture: { nativeElement: HTMLElement }) =>
    fixture.nativeElement.querySelector("app-prompt-bar") as HTMLElement | null;
  const agentPane: Terminal = { ...pane, agent: "claude" };

  it("is not rendered in a terminal tab without an agent", async () => {
    const { fixture } = await setup("observe");
    expect(bar(fixture)).toBeNull();
  });

  it("is shown in read-only mode", async () => {
    const { fixture } = await setup("observe", agentPane);
    expect(bar(fixture)!.hidden).toBe(false);
  });

  it("is hidden in control mode but stays in the DOM, so its text survives", async () => {
    const { fixture, send } = await setup("control", agentPane);
    await send(0, attached);
    fixture.detectChanges();
    expect(bar(fixture)!.hidden).toBe(true);
  });

  it("toggles live when the mode changes", async () => {
    const { fixture } = await setup("observe", agentPane);
    fixture.componentInstance.mode.set("control");
    fixture.detectChanges();
    expect(bar(fixture)!.hidden).toBe(true);
    fixture.componentInstance.mode.set("observe");
    fixture.detectChanges();
    expect(bar(fixture)!.hidden).toBe(false);
  });

  it("collapses the hidden bar although the component sets display", () => {
    const css = readFileSync("src/app/terminal-view/terminal-view.css", "utf8");
    expect(css).toMatch(/app-prompt-bar\[hidden\]\s*\{[^}]*display:\s*none/);
  });
});

describe("TerminalView on an emulated stream (poimen backend)", () => {
  const reset = (cols = 100, rows = 30): StreamRecord => ({ type: "reset", cols, rows, bytes: "" });
  const bytes = (...values: number[]): StreamRecord => ({
    type: "bytes",
    data: new Uint8Array(values),
  });
  const scrollbackOf = () => (fake.term as unknown as { options: { scrollback: number } }).options;

  it("resets xterm before a snapshot, so a reattach does not stack scrollback", async () => {
    const { send } = await setup("observe", pane, true);
    await send(0, reset());
    await send(0, bytes(104, 105));
    expect(fake.calls).toEqual(["reset", "resize 100x30"]);
    expect(fake.writes.at(-1)).toEqual(new Uint8Array([104, 105]));
  });

  it("writes raw bytes to xterm without decoding", async () => {
    const { send } = await setup("control", pane, true);
    await send(0, bytes(27, 91, 72));
    expect(fake.writes).toContainEqual(new Uint8Array([27, 91, 72]));
  });

  it("leaves a rendered stream alone: no reset, no resize", async () => {
    const { send } = await setup("observe", pane, false);
    await send(0, reset());
    expect(fake.calls).toEqual([]);
  });

  it("sizes an observer to the controller instead of fitting, on the snapshot and on resized", async () => {
    const { send } = await setup("observe", pane, true);
    await send(0, reset(90, 20));
    await send(0, { type: "resized", cols: 120, rows: 40 });
    expect(fake.calls).toEqual(["reset", "resize 90x20", "resize 120x40"]);
  });

  it("sets the PTY size of an unfocused emulated observer when its container resizes", async () => {
    const { fixture, settings, session, open } = await setup("observe", pane, true);
    fake.nextCols = 100;
    settings.set({ ...DEFAULT_SETTINGS, fontSize: 10 });
    fixture.detectChanges();
    await fixture.whenStable();
    expect(open).toHaveBeenCalledTimes(1);
    expect(session.resize).toHaveBeenCalledWith(100, 24);
  });

  it("renders at the controller's size when the server refuses an observer's resize", async () => {
    const { fixture, settings, session, sized, send } = await setup("observe", pane, true);
    await send(0, reset(90, 20));
    session.resize.mockRejectedValueOnce(new Error("not_controller"));
    fake.nextCols = 100;
    settings.set({ ...DEFAULT_SETTINGS, fontSize: 10 });
    fixture.detectChanges();
    await fixture.whenStable();
    expect(session.resize).toHaveBeenCalledWith(100, 24);
    expect(fake.calls.at(-1)).toBe("resize 90x20");
    expect(sized.at(-1)).toEqual({ cols: 90, rows: 20 });
  });

  it("gives a controller its own size back after the snapshot and tells the server", async () => {
    const { send, session } = await setup("control", pane, true);
    await send(0, attached);
    await send(0, reset(100, 30));
    await send(0, bytes(1));
    // The fit takes xterm back to the window (80 columns here) and the server follows.
    expect(session.resize).toHaveBeenCalledWith(80, 24);
  });

  it("ignores the resized echo of its own size as a controller", async () => {
    const { send } = await setup("control", pane, true);
    await send(0, attached);
    await send(0, { type: "resized", cols: 80, rows: 24 });
    expect(fake.calls).toEqual([]);
  });

  it("keeps xterm's own scrollback from the settings on an emulated stream only", async () => {
    const emulated = await setup("observe", pane, true);
    expect(scrollbackOf().scrollback).toBe(DEFAULT_SETTINGS.scrollback);
    emulated.settings.set({ ...DEFAULT_SETTINGS, scrollback: 500 });
    emulated.fixture.detectChanges();
    expect(scrollbackOf().scrollback).toBe(500);
    TestBed.resetTestingModule();
    await setup("observe", pane, false);
    expect(scrollbackOf().scrollback).toBe(0);
  });

  it("tells xterm.js it runs over ConPTY on Windows, for an emulated stream only", async () => {
    vi.stubGlobal("navigator", { platform: "Win32" });
    try {
      await setup("observe", pane, true);
      const options = () => (fake.term as unknown as { options: { windowsPty?: unknown } }).options;
      expect(options().windowsPty).toEqual({ backend: "conpty", buildNumber: 21376 });
      TestBed.resetTestingModule();
      await setup("observe", pane, false);
      expect(options().windowsPty).toBeUndefined();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("leaves windowsPty unset off Windows", async () => {
    vi.stubGlobal("navigator", { platform: "MacIntel" });
    try {
      await setup("observe", pane, true);
      expect(
        (fake.term as unknown as { options: { windowsPty?: unknown } }).options.windowsPty,
      ).toBeUndefined();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("sends typed input and terminal answers from the controller only", async () => {
    const observer = await setup("observe", pane, true);
    await observer.send(0, reset());
    fake.onData?.("\x1b[?1;2c"); // a device attributes answer
    expect(observer.session.input).not.toHaveBeenCalled();
    TestBed.resetTestingModule();

    const controller = await setup("control", pane, true);
    fake.onData?.("early");
    expect(controller.session.input).not.toHaveBeenCalled(); // not the controller yet
    await controller.send(0, attached);
    fake.onData?.("ls\r");
    expect(controller.session.input).toHaveBeenCalledWith("ls\r");
  });

  it("keeps the last screen with a banner when the process exits, and does not reattach", async () => {
    const { send, open, fixture, modes } = await setup("control", pane, true);
    await send(0, attached);
    await send(0, {
      type: "closed",
      reason: "the process exited with code 3",
      exit_code: 3,
      code: "exited",
    });
    expect(fixture.componentInstance.banner()).toBe("the process exited with code 3");
    expect(open).toHaveBeenCalledTimes(1);
    expect(modes.at(-1)).toEqual({ mode: "observe", blocked: null });
    expect(fake.calls).not.toContain("reset");
  });

  it("keeps the scrollback of an emulated stream after the process exited", async () => {
    const { send, settings, fixture } = await setup("control", pane, true);
    await send(0, attached);
    await send(0, { type: "closed", reason: "the process exited", exit_code: 0, code: "exited" });
    settings.set({ ...DEFAULT_SETTINGS, scrollback: 500 });
    fixture.detectChanges();
    expect(scrollbackOf().scrollback).toBe(500);
  });

  it("attaches again in its mode after the server went away, without a takeover", async () => {
    const frame: StreamRecord = { type: "reset", cols: 80, rows: 24, bytes: "" };
    const { send, open, fixture, modes } = await setup("control", pane, true);
    await send(0, frame);
    await send(0, attached);
    await send(0, {
      type: "closed",
      reason: "the connection to poimen-server ended: eof",
      code: "lost",
    });
    expect(fixture.componentInstance.banner()).toBe(REATTACHING);
    expect(modes.at(-1)).toEqual({ mode: "observe", blocked: null });
    expect(open).toHaveBeenCalledTimes(1);
    await new Promise((resolve) => setTimeout(resolve, REATTACH_DELAYS_MS[0] + 50));
    expect(open).toHaveBeenCalledTimes(2);
    expect(open.mock.calls[1][0]).toMatchObject({ mode: "control", takeover: false });
    await send(1, frame);
    await send(1, attached);
    expect(fixture.componentInstance.banner()).toBeNull();
    expect(modes.at(-1)).toEqual({ mode: "control", blocked: null });
  });

  it("keeps trying while no server answers yet", async () => {
    const { send, open, fixture } = await setup("observe", pane, true);
    open.mockImplementationOnce(() => Promise.reject("no server"));
    await send(0, { type: "closed", reason: "gone", code: "lost" });
    await new Promise((resolve) => setTimeout(resolve, REATTACH_DELAYS_MS[0] + 50));
    expect(open).toHaveBeenCalledTimes(2);
    expect(fixture.componentInstance.banner()).toBe(REATTACHING);
    await new Promise((resolve) => setTimeout(resolve, REATTACH_DELAYS_MS[1] + 50));
    expect(open).toHaveBeenCalledTimes(3);
    expect(open.mock.calls[2][0]).toMatchObject({ mode: "observe", takeover: false });
  });

  it("stops trying when the view is destroyed", async () => {
    const { send, open, fixture } = await setup("observe", pane, true);
    await send(0, { type: "closed", reason: "gone", code: "lost" });
    fixture.destroy();
    await new Promise((resolve) => setTimeout(resolve, REATTACH_DELAYS_MS[0] + 50));
    expect(open).toHaveBeenCalledTimes(1);
  });

  it("closes silently when the server detaches the stream", async () => {
    const { send, open, fixture } = await setup("observe", pane, true);
    await send(0, { type: "closed", reason: "detached" });
    expect(fixture.componentInstance.banner()).toBeNull();
    expect(open).toHaveBeenCalledTimes(1);
  });

  it("shows the read-only state with the input action when the controller is held", async () => {
    const { send, open, fixture, modes } = await setup("control", pane, true);
    await send(0, {
      type: "closed",
      reason: "another client controls this terminal",
      code: "controller_held",
    });
    expect(modes.at(-1)).toEqual({
      mode: "observe",
      blocked: "another client controls this terminal",
    });
    expect(fixture.nativeElement.textContent).toContain("Take over");
    expect(open).toHaveBeenCalledTimes(2);
    expect(open.mock.calls[1][0]).toMatchObject({ mode: "observe", takeover: false });
  });
});
