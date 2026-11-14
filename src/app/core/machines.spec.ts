import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Machines, checkPassed, parseMachineCheck, parseMachineStatus } from "./machines";
import { TauriBridge } from "./tauri";
import { tauri } from "./tauri.mock";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));

const box = { name: "box", ssh: "box", reposDir: "~/Source/{owner}/{repo}" };

const create = () => Injector.create({ providers: [TauriBridge, Machines] }).get(Machines);

beforeEach(() => tauri.reset());

describe("parseMachineStatus", () => {
  it("reads every state of the wire", () => {
    expect(parseMachineStatus({ ...box, state: "connecting" })).toEqual({
      ...box,
      state: "connecting",
    });
    expect(parseMachineStatus({ ...box, state: "connected", serverVersion: "0.17.0" })).toEqual({
      ...box,
      state: "connected",
      serverVersion: "0.17.0",
    });
    expect(
      parseMachineStatus({ ...box, state: "offline", error: "ssh: Permission denied" }),
    ).toEqual({ ...box, state: "offline", error: "ssh: Permission denied" });
    expect(
      parseMachineStatus({
        ...box,
        state: "incompatible",
        serverVersion: "0.1.0",
        serverProtocol: 1,
        appVersion: "0.17.0",
        appProtocol: 6,
      }),
    ).toEqual({
      ...box,
      state: "incompatible",
      serverVersion: "0.1.0",
      serverProtocol: 1,
      appVersion: "0.17.0",
      appProtocol: 6,
    });
  });

  it("rejects an unknown state and a machine without a name", () => {
    expect(parseMachineStatus({ ...box, state: "sleeping" })).toBeNull();
    expect(parseMachineStatus({ ssh: "box", state: "connecting" })).toBeNull();
    expect(parseMachineStatus(null)).toBeNull();
  });
});

describe("Machines", () => {
  it("lists the machines and follows machine:state_changed", async () => {
    tauri.answer("list_machines", () => [{ ...box, state: "connecting" }, { bad: 1 }]);
    const m = create();
    await m.start();
    expect(m.machines()).toEqual([{ ...box, state: "connecting" }]);
    tauri.emit("machine:state_changed", { ...box, state: "offline", error: "ssh: timeout" });
    expect(m.machines()).toEqual([{ ...box, state: "offline", error: "ssh: timeout" }]);
  });

  it("reads the list again for a machine it does not know yet", async () => {
    let list: unknown[] = [];
    tauri.answer("list_machines", () => list);
    const m = create();
    await m.start();
    list = [{ ...box, state: "connected", serverVersion: "0.17.0" }];
    tauri.emit("machine:state_changed", list[0]);
    await vi.waitFor(() => expect(m.machines()).toHaveLength(1));
  });

  it("asks for a reconnect by name", async () => {
    const asked: unknown[] = [];
    tauri.answer("reconnect_machine", (args) => {
      asked.push(args);
      return null;
    });
    await create().reconnect("box");
    expect(asked).toEqual([{ name: "box" }]);
  });
});

describe("parseMachineCheck", () => {
  it("reads checks with detail and fix and drops malformed ones", () => {
    const check = parseMachineCheck({
      version: "0.18.0",
      protocol: 6,
      checks: [
        { id: "git", status: "ok" },
        { id: "gh_auth", status: "missing", detail: "not logged in", fix: "gh auth login" },
        { id: "copilot", status: "warning" },
        { id: "x", status: "bogus" },
        { status: "ok" },
      ],
    });
    expect(check?.checks).toEqual([
      { id: "git", status: "ok", detail: null, fix: null },
      { id: "gh_auth", status: "missing", detail: "not logged in", fix: "gh auth login" },
      { id: "copilot", status: "warning", detail: null, fix: null },
    ]);
    expect(checkPassed(check!)).toBe(false);
  });

  it("passes with only warnings and rejects a non-list", () => {
    expect(
      checkPassed(parseMachineCheck({ checks: [{ id: "copilot", status: "warning" }] })!),
    ).toBe(true);
    expect(parseMachineCheck({ checks: 1 })).toBeNull();
    expect(parseMachineCheck(null)).toBeNull();
  });
});

describe("Machines.check", () => {
  const result = { version: "1", protocol: 6, checks: [{ id: "git", status: "ok" }] };

  it("checks a machine at its first connect only", async () => {
    const asked: unknown[] = [];
    tauri.answer("list_machines", () => [{ ...box, state: "connecting" }]);
    tauri.answer("machine_check", (args) => {
      asked.push(args);
      return result;
    });
    const m = create();
    await m.start();
    const up = { ...box, state: "connected", serverVersion: "1" };
    tauri.emit("machine:state_changed", up);
    await vi.waitFor(() => expect(m.checks()["box"]).toBeDefined());
    tauri.emit("machine:state_changed", { ...box, state: "offline", error: "x" });
    tauri.emit("machine:state_changed", up);
    expect(asked).toEqual([{ name: "box" }]);
  });

  it("checks a machine that is already connected when the list is read, once", async () => {
    const asked: unknown[] = [];
    tauri.answer("list_machines", () => [{ ...box, state: "connected", serverVersion: "1" }]);
    tauri.answer("machine_check", (args) => {
      asked.push(args);
      return result;
    });
    const m = create();
    await m.start();
    await vi.waitFor(() => expect(m.checks()["box"]).toBeDefined());
    await m.refresh();
    tauri.emit("machine:state_changed", { ...box, state: "connected", serverVersion: "1" });
    await Promise.resolve();
    expect(asked).toEqual([{ name: "box" }]);
  });

  it("checks again on demand and keeps the error when it cannot run", async () => {
    tauri.answer("machine_check", () => {
      throw new Error("machine is not connected");
    });
    const m = create();
    await m.check("box");
    expect(m.checks()["box"]).toEqual({ error: "Error: machine is not connected" });
    tauri.answer("machine_check", () => result);
    await m.check("box");
    expect(m.checks()["box"]).toMatchObject({ checks: [{ id: "git", status: "ok" }] });
  });
});
