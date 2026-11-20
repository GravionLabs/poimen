// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { Machines } from "../core/machines";
import { SettingsStore } from "../core/settings";
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
    await vi.waitFor(() => {
      fixture.detectChanges();
      expect(el.querySelector(".copy")!.textContent).toContain("Copied");
    });
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

  it("explains what a machine needs and offers Add machine when none is configured", async () => {
    const { el } = await setup([]);
    expect(el.textContent).toContain("without a password prompt");
    expect(el.textContent).toContain("poimen-server");
    expect(el.querySelector(".add")!.textContent).toContain("Add machine");
  });

  it("saves a new machine from the form and shows the errors per field", async () => {
    const { fixture, el } = await setup([]);
    el.querySelector<HTMLButtonElement>(".add")!.click();
    fixture.detectChanges();
    const field = (n: string) => el.querySelector<HTMLInputElement>(`input[name=${n}]`)!;
    const type = (n: string, v: string) => {
      field(n).value = v;
      field(n).dispatchEvent(new Event("input"));
    };
    tauri.answer("machine_save", () => {
      throw [{ field: "ssh", message: 'ssh "-o" must be a host, not an option' }];
    });
    type("name", "box");
    type("ssh", "-o");
    el.querySelector("form")!.dispatchEvent(new Event("submit", { cancelable: true }));
    await vi.waitFor(() =>
      expect(tauri.invoke).toHaveBeenCalledWith("machine_save", expect.anything()),
    );
    await fixture.whenStable();
    fixture.detectChanges();
    expect(el.querySelector("form [role=alert]")!.textContent).toContain("not an option");
    expect(field("name").value).toBe("box");

    tauri.answer("machine_save", () => undefined);
    type("ssh", "me@box");
    el.querySelector("form")!.dispatchEvent(new Event("submit", { cancelable: true }));
    await vi.waitFor(() =>
      expect(tauri.invoke).toHaveBeenLastCalledWith("list_machines", undefined),
    );
    expect(tauri.invoke).toHaveBeenCalledWith("machine_save", {
      original: null,
      name: "box",
      ssh: "me@box",
      reposDir: "",
    });
    await fixture.whenStable();
    fixture.detectChanges();
    expect(el.querySelector("form")).toBeNull();
  });

  it("suggests the hosts of the SSH config, fills name and target and keeps free input", async () => {
    const { fixture, el } = await setup([]);
    tauri.answer("ssh_config_hosts", () => [
      { host: "box", configured: false },
      { host: "old", configured: true },
    ]);
    el.querySelector<HTMLButtonElement>(".add")!.click();
    fixture.detectChanges();
    await vi.waitFor(() => {
      fixture.detectChanges();
      expect(el.querySelector(".suggestion")).not.toBeNull();
    });
    const buttons = [...el.querySelectorAll<HTMLButtonElement>(".suggestion")];
    expect(buttons.map((b) => b.disabled)).toEqual([false, true]);
    buttons[0]!.click();
    fixture.detectChanges();
    const field = (n: string) => el.querySelector<HTMLInputElement>(`input[name=${n}]`)!;
    expect(field("ssh").value).toBe("box");
    expect(field("name").value).toBe("box");
    field("ssh").value = "me@elsewhere";
    field("ssh").dispatchEvent(new Event("input"));
    expect(field("ssh").value).toBe("me@elsewhere");
  });

  it("shows no suggestions when the SSH config cannot be read", async () => {
    const { fixture, el } = await setup([]);
    tauri.answer("ssh_config_hosts", () => {
      throw new Error("x");
    });
    el.querySelector<HTMLButtonElement>(".add")!.click();
    fixture.detectChanges();
    await fixture.whenStable();
    fixture.detectChanges();
    expect(el.querySelector(".suggestion")).toBeNull();
    expect(el.querySelector("input[name=ssh]")).not.toBeNull();
  });

  it("edits a machine with its values and removes it after a confirmation", async () => {
    const { fixture, el } = await setup([
      { ...box, reposDir: "/srv/{owner}/{repo}", state: "connected", serverVersion: "1" },
    ]);
    el.querySelector<HTMLButtonElement>(".edit")!.click();
    fixture.detectChanges();
    expect(el.querySelector<HTMLInputElement>("input[name=reposDir]")!.value).toBe(
      "/srv/{owner}/{repo}",
    );
    expect(el.querySelector("h2")!.textContent).toContain("Edit machine");
    el.querySelector<HTMLButtonElement>("form .cancel")!.click();
    fixture.detectChanges();
    expect(el.querySelector("form")).toBeNull();

    el.querySelector<HTMLButtonElement>(".remove")!.click();
    fixture.detectChanges();
    expect(el.textContent).toContain("terminals keep running");
    expect(tauri.invoke).not.toHaveBeenCalledWith("machine_remove", expect.anything());
    el.querySelector<HTMLButtonElement>("app-dialog-shell .confirm")!.click();
    await vi.waitFor(() =>
      expect(tauri.invoke).toHaveBeenCalledWith("machine_remove", { name: "box" }),
    );
  });

  it("lists the warnings of invalid machine entries", async () => {
    tauri.reset();
    tauri.answer("list_machines", () => []);
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: [
        {
          provide: SettingsStore,
          useValue: {
            settings: () => ({ warnings: ["machines[1]: ssh is missing; ignored", "theme: x"] }),
          },
        },
      ],
    });
    const fixture = TestBed.createComponent(MachinesPage);
    fixture.detectChanges();
    const alerts = [...(fixture.nativeElement as HTMLElement).querySelectorAll(".warning")];
    expect(alerts.map((a) => a.textContent)).toEqual(["machines[1]: ssh is missing; ignored"]);
  });

  it("shows the state tag, Check and Reconnect on each row", async () => {
    const { fixture, el } = await setup();
    expect(el.querySelector(".state")!.textContent).toContain("connected, server 1");
    el.querySelector<HTMLButtonElement>(".check")!.click();
    await vi.waitFor(() =>
      expect(tauri.invoke).toHaveBeenCalledWith("machine_check", { name: "box" }),
    );
    el.querySelector<HTMLButtonElement>(".reconnect")!.click();
    expect(tauri.invoke).toHaveBeenCalledWith("reconnect_machine", { name: "box" });
    await fixture.whenStable();
  });

  it("shows what to do for an unknown host key and disables Check while offline", async () => {
    const { el } = await setup([
      { ...box, state: "offline", error: "Host key verification failed." },
    ]);
    expect(el.querySelector(".state")!.textContent).toContain("Host key verification failed");
    expect(el.querySelector(".guidance")!.textContent).toContain("accept it");
    expect(el.querySelector(".fix")!.textContent).toBe("ssh box");
    expect(el.querySelector(".hint-copy")).not.toBeNull();
    expect(el.querySelector<HTMLButtonElement>(".check")!.disabled).toBe(true);
  });

  it("shows an icon per check status and runs a check again after it", async () => {
    const { fixture, el } = await setup();
    const machines = TestBed.inject(Machines);
    await machines.check("box");
    fixture.detectChanges();
    const icons = [...el.querySelectorAll(".check-row .status")].map((s) => s.textContent!.trim());
    expect(icons).toEqual(["✓ ok", "✕ missing", "! warning"]);
    expect(machines.running().size).toBe(0);
    const before = tauri.invoke.mock.calls.filter((c) => c[0] === "machine_check").length;
    await machines.check("box");
    expect(tauri.invoke.mock.calls.filter((c) => c[0] === "machine_check").length).toBe(before + 1);
  });

  it("labels a check id", () => {
    expect(checkLabel("agent-claude")).toBe("agent claude");
  });
});
