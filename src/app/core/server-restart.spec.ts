// @vitest-environment jsdom
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { HerdrStore, type BackendStatus } from "./herdr-store";
import { ServerRestart, serverNotice } from "./server-restart";
import { TauriBridge } from "./tauri";

const outdated: BackendStatus = {
  state: "outdated",
  reason: null,
  running: "0.11.0",
  bundled: "0.12.0",
};

type Pane = { terminal_id: string; label?: string; survives_restart?: boolean };

function setup(
  status: BackendStatus | null,
  invoke = vi.fn(),
  terminals: Pane[] = [{ terminal_id: "t1", label: "build" }, { terminal_id: "t2" }],
) {
  const backend = signal<BackendStatus | null>(status);
  const ensureBackend = vi.fn(async () => backend.set({ state: "running", reason: null }));
  const store = {
    backend,
    ensureBackend,
    snapshot: signal({ spaces: [], tabs: [], terminals }),
  };
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      { provide: TauriBridge, useValue: { invoke } },
      { provide: HerdrStore, useValue: store },
    ],
  });
  return { restart: TestBed.inject(ServerRestart), invoke, ensureBackend, backend };
}

describe("serverNotice", () => {
  it("names both versions of an outdated server", () => {
    expect(serverNotice(outdated)).toBe("poimen-server 0.11.0 is running, 0.12.0 is installed");
  });

  it("says a server with another protocol does not match", () => {
    const notice = serverNotice({
      state: "incompatible",
      reason: null,
      running: "0.5.0",
      protocol: 2,
    });
    expect(notice).toContain("0.5.0");
    expect(notice).toContain("protocol 2");
    expect(notice).toContain("does not match");
  });

  it("is empty for every other state", () => {
    expect(serverNotice(null)).toBeNull();
    expect(serverNotice({ state: "running", reason: null })).toBeNull();
    expect(serverNotice({ state: "failed", reason: "x" })).toBeNull();
  });
});

describe("ServerRestart", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
      teardown: { destroyAfterEach: true },
    });
  });

  it("offers the notice for an outdated server and Later hides it for the session", () => {
    const { restart, invoke } = setup(outdated);
    expect(restart.notice()).toContain("0.11.0");
    restart.later();
    expect(restart.notice()).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("stops nothing until the confirmation, which names the terminals", () => {
    const { restart, invoke } = setup(outdated);
    restart.askRestart();
    expect(restart.action()).toBe("restart");
    expect(restart.terminals()).toEqual(["build", "t2"]);
    restart.cancel();
    expect(restart.action()).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("a restart ends only the terminals the next server does not adopt, a quit all", () => {
    const panes = [
      { terminal_id: "t1", label: "build", survives_restart: true },
      { terminal_id: "t2", survives_restart: true },
      { terminal_id: "t3", label: "old" },
    ];
    const { restart } = setup(outdated, vi.fn(), panes);
    restart.askRestart();
    expect(restart.terminals()).toEqual(["old"]);
    expect(restart.kept()).toBe(2);
    restart.askQuit();
    expect(restart.terminals()).toEqual(["build", "t2", "old"]);
    expect(restart.kept()).toBe(0);
  });

  it("a restart with every terminal in a host ends none", () => {
    const panes = [{ terminal_id: "t1", survives_restart: true }];
    const { restart } = setup(outdated, vi.fn(), panes);
    restart.askRestart();
    expect(restart.terminals()).toEqual([]);
    expect(restart.kept()).toBe(1);
  });

  it("restarts after the confirmation and checks the backend again", async () => {
    const invoke = vi.fn(async () => ({ state: "started", reason: null }));
    const { restart, ensureBackend } = setup(outdated, invoke);
    restart.askRestart();
    await restart.confirm();
    expect(invoke).toHaveBeenCalledWith("restart_server");
    expect(ensureBackend).toHaveBeenCalled();
    expect(restart.action()).toBeNull();
    expect(restart.notice()).toBeNull();
  });

  it("keeps the confirmation open with the error when the restart fails", async () => {
    const invoke = vi.fn(async () => ({ state: "failed", reason: "did not stop" }));
    const { restart, ensureBackend } = setup(outdated, invoke);
    restart.askRestart();
    await restart.confirm();
    expect(restart.error()).toBe("did not stop");
    expect(restart.action()).toBe("restart");
    expect(ensureBackend).not.toHaveBeenCalled();
  });

  it("quits and stops the server only after the confirmation", async () => {
    const invoke = vi.fn(async () => undefined);
    const { restart } = setup({ state: "running", reason: null }, invoke);
    restart.askQuit();
    expect(restart.action()).toBe("quit");
    expect(invoke).not.toHaveBeenCalled();
    await restart.confirm();
    expect(invoke).toHaveBeenCalledWith("quit_and_stop_server");
  });

  it("shows a rejected quit and stays open", async () => {
    const invoke = vi.fn(() => Promise.reject("could not stop"));
    const { restart } = setup({ state: "running", reason: null }, invoke);
    restart.askQuit();
    await restart.confirm();
    expect(restart.error()).toBe("could not stop");
    expect(restart.action()).toBe("quit");
  });
});
