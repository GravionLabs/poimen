import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Machines, parseMachineStatus } from "./machines";
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
