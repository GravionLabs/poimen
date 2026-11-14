// @vitest-environment jsdom
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { buildTree, qualifySnapshot, type Snapshot } from "../../core/snapshot";
import { SpacesStore } from "../../core/spaces-store";
import { Machines, type MachineCheck, type MachineStatus } from "../../core/machines";
import { MachinesSection, checkNote, machineNote } from "./machines-section";

const box = { name: "box", ssh: "box", reposDir: "~/Source/{owner}/{repo}" };

const snap: Snapshot = {
  spaces: [{ space_id: "s1", label: "api", number: 1, agent_status: "idle" }],
  tabs: [{ tab_id: "tab1", space_id: "s1", label: "shell", number: 1, agent_status: "idle" }],
  terminals: [{ terminal_id: "t1", tab_id: "tab1", space_id: "s1", agent_status: "idle" }],
};

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized
  }
});

function setup(
  status: MachineStatus,
  checks: Record<string, MachineCheck | { error: string }> = {},
) {
  const reconnect = vi.fn(async () => {});
  const check = vi.fn(async () => {});
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      {
        provide: Machines,
        useValue: { machines: signal([status]), checks: signal(checks), reconnect, check },
      },
      {
        provide: SpacesStore,
        useValue: {
          remoteTree: () => buildTree(qualifySnapshot("box", snap, status.state !== "connected")),
        },
      },
    ],
  });
  const fixture = TestBed.createComponent(MachinesSection);
  const picked: unknown[] = [];
  fixture.componentInstance.selected.subscribe((t) => picked.push(t));
  fixture.detectChanges();
  return { fixture, el: fixture.nativeElement as HTMLElement, reconnect, check, picked };
}

describe("MachinesSection", () => {
  it("lists a connected machine's spaces and opens a terminal on click", () => {
    const { el, picked } = setup({ ...box, state: "connected", serverVersion: "0.17.0" });
    expect(el.querySelector(".machine-name")?.textContent).toBe("box");
    expect(el.querySelector(".space-name")?.textContent).toBe("api");
    const row = el.querySelector<HTMLButtonElement>(".row")!;
    expect(row.disabled).toBe(false);
    row.click();
    expect(picked).toHaveLength(1);
  });

  it("greys an offline machine's last known spaces and makes them inert", () => {
    const { el, picked, reconnect } = setup({ ...box, state: "offline", error: "ssh: timeout" });
    expect(el.querySelector(".machine")?.classList.contains("offline")).toBe(true);
    expect(el.querySelector(".note")?.textContent).toContain("Offline: ssh: timeout");
    const row = el.querySelector<HTMLButtonElement>(".row")!;
    expect(row.disabled).toBe(true);
    row.click();
    expect(picked).toEqual([]);
    el.querySelector<HTMLButtonElement>(".reconnect")!.click();
    expect(reconnect).toHaveBeenCalledWith("box");
  });

  it("offers Reconnect in the context menu of a machine", () => {
    const { fixture, el, reconnect } = setup({ ...box, state: "offline", error: "x" });
    el.querySelector(".machine-head")!.dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
    );
    fixture.detectChanges();
    el.querySelector<HTMLButtonElement>(".menu-item")!.click();
    expect(reconnect).toHaveBeenCalledWith("box");
  });
});

describe("machineNote", () => {
  it("shows the version message of an incompatible machine", () => {
    const note = machineNote({
      ...box,
      state: "incompatible",
      serverVersion: "0.1.0",
      serverProtocol: 1,
      appVersion: "0.17.0",
      appProtocol: 6,
    });
    expect(note).toContain("poimen-server 0.1.0 (protocol 1)");
    expect(note).toContain("poimen 0.17.0 (protocol 6)");
  });

  it("describes the other states", () => {
    expect(machineNote({ ...box, state: "connecting" })).toBe("Connecting…");
    expect(machineNote({ ...box, state: "offline", error: "" })).toBe("Offline");
  });

  it("runs the machine check from the context menu and shows its result", () => {
    const failed: MachineCheck = {
      version: "0.17.0",
      protocol: 7,
      checks: [{ id: "git", status: "missing", detail: null, fix: null }],
    };
    const { fixture, el, check } = setup(
      { ...box, state: "connected", serverVersion: "0.17.0" },
      { box: failed },
    );
    expect(el.querySelector(".check")?.textContent).toContain("missing git");
    el.querySelector(".machine-head")!.dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
    );
    fixture.detectChanges();
    const items = el.querySelectorAll<HTMLButtonElement>(".menu-item");
    expect(items[1].textContent).toContain("Check");
    items[1].click();
    expect(check).toHaveBeenCalledWith("box");
  });

  it("disables Check while the machine is not connected", () => {
    const { fixture, el } = setup({ ...box, state: "offline", error: "x" });
    el.querySelector(".machine-head")!.dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
    );
    fixture.detectChanges();
    expect(el.querySelectorAll<HTMLButtonElement>(".menu-item")[1].disabled).toBe(true);
  });

  it("summarizes a check", () => {
    expect(checkNote(undefined)).toBeNull();
    expect(checkNote({ error: "boom" })).toBe("Check failed: boom");
    expect(checkNote({ version: "1", protocol: 1, checks: [] })).toBe("Check passed");
  });
});
