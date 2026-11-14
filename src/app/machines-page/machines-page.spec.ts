// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { Machines } from "../core/machines";
import { tauri } from "../core/tauri.mock";
import { MachinesPage, checkLabel } from "./machines-page";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});
afterEach(() => TestBed.resetTestingModule());

const box = { name: "box", ssh: "box", reposDir: "~/Source/{owner}/{repo}" };

const RESULT = {
  version: "0.17.0",
  protocol: 6,
  checks: [
    { id: "git", status: "ok" },
    { id: "gh", status: "missing", detail: "not found", fix: "brew install gh" },
    { id: "agent-copilot", status: "warning", fix: "npm i -g @github/copilot" },
  ],
};

async function setup(machines: unknown[] = [{ ...box, state: "connected", serverVersion: "1" }]) {
  tauri.reset();
  tauri.answer("list_machines", () => machines);
  tauri.answer("machine_check", () => RESULT);
  TestBed.resetTestingModule();
  const fixture = TestBed.createComponent(MachinesPage);
  await TestBed.inject(Machines).refresh();
  fixture.detectChanges();
  const el = fixture.nativeElement as HTMLElement;
  return { fixture, el };
}

const rightClick = (el: HTMLElement) =>
  el
    .querySelector(".machine")!
    .dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 10, clientY: 20 }),
    );

describe("MachinesPage", () => {
  it("says so when no machine is configured", async () => {
    const { el } = await setup([]);
    expect(el.textContent).toContain("No machine is configured.");
  });

  it("offers Check machine in the context menu and runs the check", async () => {
    const { fixture, el } = await setup();
    expect(el.querySelector("[role=menu]")).toBeNull();
    rightClick(el);
    fixture.detectChanges();
    const item = el.querySelector<HTMLButtonElement>("[role=menuitem]")!;
    expect(item.textContent).toContain("Check machine");
    item.click();
    await vi.waitFor(() =>
      expect(tauri.invoke).toHaveBeenCalledWith("machine_check", { name: "box" }),
    );
    await fixture.whenStable();
    fixture.detectChanges();
    expect(el.querySelector("[role=menu]")).toBeNull();
  });

  it("shows one row per check with status and the fix command", async () => {
    const { fixture, el } = await setup();
    await TestBed.inject(Machines).check("box");
    fixture.detectChanges();
    const rows = [...el.querySelectorAll(".check-row")];
    expect(rows.map((r) => r.getAttribute("data-status"))).toEqual(["ok", "missing", "warning"]);
    expect(rows[1].querySelector(".fix")!.textContent).toBe("brew install gh");
    expect(rows[2].textContent).toContain("agent copilot");
    expect(rows[0].querySelector(".fix")).toBeNull();
    expect(el.querySelector(".summary")!.textContent).toContain("Missing");
  });

  it("copies the fix without running anything", async () => {
    const { fixture, el } = await setup();
    const writeText = vi.fn(async () => {});
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    await TestBed.inject(Machines).check("box");
    fixture.detectChanges();
    const calls = tauri.invoke.mock.calls.length;
    el.querySelector<HTMLButtonElement>(".copy")!.click();
    expect(writeText).toHaveBeenCalledWith("brew install gh");
    expect(tauri.invoke.mock.calls.length).toBe(calls);
  });

  it("shows the error when the check could not run", async () => {
    const { fixture, el } = await setup();
    tauri.answer("machine_check", () => {
      throw new Error("offline");
    });
    await TestBed.inject(Machines).check("box");
    fixture.detectChanges();
    expect(el.querySelector("[role=alert]")!.textContent).toContain("offline");
  });

  it("labels a check id", () => {
    expect(checkLabel("agent-claude")).toBe("agent claude");
  });
});
