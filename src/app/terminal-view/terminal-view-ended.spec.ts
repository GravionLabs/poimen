// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { Terminal } from "../core/snapshot";
import type { StreamRecord } from "../core/backend";
import { SettingsStore, DEFAULT_SETTINGS } from "../core/settings";
import { SpaceActions } from "../core/space-actions";
import { TauriBridge } from "../core/tauri";
import { TerminalStreamService } from "../core/terminal-stream";

const fake = vi.hoisted(() => ({ selection: "" }));

vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    options: Record<string, unknown> = {};
    cols = 80;
    rows = 24;
    buffer = { active: { viewportY: 0 } };
    unicode = { activeVersion: "6" };
    loadAddon() {}
    open() {}
    registerLinkProvider() {}
    onData() {}
    attachCustomWheelEventHandler() {}
    attachCustomKeyEventHandler() {}
    focus() {}
    getSelection() {
      return fake.selection;
    }
    write() {}
    reset() {}
    resize() {}
    dispose() {}
  },
}));
vi.mock("@xterm/addon-unicode11", () => ({ Unicode11Addon: class {} }));
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit() {}
  },
}));
vi.mock("@xterm/addon-web-links", () => ({ WebLinksAddon: class {} }));
vi.mock("@xterm/addon-search", () => ({
  SearchAddon: class {
    onDidChangeResults() {
      return { dispose() {} };
    }
  },
}));
vi.mock("@xterm/addon-image", () => ({ ImageAddon: class {} }));
vi.mock("@xterm/addon-webgl", () => ({ WebglAddon: class {} }));

import { TerminalView } from "./terminal-view";

const pane: Terminal = {
  terminal_id: "w1:p1",
  tab_id: "w1:t1",
  space_id: "w1",
  agent_status: "idle",
};

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

async function setup() {
  const records: ((record: StreamRecord) => void)[] = [];
  const open = vi.fn(async (_opts: unknown, onRecord: (record: StreamRecord) => void) => {
    records.push(onRecord);
    return { open: true, resize: vi.fn(), close: vi.fn(), input: vi.fn() };
  });
  TestBed.configureTestingModule({
    providers: [
      { provide: SettingsStore, useValue: { settings: signal(DEFAULT_SETTINGS) } },
      { provide: TerminalStreamService, useValue: { open } },
      { provide: TauriBridge, useValue: { openUrl: vi.fn(), invokeRaw: vi.fn() } },
      { provide: SpaceActions, useValue: { notify: vi.fn() } },
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
  const body = document.createElement("div");
  Object.assign(fixture.componentInstance, {
    body: () => ({ nativeElement: body }),
    pane: signal(pane),
    wanted: signal("observe"),
  });
  fixture.detectChanges();
  await fixture.whenStable();
  await vi.waitFor(() => expect(records.length).toBeGreaterThan(0));
  const send = async (record: StreamRecord) => {
    records[0](record);
    await fixture.whenStable();
  };
  return { fixture, body, send };
}

const ended: StreamRecord = {
  type: "closed",
  reason: "the terminal ended; this is its history, read-only",
  code: "exited",
};

function copyEvent() {
  const data = new Map<string, string>();
  const event = new Event("copy", { bubbles: true, cancelable: true }) as ClipboardEvent;
  Object.defineProperty(event, "clipboardData", {
    value: { setData: (k: string, v: string) => data.set(k, v), getData: () => "" },
  });
  return { event, data };
}

describe("TerminalView of an ended terminal (#1233)", () => {
  it("copies the selection after the closed record with code exited", async () => {
    const { body, send } = await setup();
    await send({ type: "reset", cols: 80, rows: 24 });
    await send(ended);
    fake.selection = "git status";
    const { event, data } = copyEvent();
    body.dispatchEvent(event);
    expect(data.get("text/plain")).toBe("git status");
    expect(event.defaultPrevented).toBe(true);
  });

  it("opens a Copy menu on the context menu event, and only once ended", async () => {
    const { body, send, fixture } = await setup();
    const before = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
    body.dispatchEvent(before);
    expect(before.defaultPrevented).toBe(false);
    await send(ended);
    fixture.detectChanges();
    const menu = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
    body.dispatchEvent(menu);
    fixture.detectChanges();
    expect(menu.defaultPrevented).toBe(true);
    expect(fixture.nativeElement.querySelector("app-context-menu")).not.toBeNull();
  });
});
