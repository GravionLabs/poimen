// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { Machines, serverInstallOffer, type MachineStatus } from "../core/machines";
import { ServerInstall } from "../core/server-install";
import { tauri } from "../core/tauri.mock";
import { expectFocusInside, focusOpener, press } from "../ui/dialog-shell/dialog-contract";
import { MachinesPage } from "./machines-page";

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

const box = { name: "box", ssh: "me@box", reposDir: "~/Source/{owner}/{repo}" };
const missing = { ...box, state: "offline", error: "bash: poimen-server: command not found" };
const incompatible = {
  ...box,
  state: "incompatible",
  serverVersion: "0.9.0",
  serverProtocol: 3,
  appVersion: "0.23.0",
  appProtocol: 7,
};

const PLAN = {
  machine: "box",
  ssh: "me@box",
  version: "0.23.0",
  channel: "stable",
  target: "x86_64-unknown-linux-musl",
  archive: "poimen-server-0.23.0-x86_64-unknown-linux-musl.tar.gz",
  origin: "https://github.com/GravionLabs/poimen/releases/download/v0.23.0/a.tar.gz",
  sha256: "ab".repeat(32),
  size: 3 * 1024 * 1024,
  dir: "~/.local/bin",
  server: "poimen-server",
  host: "poimen-pty-host",
  existing: null,
  steps: [
    { id: "mkdir", label: "Create the target directory", command: 'mkdir -p "$HOME/.local/bin"' },
    { id: "copy", label: "Copy the archive", command: "tar -xzf - -C stage" },
  ],
};

async function setup(machine: unknown, plan: () => unknown = () => PLAN) {
  tauri.reset();
  tauri.answer("list_machines", () => [machine]);
  tauri.answer("machine_install_plan", plan);
  tauri.answer("machine_install_run", () => undefined);
  TestBed.resetTestingModule();
  const fixture = TestBed.createComponent(MachinesPage);
  await TestBed.inject(Machines).refresh();
  fixture.detectChanges();
  const el = fixture.nativeElement as HTMLElement;
  const open = async () => {
    el.querySelector<HTMLButtonElement>(".install")!.click();
    await fixture.whenStable();
    fixture.detectChanges();
  };
  return { fixture, el, open, install: TestBed.inject(ServerInstall) };
}

describe("server install offer", () => {
  it("is offered for a missing or an incompatible server and for nothing else", () => {
    const m = (x: unknown) => x as MachineStatus;
    expect(serverInstallOffer(m(missing))).toBe("install");
    expect(serverInstallOffer(m(incompatible))).toBe("update");
    expect(serverInstallOffer(m({ ...box, state: "connected", serverVersion: "1" }))).toBeNull();
    expect(
      serverInstallOffer(m({ ...box, state: "offline", error: "Permission denied (publickey)." })),
    ).toBeNull();
    expect(serverInstallOffer(m({ ...box, state: "connecting" }))).toBeNull();
  });
});

describe("Install server dialog", () => {
  it("shows no button for a connected machine", async () => {
    const { el } = await setup({ ...box, state: "connected", serverVersion: "1" });
    expect(el.querySelector(".install")).toBeNull();
  });

  it("plans on open, shows machine, version, path and commands and runs nothing yet", async () => {
    const { el, open } = await setup(missing);
    expect(el.querySelector(".install")!.textContent).toContain("Install server…");
    await open();
    expect(tauri.calls("machine_install_plan")).toEqual([{ name: "box" }]);
    expect(tauri.calls("machine_install_run")).toEqual([]);
    const text = el.querySelector("dialog")!.textContent!;
    expect(text).toContain("box (me@box)");
    expect(text).toContain("0.23.0 (stable)");
    expect(text).toContain("~/.local/bin/poimen-server");
    expect(text).toContain("3.0 MiB");
    expect(el.querySelector(".commands")!.textContent).toContain("tar -xzf - -C stage");
    expect(el.querySelector(".confirm")!.textContent).toContain("Install on box");
  });

  it("offers Update for an incompatible server and names the version found", async () => {
    const { el, open } = await setup(incompatible);
    expect(el.querySelector(".install")!.textContent).toContain("Update server…");
    await open();
    expect(el.querySelector("dialog")!.textContent).toContain("replacing version 0.9.0");
    expect(el.querySelector(".confirm")!.textContent).toContain("Update on box");
  });

  it("has the focus on Cancel, Enter does not confirm, Cancel drops the plan and runs nothing", async () => {
    const opener = focusOpener();
    const { fixture, el, open } = await setup(missing);
    await open();
    await fixture.whenStable();
    expectFocusInside(el);
    expect(document.activeElement?.textContent).toContain("Cancel");
    press("Enter");
    expect(tauri.calls("machine_install_run")).toEqual([]);
    press("Escape");
    await fixture.whenStable();
    fixture.detectChanges();
    expect(el.querySelector("dialog")).toBeNull();
    expect(tauri.calls("machine_install_cancel")).toEqual([{ name: "box" }]);
    expect(tauri.calls("machine_install_run")).toEqual([]);
    expect(document.activeElement).toBe(opener);
  });

  it("runs on the confirmation, shows progress per step and the check runs again after", async () => {
    const { fixture, el, open, install } = await setup(missing);
    await open();
    el.querySelector<HTMLButtonElement>(".confirm")!.click();
    await fixture.whenStable();
    expect(tauri.calls("machine_install_run")).toEqual([{ name: "box" }]);
    tauri.emit("machine:install_step", { machine: "box", step: "mkdir", status: "ok", output: "" });
    expect(install.state()!.steps.map((s) => s.status)).toContain("ok");
    expect(install.state()!.phase).toBe("done");
    // The next connect of the machine checks it again.
    tauri.answer("machine_check", () => ({ version: "0.23.0", protocol: 7, checks: [] }));
    await TestBed.inject(Machines).start();
    tauri.emit("machine:state_changed", { ...box, state: "connected", serverVersion: "0.23.0" });
    await fixture.whenStable();
    expect(tauri.calls("machine_check")).toEqual([{ name: "box" }]);
  });

  it("shows ssh's output of a failed step and stops", async () => {
    const { fixture, el, open, install } = await setup(missing);
    tauri.answer("machine_install_run", () => {
      throw "Copy the archive failed: tar: Child returned status 1";
    });
    await open();
    tauri.emit("machine:install_step", {
      machine: "box",
      step: "copy",
      status: "failed",
      output: "tar: Child returned status 1",
    });
    el.querySelector<HTMLButtonElement>(".confirm")!.click();
    await fixture.whenStable();
    fixture.detectChanges();
    expect(install.state()!.phase).toBe("failed");
    expect(el.querySelector("[role=alert]")!.textContent).toContain("Child returned status 1");
    expect(el.querySelector(".output")!.textContent).toContain("tar: Child returned status 1");
    expect(el.querySelector(".confirm")).toBeNull();
  });

  it("shows a failed plan (checksum, ssh) without a confirm button", async () => {
    const { fixture, el, open } = await setup(missing, () => {
      throw "archive does not match SHA256SUMS";
    });
    await open();
    fixture.detectChanges();
    expect(el.querySelector("[role=alert]")!.textContent).toContain("does not match SHA256SUMS");
    expect(el.querySelector(".confirm")).toBeNull();
    expect(tauri.calls("machine_install_run")).toEqual([]);
  });
});
